//! Sync Contacts, judged from the iPhone's answers: when to check its phonebook (PBAP) again, what
//! a check needs to pull, and when an empty answer means the switch is off. Pure, so the schedule
//! is unit-tested without a phone; the message worker (`service`) does the checking and mirrors the
//! result into `DeviceStatus` (`contacts_shared`, `contacts_off`).
//!
//! The iPhone answers a pull with an empty phonebook, not a refusal, while this PC's Sync Contacts
//! switch is off (it defaults off after every re-pair). One empty answer can be the moment before
//! the switch is flipped, so it takes `OFF_AFTER_EMPTY` in a row to say "off" (the second asked
//! `CONFIRM_EMPTY` after the first); after that the UI shows a definite off instead of "Checking…".
//!
//! The schedule (set by the owner): a routine check every `ROUTINE` (10 min), and a check when it
//! matters: the texts connection (re)opening, a text from a number tug has no name for (at most
//! once per `UNKNOWN_SENDER_GAP`), "Check again", and every `QUICK_RETRY` while the iPhone's
//! switches are on screen (so flipping Sync Contacts either way shows within seconds). Coming back
//! to tug's window doesn't check. None of these delays a send: the worker drops a check for one.
//!
//! A check is cheap when the phone allows it (`after_check`): one PBAP connection asking only for
//! the phonebook's size and its version counters (PBAP 1.2: the folder version counters and the
//! database identifier, which the iPhone advertises). The names are pulled only when those changed,
//! on the first check of a texts connection, or, from a phone that doesn't send counters, on each
//! routine check.

use std::time::Duration;

/// While the switches are on screen (setup, Settings › iPhone): check this often, either way.
pub const QUICK_RETRY: Duration = Duration::from_secs(10);
/// Otherwise: a routine check this often, whatever the last answer was.
pub const ROUTINE: Duration = Duration::from_secs(10 * 60);
/// The first empty answer is confirmed this soon (the second makes it "off"), so the switch shows
/// as off within a minute of connecting rather than after a routine wait.
pub const CONFIRM_EMPTY: Duration = Duration::from_secs(30);
/// A check that failed outright while the switches are on screen: try again after this, not every
/// `QUICK_RETRY` (a check that hangs ties the worker's PBAP turn up until its timeout).
pub const FAILED_RETRY_WATCHING: Duration = Duration::from_secs(2 * 60);
/// An asked-for check ("Check again", the switches coming on screen) this soon after the last one
/// waits out the gap instead: each is a whole PBAP connection.
pub const CHECK_GAP: Duration = Duration::from_secs(3);
/// A text from a number tug has no name for asks again at most this often (a new contact on the
/// phone shows up without waiting for the routine check; a stranger texting can't cause a stream).
pub const UNKNOWN_SENDER_GAP: Duration = Duration::from_secs(2 * 60);
/// Empty answers in a row before Sync Contacts counts as off.
pub const OFF_AFTER_EMPTY: u32 = 2;
/// Log every this-many empty answers in a row at debug (plus the first), not every check.
const EMPTY_LOG_EVERY: u32 = 20;

/// What the phone said about its phonebook without sending it: the answer to PullPhoneBook with
/// MaxListCount 0. Each part is `None` when the phone didn't send it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Fingerprint {
    /// PhonebookSize.
    pub size: Option<u16>,
    /// PrimaryVersionCounter: changes when anything in any contact changes.
    pub primary: Option<[u8; 16]>,
    /// SecondaryVersionCounter: changes when a name, number, email or address changes (what tug
    /// reads), so a photo or a note edited on the phone doesn't cost a pull.
    pub secondary: Option<[u8; 16]>,
    /// DatabaseIdentifier: changes when the phone rebuilt its contacts (counters start over).
    pub database: Option<[u8; 16]>,
}

impl Fingerprint {
    /// The counter that says whether the names changed: the secondary, or the primary if that's
    /// all the phone sends.
    fn counter(&self) -> Option<[u8; 16]> {
        self.secondary.or(self.primary)
    }

    /// The phone sends version counters (PBAP 1.2), so "unchanged" can be trusted.
    pub fn has_counters(&self) -> bool {
        self.counter().is_some() && self.database.is_some()
    }
}

/// What a check without the names means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckAnswer {
    /// No contacts: the switch is off. Only trusted from a phone that has reported a real size
    /// before, so one that always says 0 can't make tug claim the switch is off.
    Empty,
    /// The same as when the names were last pulled, and still shared: nothing to pull.
    Unchanged,
    /// Pull the names.
    Pull,
}

