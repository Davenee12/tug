//! Reading a phone's zoneless time as UTC, so a Bluetooth text can be compared with a backup's.
//!
//! Over Bluetooth the iPhone often gives a text's time as its own local time with no zone
//! (`2026-10-04T19:11:17`), while a backup's times are UTC. Guessing (allowing for any offset up
//! to 14 hours) merged two different texts with the same words hours apart, so a zoneless time is
//! only used once it's turned into UTC with an offset tug actually knows:
//!
//! - the PC's own time-zone rules for that date (which know daylight saving), when the phone is in
//!   the PC's zone now or tug hasn't heard the phone's offset;
//! - the phone's own offset (Bluetooth Current Time Service, read with the Bluetooth inventory),
//!   when it differs from the PC's (the phone is travelling);
//! - nothing, when neither is known: then the zoneless time isn't compared at all.

/// Where a zoneless phone time's offset comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalZone {
    /// This PC's zone, or the phone's when it reports a different one (see the module docs).
    System,
    /// A fixed offset from UTC, in minutes (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    Fixed(i32),
    /// Unknown: zoneless times aren't compared.
    #[cfg_attr(not(test), allow(dead_code))]
    Unknown,
}

impl LocalZone {
    /// What's needed to turn many zoneless times into UTC, looked up once.
    pub fn resolver(self) -> Resolver {
        match self {
            LocalZone::System => Resolver::System {
                phone_now: phone_offset_now(),
                pc_now: pc::offset_now(),
            },
            LocalZone::Fixed(m) => Resolver::Fixed(m),
            LocalZone::Unknown => Resolver::Unknown,
        }
    }
}

/// A `LocalZone` with the phone's and the PC's current offsets already looked up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolver {
    System {
        phone_now: Option<i32>,
        pc_now: Option<i32>,
    },
    Fixed(i32),
    Unknown,
}

impl Resolver {
    /// A zoneless local time (ms, its digits read as if UTC) as real UTC ms, or `None` when the
    /// offset isn't known.
    pub fn to_utc(self, local_ms: i64) -> Option<i64> {
        match self {
            Resolver::Fixed(m) => Some(local_ms - i64::from(m) * 60_000),
            Resolver::Unknown => None,
            Resolver::System { phone_now, pc_now } => {
                choose(local_ms, phone_now, pc_now, || pc::local_to_utc(local_ms))
            }
        }
    }
}

/// Which offset to trust for `local_ms`: the phone's own when it's in another zone than the PC
/// right now; otherwise the PC's rules for that date (`pc_at`, which knows daylight saving), or the
/// phone's offset if the PC can't say; `None` when nothing is known.
fn choose(
    local_ms: i64,
    phone_now: Option<i32>,
    pc_now: Option<i32>,
    pc_at: impl FnOnce() -> Option<i64>,
) -> Option<i64> {
    let by_phone = |m: i32| local_ms - i64::from(m) * 60_000;
    match (phone_now, pc_now) {
        (Some(phone), Some(here)) if phone != here => Some(by_phone(phone)),
        _ => pc_at().or_else(|| phone_now.map(by_phone)),
    }
}

/// The phone's offset from UTC as it last told tug (Current Time Service), if it has.
fn phone_offset_now() -> Option<i32> {
    crate::bt_inventory::last_report()
        .and_then(|r| r.current_time.ok().and_then(|c| c.utc_offset_minutes))
        // A real offset is within -12:00..+14:00.
        .filter(|m| (-12 * 60..=14 * 60).contains(m))
}

#[cfg(windows)]
mod pc {
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::System::Time::{SystemTimeToTzSpecificLocalTime, TzSpecificLocalTimeToSystemTime};

    use crate::backup::import::{civil_from_days, days_from_civil};

