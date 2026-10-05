//! Is the texts (Classic) pairing healthy? Seen on hardware: after pairing for notifications,
//! the iPhone can keep a texts pairing Windows no longer matches. Windows still lists the
//! phone, but every connection fails ("unreachable") while the notifications link to the same
//! phone is up, and nothing recovers until the texts pairing is made again. Once that has gone
//! on long enough to rule out a phone that's just restarting Bluetooth, say so.

use std::time::{Duration, Instant};

use serde::Serialize;

/// One attempt to open message access, as far as pairing health is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attempt {
    /// Session open.
    Connected,
    /// The phone answered (e.g. "switch is off"): the pairing works.
    Answered,
    /// Windows has no texts pairing with any phone.
    NoDevice,
    /// The connection never reached the phone, or the phone didn't offer message access.
    Unreachable,
    /// Anything else (timeouts, a dropped session): says nothing about the pairing.
    Other,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TextsPairing {
    #[default]
    Unknown,
    /// Not paired for texts yet.
    Missing,
    /// Paired in Windows, but the phone won't take the connection: pair for texts again.
    Broken,
    Ok,
}

/// The phone recovered on its own within ~30 s in testing; the broken case never did.
const BROKEN_AFTER: Duration = Duration::from_secs(90);
const BROKEN_MIN_FAILURES: u32 = 5;

#[derive(Debug, Default)]
pub struct Health {
    state: TextsPairing,
    failing_since: Option<Instant>,
    failures: u32,
}

impl Health {
    pub fn state(&self) -> TextsPairing {
        self.state
    }

    /// `phone_linked`: the notifications link is up, so the phone is in range and awake.
    pub fn record(&mut self, attempt: Attempt, phone_linked: bool, now: Instant) -> TextsPairing {
        match attempt {
            Attempt::Connected | Attempt::Answered => self.reset(TextsPairing::Ok),
            Attempt::NoDevice => self.reset(TextsPairing::Missing),
            Attempt::Unreachable if phone_linked => {
                let since = *self.failing_since.get_or_insert(now);
                self.failures += 1;
                if self.failures >= BROKEN_MIN_FAILURES && now.duration_since(since) >= BROKEN_AFTER {
                    self.state = TextsPairing::Broken;
                }
            }
            // Out of range says nothing about the pairing; start counting afresh when back.
            Attempt::Unreachable => {
                self.failing_since = None;
                self.failures = 0;
            }
            Attempt::Other => {}
        }
        self.state
    }

    fn reset(&mut self, state: TextsPairing) {
        self.state = state;
        self.failing_since = None;
        self.failures = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, secs: u64) -> Instant {
        start + Duration::from_secs(secs)
    }

    #[test]
    fn steady_failures_with_the_phone_nearby_mean_broken() {
        let t = Instant::now();
        let mut h = Health::default();
        h.record(Attempt::Answered, true, t);
        for s in (5..90).step_by(5) {
            assert_eq!(
                h.record(Attempt::Unreachable, true, at(t, s)),
                TextsPairing::Ok,
                "too early at {s}s"
            );
        }
        assert_eq!(h.record(Attempt::Unreachable, true, at(t, 95)), TextsPairing::Broken);
        // Fixed by pairing again.
        assert_eq!(h.record(Attempt::Connected, true, at(t, 120)), TextsPairing::Ok);
    }

    #[test]
    fn a_phone_restarting_bluetooth_is_not_broken() {
        let t = Instant::now();
        let mut h = Health::default();
        for s in [0, 5, 10, 20, 30] {
            h.record(Attempt::Unreachable, true, at(t, s));
        }
        assert_eq!(h.record(Attempt::Answered, true, at(t, 35)), TextsPairing::Ok);
        assert_eq!(h.record(Attempt::Unreachable, true, at(t, 40)), TextsPairing::Ok);
    }

    #[test]
    fn out_of_range_never_counts() {
        let t = Instant::now();
        let mut h = Health::default();
        for s in (0..600).step_by(30) {
            assert_eq!(h.record(Attempt::Unreachable, false, at(t, s)), TextsPairing::Unknown);
        }
        // Back in range: the clock starts again.
        h.record(Attempt::Unreachable, true, at(t, 600));
        assert_eq!(h.record(Attempt::Unreachable, true, at(t, 650)), TextsPairing::Unknown);
    }

    #[test]
    fn two_slow_failures_are_not_enough() {
        let t = Instant::now();
        let mut h = Health::default();
        h.record(Attempt::Unreachable, true, t);
        assert_eq!(h.record(Attempt::Unreachable, true, at(t, 300)), TextsPairing::Unknown);
    }

    #[test]
    fn no_texts_pairing_at_all_is_missing() {
        let mut h = Health::default();
        assert_eq!(h.record(Attempt::NoDevice, true, Instant::now()), TextsPairing::Missing);
        assert_eq!(h.record(Attempt::Other, true, Instant::now()), TextsPairing::Missing);
    }
}