/// Whether a check must pull the names. `now`: what the phone just said. `last`: what it said at
/// the last pull that came back with contacts on this texts connection (None: the first check, so
/// pull). `zero_trusted`: this phone has reported a non-zero size a pull bore out. `routine`: the
/// 10-minute check, which pulls from a phone without counters (nothing else would see a renamed
/// contact); a quick or triggered check settles for the size there.
pub fn after_check(now: &Fingerprint, last: Option<&Fingerprint>, zero_trusted: bool, routine: bool) -> CheckAnswer {
    if now.size == Some(0) && zero_trusted {
        return CheckAnswer::Empty;
    }
    let Some(last) = last else {
        return CheckAnswer::Pull;
    };
    // An untrusted zero, or nothing to compare: the names settle it (an empty pull is cheap).
    if now.size == Some(0) || (now.size.is_none() && !now.has_counters()) {
        return CheckAnswer::Pull;
    }
    let same_size = now.size == last.size;
    if now.has_counters() && last.has_counters() {
        let same = same_size && now.counter() == last.counter() && now.database == last.database;
        return if same {
            CheckAnswer::Unchanged
        } else {
            CheckAnswer::Pull
        };
    }
    if !routine && same_size && now.size.is_some_and(|n| n > 0) {
        return CheckAnswer::Unchanged;
    }
    CheckAnswer::Pull
}

/// What one phonebook check came back with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pull {
    /// People in it: the switch is on.
    Shared,
    /// An empty phonebook: the switch is off (or about to be flipped on).
    Empty,
    /// The phone refused contact access outright: off.
    Refused,
    /// Timed out or failed for another reason: says nothing about the switch.
    Failed,
}

/// A change worth one log line (and the UI's toggle moving).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// The phone is sharing again after it was judged off.
    On,
    /// The phone stopped sharing (or never started): Sync Contacts is off.
    Off,
}

/// What to do after a check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// When to check again.
    pub next: Duration,
    pub transition: Option<Transition>,
    /// Whether this empty answer gets its (rate-limited) debug line.
    pub log_empty: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ContactsWatch {
    /// The phone shared contacts on the current texts connection.
    shared: bool,
    /// Sync Contacts is judged off. Kept across a texts reconnect (the next check settles it within
    /// seconds) so a flaky link doesn't flicker the toggle between off and "Checking…".
    off: bool,
    empty_in_a_row: u32,
}

impl ContactsWatch {
    pub fn shared(&self) -> bool {
        self.shared
    }

    pub fn off(&self) -> bool {
        self.off
    }

    /// Fold one check into the state; returns when to check next and any transition to log.
    /// `quick`: the iPhone's switches are on screen.
    pub fn record(&mut self, pull: Pull, quick: bool) -> Outcome {
        match pull {
            Pull::Shared => {
                let transition = self.off.then_some(Transition::On);
                self.shared = true;
                self.off = false;
                self.empty_in_a_row = 0;
                Outcome {
                    next: if quick { QUICK_RETRY } else { ROUTINE },
                    transition,
                    log_empty: false,
                }
            }
            Pull::Empty | Pull::Refused => {
                self.shared = false;
                self.empty_in_a_row = self.empty_in_a_row.saturating_add(1);
                let now_off = pull == Pull::Refused || self.empty_in_a_row >= OFF_AFTER_EMPTY;
                let transition = (now_off && !self.off).then_some(Transition::Off);
                self.off |= now_off;
                let n = self.empty_in_a_row;
                let next = match (quick, now_off) {
                    (true, _) => QUICK_RETRY,
                    (false, false) => CONFIRM_EMPTY,
                    (false, true) => ROUTINE,
                };
                Outcome {
                    next,
                    transition,
                    log_empty: pull == Pull::Empty && (n == 1 || n.is_multiple_of(EMPTY_LOG_EVERY)),
                }
            }
            Pull::Failed => Outcome {
                next: if quick { FAILED_RETRY_WATCHING } else { ROUTINE },
                transition: None,
                log_empty: false,
            },
        }
    }

    /// The texts session dropped: what the phone shares is per connection, so "shared" waits for
    /// the next check (made right after the reconnect). A judged "off" is kept until then.
    pub fn connection_dropped(&mut self) {
        self.shared = false;
        self.empty_in_a_row = 0;
    }

    /// A different phone, or none (Forget): nothing carries over.
    pub fn forget(&mut self) {
        *self = Self::default();
    }
}

/// When an asked-for check should run: right away, or `CHECK_GAP` after the last check if that was
/// more recent. `None` when a check is already due by then, so asking again ("Check again" right
/// after the switches came on screen) changes nothing and costs nothing.
pub fn check_now_at<I>(now: I, last_pull: Option<I>, next_pull: I) -> Option<I>
where
    I: Copy + Ord + std::ops::Add<Duration, Output = I>,
{
    let at = last_pull.map_or(now, |last| now.max(last + CHECK_GAP));
    (at < next_pull).then_some(at)
}

