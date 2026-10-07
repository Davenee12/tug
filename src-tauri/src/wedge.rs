//! Spotting a Bluetooth link that still reports "connected" but has stopped answering.
//!
//! Seen on hardware (2026-10-06): the Intel adapter logged "A command sent to the adapter has timed
//! out" three minutes running. The link stayed connected and the iPhone's own notifications kept
//! arriving, but every request tug sent timed out, and each queued notification burned its three
//! attempts and was given up on. A run of timeouts with no answer in between is a wedged link: stop
//! sending, give the adapter a few seconds to settle, then reconnect, backing off if it keeps
//! happening. An adapter that wedges again and again isn't helped by rebuilding the link every few
//! minutes (the sidebar would flip between Connected and Reconnecting… forever), so after
//! MAX_RELINKS in a row tug stops and leaves the link to the ordinary reconnect path until the
//! adapter has been quiet for a while. Pure, so the rules are unit-tested; the actor feeds it.

use std::time::{Duration, Instant};

/// Timeouts in a row (no answer between them) that mark the link wedged.
pub const STRIKES: usize = 3;
/// A timeout older than this no longer counts toward a run.
pub const STRIKE_WINDOW: Duration = Duration::from_secs(60);
/// How long to leave the adapter alone before reconnecting, the first time.
pub const SETTLE: Duration = Duration::from_secs(5);
/// The longest settle once wedges keep recurring.
pub const MAX_SETTLE: Duration = Duration::from_secs(60);
/// A wedge within this long of the last one is a recurrence and settles longer; after a quiet spell
/// it's a fresh one and gets the short settle again.
pub const RECUR_WITHIN: Duration = Duration::from_secs(10 * 60);
/// Relinks in a row (each wedge within RECUR_WITHIN of the last) before tug stops rebuilding the link.
pub const MAX_RELINKS: u32 = 6;

/// What a completed run of timeouts calls for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wedged {
    /// Pause requests, let the adapter settle this long, then rebuild the link.
    Relink { settle: Duration },
    /// It keeps happening: leave the link alone. `first` is true the first time, for one log line.
    Persistent { first: bool },
}

#[derive(Debug, Default)]
pub struct WedgeWatch {
    /// Timeouts in the current unanswered run, oldest first.
    strikes: Vec<Instant>,
    /// Wedges in a row, each within RECUR_WITHIN of the one before.
    recurrences: u32,
    last_wedge: Option<Instant>,
    /// Gave up relinking for the current streak (and said so).
    persistent: bool,
}

impl WedgeWatch {
    /// A Bluetooth request got an answer (success, or a refusal from the phone): the adapter works.
    pub fn answered(&mut self) {
        self.strikes.clear();
    }

    /// A Bluetooth request timed out while the link still reported connected. When this completes a
    /// run, says what to do about it.
    pub fn timed_out(&mut self, now: Instant) -> Option<Wedged> {
        self.strikes
            .retain(|t| now.saturating_duration_since(*t) < STRIKE_WINDOW);
        self.strikes.push(now);
        if self.strikes.len() < STRIKES {
            return None;
        }
        self.strikes.clear();
        let recurring = self
            .last_wedge
            .is_some_and(|t| now.saturating_duration_since(t) < RECUR_WITHIN);
        self.recurrences = if recurring {
            self.recurrences.saturating_add(1)
        } else {
            1
        };
        self.last_wedge = Some(now);
        if self.recurrences > MAX_RELINKS {
            let first = !self.persistent;
            self.persistent = true;
            return Some(Wedged::Persistent { first });
        }
        self.persistent = false;
        Some(Wedged::Relink {
            settle: settle_for(self.recurrences),
        })
    }

    /// A new link: timeouts on the old one don't count against it. (The recurrence count stays, so
    /// an adapter that wedges again right after reconnecting still backs off.)
    pub fn new_link(&mut self) {
        self.strikes.clear();
    }
}