    fn to_systemtime(ms: i64) -> Option<SYSTEMTIME> {
        let secs = ms.div_euclid(1000);
        let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
        let rem = secs.rem_euclid(86_400);
        Some(SYSTEMTIME {
            wYear: u16::try_from(y).ok().filter(|y| (1601..=30827).contains(y))?,
            wMonth: m as u16,
            wDayOfWeek: 0,
            wDay: d as u16,
            wHour: (rem / 3600) as u16,
            wMinute: (rem % 3600 / 60) as u16,
            wSecond: (rem % 60) as u16,
            wMilliseconds: ms.rem_euclid(1000) as u16,
        })
    }

    fn from_systemtime(t: &SYSTEMTIME) -> i64 {
        let days = days_from_civil(i64::from(t.wYear), i64::from(t.wMonth), i64::from(t.wDay));
        ((days * 24 + i64::from(t.wHour)) * 60 + i64::from(t.wMinute)) * 60_000
            + i64::from(t.wSecond) * 1000
            + i64::from(t.wMilliseconds)
    }

    /// A local time on this PC (its zone's rules for that date) as UTC.
    pub fn local_to_utc(local_ms: i64) -> Option<i64> {
        let local = to_systemtime(local_ms)?;
        let mut utc = SYSTEMTIME::default();
        // SAFETY: plain FFI call on two stack values that outlive it; no time zone = the PC's own.
        unsafe { TzSpecificLocalTimeToSystemTime(None, &local, &mut utc) }.ok()?;
        Some(from_systemtime(&utc))
    }

    /// This PC's offset from UTC right now, in minutes.
    pub fn offset_now() -> Option<i32> {
        let now = crate::state::now_ms();
        let utc = to_systemtime(now)?;
        let mut local = SYSTEMTIME::default();
        // SAFETY: as above.
        unsafe { SystemTimeToTzSpecificLocalTime(None, &utc, &mut local) }.ok()?;
        i32::try_from((from_systemtime(&local) - now).div_euclid(60_000)).ok()
    }
}

#[cfg(not(windows))]
mod pc {
    pub fn local_to_utc(_local_ms: i64) -> Option<i64> {
        None
    }

    pub fn offset_now() -> Option<i32> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 3_600_000;
    const T: i64 = 1_760_000_000_000;

    #[test]
    fn a_known_offset_turns_local_time_into_utc() {
        // 04:53 local, four hours behind UTC, is 08:53 UTC.
        assert_eq!(Resolver::Fixed(-240).to_utc(T - 4 * H), Some(T));
        assert_eq!(Resolver::Fixed(330).to_utc(T + 5 * H + H / 2), Some(T));
        assert_eq!(Resolver::Unknown.to_utc(T), None, "unknown: never guessed");
    }

    #[test]
    fn the_phone_wins_only_when_it_is_somewhere_else() {
        let pc_rules = || Some(T + 60_000);
        // Same zone as the PC: the PC's rules for that date (they know daylight saving).
        assert_eq!(choose(T, Some(-240), Some(-240), pc_rules), Some(T + 60_000));
        // The phone is travelling: its own offset.
        assert_eq!(choose(T, Some(60), Some(-240), pc_rules), Some(T - H));
        // The phone hasn't said: the PC's rules; the PC can't say: the phone's offset.
        assert_eq!(choose(T, None, Some(-240), pc_rules), Some(T + 60_000));
        assert_eq!(choose(T, Some(-240), None, || None), Some(T + 4 * H));
        // Nothing known: no time at all.
        assert_eq!(choose(T, None, None, || None), None);
    }

    #[cfg(windows)]
    #[test]
    fn the_pcs_own_zone_round_trips() {
        // Whatever zone the PC is in, now in local time and back is now (to the minute).
        let now = crate::state::now_ms() / 60_000 * 60_000;
        let offset = pc::offset_now().expect("Windows knows its zone");
        let local = now + i64::from(offset) * 60_000;
        assert_eq!(pc::local_to_utc(local), Some(now));
    }
}