/// The "text from a number tug has no name for" trigger: at most once per `UNKNOWN_SENDER_GAP`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerGate<I> {
    last: Option<I>,
}

impl<I> Default for TriggerGate<I> {
    fn default() -> Self {
        Self { last: None }
    }
}

impl<I> TriggerGate<I>
where
    I: Copy + Ord + std::ops::Add<Duration, Output = I>,
{
    /// Whether this trigger may check now (and if so, counts it).
    pub fn allow(&mut self, now: I) -> bool {
        if self.last.is_some_and(|last| now < last + UNKNOWN_SENDER_GAP) {
            return false;
        }
        self.last = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const fn s(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    fn fp(size: u16, counter: u8, database: u8) -> Fingerprint {
        Fingerprint {
            size: Some(size),
            primary: Some([counter.wrapping_add(100); 16]),
            secondary: Some([counter; 16]),
            database: Some([database; 16]),
        }
    }

    fn size_only(size: u16) -> Fingerprint {
        Fingerprint {
            size: Some(size),
            ..Fingerprint::default()
        }
    }

    #[test]
    fn the_routine_check_is_every_ten_minutes_whatever_the_answer() {
        let mut w = ContactsWatch::default();
        assert_eq!(w.record(Pull::Shared, false).next, s(600));
        assert_eq!(w.record(Pull::Shared, false).next, s(600));
        assert_eq!(w.record(Pull::Failed, false).next, s(600));
        assert_eq!(w.record(Pull::Refused, false).next, s(600));
        for _ in 0..50 {
            assert_eq!(w.record(Pull::Empty, false).next, s(600), "never a 30 s loop while off");
        }
    }

    #[test]
    fn a_first_empty_answer_is_confirmed_in_30s_then_it_is_off() {
        let mut w = ContactsWatch::default();
        let first = w.record(Pull::Empty, false);
        assert_eq!(first.next, CONFIRM_EMPTY);
        assert_eq!(
            first.transition, None,
            "one empty answer can be the moment before the flip"
        );
        assert!(!w.off());
        let second = w.record(Pull::Empty, false);
        assert_eq!(
            second.transition,
            Some(Transition::Off),
            "repeated empties are a definite off"
        );
        assert_eq!(second.next, ROUTINE);
        assert!(w.off() && !w.shared());
        assert_eq!(
            w.record(Pull::Empty, false).transition,
            None,
            "the off transition is logged once"
        );
    }

    #[test]
    fn with_the_switches_on_screen_it_checks_every_10s_either_way() {
        let mut w = ContactsWatch::default();
        for _ in 0..20 {
            assert_eq!(w.record(Pull::Empty, true).next, s(10));
        }
        let on = w.record(Pull::Shared, true);
        assert_eq!(on.transition, Some(Transition::On), "on from off in one check");
        assert_eq!(on.next, s(10), "still quick, to see it go off again");
        assert_eq!(w.record(Pull::Failed, true).next, FAILED_RETRY_WATCHING);
    }

    #[test]
    fn first_ever_share_is_not_an_off_to_on_transition() {
        let mut w = ContactsWatch::default();
        assert_eq!(w.record(Pull::Shared, false).transition, None);
        assert!(w.shared());
    }

    #[test]
    fn on_then_empty_twice_is_off_and_a_refusal_is_off_at_once() {
        let mut w = ContactsWatch::default();
        w.record(Pull::Shared, false);
        assert_eq!(w.record(Pull::Empty, false).transition, None);
        assert!(!w.shared(), "an empty answer is never 'on'");
        assert_eq!(w.record(Pull::Empty, false).transition, Some(Transition::Off));
        let mut r = ContactsWatch::default();
        assert_eq!(r.record(Pull::Refused, false).transition, Some(Transition::Off));
        assert!(r.off());
    }

    #[test]
    fn a_failed_check_changes_nothing() {
        let mut w = ContactsWatch::default();
        w.record(Pull::Shared, false);
        let o = w.record(Pull::Failed, false);
        assert_eq!(o.transition, None);
        assert!(w.shared());
    }

    #[test]
    fn unchanged_counters_mean_no_pull() {
        let last = fp(120, 7, 1);
        assert_eq!(
            after_check(&fp(120, 7, 1), Some(&last), true, true),
            CheckAnswer::Unchanged
        );
        assert_eq!(
            after_check(&fp(120, 7, 1), Some(&last), true, false),
            CheckAnswer::Unchanged
        );
        // A photo edit moves only the primary counter: still nothing tug reads changed.
        let mut photo = fp(120, 7, 1);
        photo.primary = Some([0xEE; 16]);
        assert_eq!(after_check(&photo, Some(&last), true, true), CheckAnswer::Unchanged);
    }

    #[test]
    fn a_changed_counter_size_or_database_means_a_pull() {
        let last = fp(120, 7, 1);
        assert_eq!(
            after_check(&fp(120, 8, 1), Some(&last), true, true),
            CheckAnswer::Pull,
            "a rename"
        );
        assert_eq!(
            after_check(&fp(121, 8, 1), Some(&last), true, false),
            CheckAnswer::Pull,
            "added"
        );
        assert_eq!(
            after_check(&fp(120, 7, 2), Some(&last), true, true),
            CheckAnswer::Pull,
            "rebuilt"
        );
    }

    #[test]
    fn the_first_check_of_a_connection_always_pulls() {
        assert_eq!(after_check(&fp(120, 7, 1), None, true, false), CheckAnswer::Pull);
        assert_eq!(after_check(&size_only(120), None, true, false), CheckAnswer::Pull);
    }

    #[test]
    fn without_counters_the_routine_check_pulls_and_quick_ones_go_by_size() {
        let last = size_only(120);
        assert_eq!(after_check(&size_only(120), Some(&last), true, true), CheckAnswer::Pull);
        assert_eq!(
            after_check(&size_only(120), Some(&last), true, false),
            CheckAnswer::Unchanged
        );
        assert_eq!(
            after_check(&size_only(121), Some(&last), true, false),
            CheckAnswer::Pull
        );
        let nothing = Fingerprint::default();
        assert_eq!(
            after_check(&nothing, Some(&last), true, false),
            CheckAnswer::Pull,
            "said nothing"
        );
    }

    #[test]
    fn zero_means_off_only_from_a_phone_whose_sizes_proved_real() {
        assert_eq!(
            after_check(&fp(0, 7, 1), Some(&fp(120, 7, 1)), true, false),
            CheckAnswer::Empty
        );
        assert_eq!(after_check(&size_only(0), None, true, false), CheckAnswer::Empty);
        assert_eq!(after_check(&size_only(0), None, false, false), CheckAnswer::Pull);
        assert_eq!(
            after_check(&size_only(0), Some(&size_only(0)), false, false),
            CheckAnswer::Pull,
            "an untrusted zero is confirmed by pulling (an empty pull is cheap)"
        );
    }

    #[test]
    fn an_asked_for_check_pulls_now_unless_one_is_already_due() {
        let t0 = Instant::now();
        assert_eq!(check_now_at(t0, None, t0 + s(60)), Some(t0));
        assert_eq!(
            check_now_at(t0, None, t0),
            None,
            "already due: the second ask does nothing"
        );
        let last = t0 - s(1);
        assert_eq!(check_now_at(t0, Some(last), t0 + s(60)), Some(last + CHECK_GAP));
        assert_eq!(check_now_at(t0, Some(last), last + CHECK_GAP), None);
        assert_eq!(check_now_at(t0, Some(t0 - s(600)), t0 + s(30)), Some(t0));
    }

    #[test]
    fn unknown_senders_check_at_most_every_two_minutes() {
        let t0 = Instant::now();
        let mut gate = TriggerGate::default();
        assert!(gate.allow(t0), "the first one checks");
        assert!(!gate.allow(t0 + s(1)), "a burst of texts from strangers is one check");
        assert!(!gate.allow(t0 + s(119)));
        assert!(gate.allow(t0 + s(120)));
        assert!(!gate.allow(t0 + s(150)), "counted from the last one that checked");
    }

    #[test]
    fn empty_lines_are_rate_limited() {
        let mut w = ContactsWatch::default();
        let logged: Vec<u32> = (1..=60).filter(|_| w.record(Pull::Empty, false).log_empty).collect();
        assert_eq!(logged.len(), 4, "the first, then every 20th: {logged:?}");
    }

    #[test]
    fn a_reconnect_keeps_off_but_clears_shared_and_forget_clears_all() {
        let mut w = ContactsWatch::default();
        w.record(Pull::Empty, false);
        w.record(Pull::Empty, false);
        w.connection_dropped();
        assert!(w.off(), "no flicker to 'Checking…' on a texts reconnect");
        assert_eq!(w.record(Pull::Empty, false).transition, None);
        w.record(Pull::Shared, false);
        w.connection_dropped();
        assert!(!w.shared(), "shared is per connection");
        w.forget();
        assert_eq!(w, ContactsWatch::default());
    }
}