/// The settle before reconnecting for the n-th wedge in a row: 5 s, doubling, capped at a minute.
pub fn settle_for(recurrences: u32) -> Duration {
    let doublings = recurrences.saturating_sub(1).min(5);
    SETTLE.saturating_mul(1 << doublings).min(MAX_SETTLE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn relink(settle: Duration) -> Option<Wedged> {
        Some(Wedged::Relink { settle })
    }

    #[test]
    fn three_unanswered_timeouts_wedge_the_link() {
        // The 2026-10-06 log: one ANCS request timing out at :29, :39 and :49.
        let t0 = Instant::now();
        let mut w = WedgeWatch::default();
        assert_eq!(w.timed_out(t0), None);
        assert_eq!(w.timed_out(t0 + secs(10)), None);
        assert_eq!(w.timed_out(t0 + secs(20)), relink(SETTLE));
        assert_eq!(w.timed_out(t0 + secs(30)), None, "a new run starts after a wedge");
    }

    #[test]
    fn an_answer_in_between_means_the_adapter_works() {
        let t0 = Instant::now();
        let mut w = WedgeWatch::default();
        w.timed_out(t0);
        w.timed_out(t0 + secs(10));
        w.answered();
        assert_eq!(w.timed_out(t0 + secs(20)), None, "one slow request, not a wedge");
        assert_eq!(w.timed_out(t0 + secs(30)), None);
        assert_eq!(w.timed_out(t0 + secs(40)), relink(SETTLE));
    }

    #[test]
    fn timeouts_spread_over_minutes_are_not_a_wedge() {
        let t0 = Instant::now();
        let mut w = WedgeWatch::default();
        assert_eq!(w.timed_out(t0), None);
        assert_eq!(w.timed_out(t0 + secs(50)), None);
        // The first has aged out of the window by now: only two in it.
        assert_eq!(w.timed_out(t0 + secs(70)), None);
        assert_eq!(w.timed_out(t0 + secs(80)), relink(SETTLE));
    }

    #[test]
    fn a_new_link_starts_with_a_clean_count() {
        let t0 = Instant::now();
        let mut w = WedgeWatch::default();
        w.timed_out(t0);
        w.timed_out(t0 + secs(10));
        w.new_link();
        assert_eq!(w.timed_out(t0 + secs(20)), None);
    }

    #[test]
    fn recurring_wedges_settle_longer_then_reset_after_a_quiet_spell() {
        let mut w = WedgeWatch::default();
        let wedge = |w: &mut WedgeWatch, at: Instant| {
            w.timed_out(at);
            w.timed_out(at + secs(1));
            w.timed_out(at + secs(2))
        };
        let t0 = Instant::now();
        assert_eq!(wedge(&mut w, t0), relink(secs(5)));
        assert_eq!(wedge(&mut w, t0 + secs(60)), relink(secs(10)), "again a minute later");
        assert_eq!(wedge(&mut w, t0 + secs(120)), relink(secs(20)));
        assert_eq!(wedge(&mut w, t0 + secs(180)), relink(secs(40)));
        assert_eq!(wedge(&mut w, t0 + secs(240)), relink(secs(60)));
        assert_eq!(wedge(&mut w, t0 + secs(300)), relink(MAX_SETTLE), "capped");
        assert_eq!(
            wedge(&mut w, t0 + secs(300) + RECUR_WITHIN + secs(10)),
            relink(SETTLE),
            "a fresh wedge after a quiet spell"
        );
    }

    #[test]
    fn an_adapter_that_keeps_wedging_is_left_alone_until_it_goes_quiet() {
        let mut w = WedgeWatch::default();
        let wedge = |w: &mut WedgeWatch, at: Instant| {
            w.timed_out(at);
            w.timed_out(at + secs(1));
            w.timed_out(at + secs(2))
        };
        let t0 = Instant::now();
        for n in 0..MAX_RELINKS {
            let at = t0 + secs(90 * u64::from(n));
            assert!(matches!(wedge(&mut w, at), Some(Wedged::Relink { .. })), "relink {n}");
        }
        let next = t0 + secs(90 * u64::from(MAX_RELINKS));
        assert_eq!(
            wedge(&mut w, next),
            Some(Wedged::Persistent { first: true }),
            "said once"
        );
        assert_eq!(
            wedge(&mut w, next + secs(90)),
            Some(Wedged::Persistent { first: false }),
            "then quiet"
        );
        assert_eq!(
            wedge(&mut w, next + secs(90) + RECUR_WITHIN + secs(10)),
            relink(SETTLE),
            "after a quiet spell it's worth rebuilding again"
        );
    }

    #[test]
    fn settle_backoff_has_no_overflow() {
        assert_eq!(settle_for(0), SETTLE);
        assert_eq!(settle_for(1), SETTLE);
        assert_eq!(settle_for(u32::MAX), MAX_SETTLE);
    }
}
