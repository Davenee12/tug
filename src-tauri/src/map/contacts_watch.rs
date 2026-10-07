//! Sync Contacts, judged from the iPhone's answers: when to pull its phonebook (PBAP) again, and
//! when an empty answer means the switch is off. Pure, so the schedule is unit-tested without a
//! phone; the message worker (`service`) does the pulling and mirrors the result into
//! `DeviceStatus` (`contacts_shared`, `contacts_off`).
//!
//! The iPhone answers a pull with an empty phonebook, not a refusal, while this PC's Sync Contacts
//! switch is off (it defaults off after every re-pair). One empty answer can be the moment before
//! the switch is flipped, so it takes `OFF_AFTER_EMPTY` in a row to say "off"; after that the UI
//! shows a definite off instead of "Checking…" forever. tug keeps asking often — every
//! `UNSHARED_RETRY`, `WATCHING_RETRY` while the switches are on screen — so turning the switch on
//! shows up within one quick pull, never minutes later. Once shared, changes are picked up every
//! `RESYNC` (the phone sends nothing when a contact is added or renamed).

use std::time::Duration;

/// While the phone isn't sharing: ask again this often, for as long as the texts session is open.
/// Each ask is one bounded PBAP connection.
pub const UNSHARED_RETRY: Duration = Duration::from_secs(30);
/// While the switches are on screen (setup, Settings › iPhone): ask this often.
pub const WATCHING_RETRY: Duration = Duration::from_secs(10);
/// Once shared: look for added or renamed contacts this often (a pull takes about a second).
pub const RESYNC: Duration = Duration::from_secs(5 * 60);
/// A pull that failed outright (timed out, link dropped): try again after this. Not the empty
/// answer of a switch that's off — that's `UNSHARED_RETRY`.
pub const FAILED_RETRY: Duration = Duration::from_secs(2 * 60);
/// Empty answers in a row before Sync Contacts counts as off.
pub const OFF_AFTER_EMPTY: u32 = 2;
/// Log every this-many empty answers in a row at debug (plus the first), not every pull.
const EMPTY_LOG_EVERY: u32 = 20;

/// What one phonebook pull came back with.
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

/// What to do after a pull.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// When to pull again.
    pub next: Duration,
    pub transition: Option<Transition>,
    /// Whether this empty answer gets its (rate-limited) debug line.
    pub log_empty: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ContactsWatch {
    /// The phone shared contacts on the current texts connection.
    shared: bool,
    /// Sync Contacts is judged off. Kept across a texts reconnect (the next pull settles it within
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

    /// Fold one pull into the state; returns when to pull next and any transition to log.
    pub fn record(&mut self, pull: Pull, watching: bool) -> Outcome {
        let unshared = if watching { WATCHING_RETRY } else { UNSHARED_RETRY };
        match pull {
            Pull::Shared => {
                let transition = self.off.then_some(Transition::On);
                self.shared = true;
                self.off = false;
                self.empty_in_a_row = 0;
                Outcome {
                    next: RESYNC,
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
                Outcome {
                    next: unshared,
                    transition,
                    log_empty: pull == Pull::Empty && (n == 1 || n.is_multiple_of(EMPTY_LOG_EVERY)),
                }
            }
            Pull::Failed => Outcome {
                next: if watching { UNSHARED_RETRY } else { FAILED_RETRY },
                transition: None,
                log_empty: false,
            },
        }
    }

    /// The texts session dropped: what the phone shares is per connection, so "shared" waits for
    /// the next pull (made right after the reconnect). A judged "off" is kept until then.
    pub fn connection_dropped(&mut self) {
        self.shared = false;
        self.empty_in_a_row = 0;
    }

    /// A different phone, or none (Forget): nothing carries over.
    pub fn forget(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_answers_turn_off_after_two_and_keep_checking_every_30s_indefinitely() {
        let mut w = ContactsWatch::default();
        let first = w.record(Pull::Empty, false);
        assert_eq!(first.next, UNSHARED_RETRY);
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
        assert!(w.off() && !w.shared());
        // Never slows down to minutes: the user must not wait long after turning it on.
        for _ in 0..500 {
            let o = w.record(Pull::Empty, false);
            assert_eq!(o.next, Duration::from_secs(30));
            assert_eq!(o.transition, None, "the off transition is logged once");
        }
    }

    #[test]
    fn watching_checks_every_10s() {
        let mut w = ContactsWatch::default();
        for _ in 0..50 {
            assert_eq!(w.record(Pull::Empty, true).next, Duration::from_secs(10));
        }
    }

    #[test]
    fn sharing_resyncs_every_5_minutes_and_turns_on_from_off_in_one_pull() {
        let mut w = ContactsWatch::default();
        w.record(Pull::Empty, false);
        w.record(Pull::Empty, false);
        assert!(w.off());
        let on = w.record(Pull::Shared, false);
        assert_eq!(on.transition, Some(Transition::On));
        assert_eq!(on.next, Duration::from_secs(5 * 60));
        assert!(w.shared() && !w.off());
        let again = w.record(Pull::Shared, false);
        assert_eq!(again.transition, None, "on → on logs nothing");
        assert_eq!(again.next, RESYNC);
    }

    #[test]
    fn first_ever_share_is_not_an_off_to_on_transition() {
        let mut w = ContactsWatch::default();
        assert_eq!(w.record(Pull::Shared, false).transition, None);
        assert!(w.shared());
    }

    #[test]
    fn on_then_empty_twice_is_off() {
        let mut w = ContactsWatch::default();
        w.record(Pull::Shared, false);
        assert_eq!(w.record(Pull::Empty, false).transition, None);
        assert!(!w.shared(), "an empty answer is never 'on'");
        assert_eq!(w.record(Pull::Empty, false).transition, Some(Transition::Off));
    }

    #[test]
    fn a_refusal_is_off_at_once() {
        let mut w = ContactsWatch::default();
        let o = w.record(Pull::Refused, false);
        assert_eq!(o.transition, Some(Transition::Off));
        assert_eq!(o.next, UNSHARED_RETRY);
        assert!(w.off());
    }

    #[test]
    fn a_failed_pull_changes_nothing_and_retries_later() {
        let mut w = ContactsWatch::default();
        w.record(Pull::Shared, false);
        let o = w.record(Pull::Failed, false);
        assert_eq!(
            o,
            Outcome {
                next: FAILED_RETRY,
                transition: None,
                log_empty: false
            }
        );
        assert!(w.shared());
        assert_eq!(w.record(Pull::Failed, true).next, UNSHARED_RETRY);
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
        // One more empty after the reconnect is still off, with no second transition.
        assert_eq!(w.record(Pull::Empty, false).transition, None);
        w.record(Pull::Shared, false);
        w.connection_dropped();
        assert!(!w.shared(), "shared is per connection");
        w.forget();
        assert_eq!(w, ContactsWatch::default());
    }
}
