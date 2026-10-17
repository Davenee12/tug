//! Telling "the iPhone forgot this PC" apart from an iPhone that's away, locked, or on a hiccuping
//! adapter.
//!
//! Seen on hardware: after Forget This Device on the iPhone, Windows still held the bond. The phone
//! was right there, so Windows kept bringing the link up, the phone (with no key for this PC) kept
//! dropping it, and the link went up and down about every 2 s. No connect ever reached a working
//! notifications session, Windows never reported the stale-bond error the old check keyed on, and
//! tug said "iPhone away" forever with no way forward.
//!
//! The rule, on a phone that has worked with this PC before (`bonded`):
//!
//! - **Flapping:** `FLAPS` link-ups within `FLAP_WINDOW` that each dropped within `SHORT_LINK`, with
//!   no working session in between. A phone out of range brings no link up at all; one at the edge
//!   of range comes and goes far more slowly, and gets a working session now and then (which starts
//!   the count again); a single blip or an adapter hiccup is a handful of edges, not ten in a minute.
//! - **Stale bond:** `REFUSALS` connect attempts within `REFUSAL_WINDOW` that failed with Windows'
//!   stale-bond error (0x80070016). It's definitive on its own, so it counts even for a phone that
//!   hasn't connected yet (what the previous check did).
//! - **Refused while flapping:** `REFUSALS` attempts the phone refused (ATT insufficient
//!   authentication, authorization or encryption, access denied, or the stale-bond error) within
//!   `FLAP_WINDOW`, together with at least `FLAP_EVIDENCE` short link-ups in that same window.
//!
//! ATT refusals on their own never trip it: a healthy phone returns them too, for example with
//! Share System Notifications off or after the person tapped Don't Allow. Pair again removes the
//! pairing, so a false verdict costs far more than a late one.
//!
//! Never while the phone is locked: a phone that just restarted keeps its link up but withholds
//! notifications until it's unlocked, and that has its own "unlock your iPhone" state. A wake, the
//! radio changing, an adapter stall or tug relinking on its own forgets the evidence gathered so far
//! (the link was messy for another reason). The link coming and going while notifications are
//! still subscribed is a blip on a working session, never evidence. The verdict itself only clears
//! when a session works again (a connect, a notification or media update arriving, or an edge on a
//! subscribed link), or the phone is forgotten or replaced. Pure, so the rules are unit-tested; the
//! BLE actor feeds it.

use std::time::{Duration, Instant};

/// A link-up that dropped again within this long never got anywhere.
pub const SHORT_LINK: Duration = Duration::from_secs(5);
/// Short link-ups within `FLAP_WINDOW` that mean the phone keeps refusing this PC.
pub const FLAPS: usize = 10;
pub const FLAP_WINDOW: Duration = Duration::from_secs(60);
/// Stale-bond errors within `REFUSAL_WINDOW` that mean the phone has no pairing for this PC; also
/// how many refusals of any kind, with `FLAP_EVIDENCE` short link-ups in `FLAP_WINDOW`, do.
pub const REFUSALS: usize = 2;
pub const REFUSAL_WINDOW: Duration = Duration::from_secs(120);
/// Short link-ups within `FLAP_WINDOW` that turn refusals in that window into a verdict: half the
/// flapping rule, so refusals alone (a healthy phone saying no) never trip it.
pub const FLAP_EVIDENCE: usize = FLAPS / 2;

/// What a failed connect attempt says about the bond.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attempt {
    /// Windows reported the stale-bond error: the phone no longer has this PC's keys.
    StaleBond,
    /// The phone refused (insufficient authentication, authorization or encryption, or access
    /// denied). A healthy phone can say this too (Share System Notifications off, Don't Allow), so
    /// it only counts alongside flapping.
    Refused,
    /// The phone answered but offers no notifications: locked after a restart, not forgotten.
    Locked,
    /// Out of range, timed out, or anything else: says nothing about the bond.
    Other,
}

/// A change of verdict, for the status and one log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The phone looks to have forgotten this PC: how many short link-ups and refusals said so.
    Entered { flaps: usize, refusals: usize },
    /// It isn't (any more): a session worked, or the phone was forgotten or replaced.
    Left,
}

#[derive(Debug, Default)]
pub struct ForgottenWatch {
    /// The phone has worked with this PC: a session this run, or a remembered phone that connected
    /// when it was set up. A freshly paired phone that never connected has its own "pair again" rule.
    bonded: bool,
    /// When the link last came up, while it's still up.
    up_since: Option<Instant>,
    /// When each short link-up ended, oldest first.
    flaps: Vec<Instant>,
    /// When each refused attempt failed, oldest first, and whether it was the stale-bond error.
    refusals: Vec<(Instant, bool)>,
    forgotten: bool,
}

impl ForgottenWatch {
    /// Watching a phone: `bonded` when it has worked with this PC before (a remembered phone), not
    /// for one just paired. Forgets everything about the previous phone, verdict included.
    pub fn new(bonded: bool) -> Self {
        Self {
            bonded,
            ..Self::default()
        }
    }

    /// Whether the phone looks to have forgotten this PC.
    pub fn forgotten(&self) -> bool {
        self.forgotten
    }

    /// A different phone, or none (Forget, Pair again, a new pairing): start from nothing.
    pub fn replace(&mut self, bonded: bool) -> Option<Change> {
        let was = self.forgotten;
        *self = Self::new(bonded);
        was.then_some(Change::Left)
    }

    /// A connect worked (notifications subscribed): the bond is fine.
    pub fn healthy(&mut self) -> Option<Change> {
        self.session_alive()
    }

    /// The session is working right now: notifications are subscribed on the link, or a
    /// notification or media update just arrived. A phone that forgot this PC can do neither, so
    /// this is proof the bond is fine: forget the evidence and clear any verdict.
    pub fn session_alive(&mut self) -> Option<Change> {
        let was = self.forgotten;
        *self = Self::new(true);
        was.then_some(Change::Left)
    }

    /// A connection edge from Windows (stamped when it said so). `subscribed` is whether
    /// notifications are still subscribed on this link: then the edge is a blip on a working
    /// session, not the phone refusing this PC, and it counts as `session_alive` instead. Only a
    /// link with no working session feeds the flap count.
    pub fn edge(&mut self, connected: bool, at: Instant, subscribed: bool, awaiting_unlock: bool) -> Option<Change> {
        if subscribed {
            return self.session_alive();
        }
        if connected {
            self.link_up(at);
            None
        } else {
            self.link_down(at, awaiting_unlock)
        }
    }

    /// Something other than the bond explains a messy link (the PC woke, the radio changed, the
    /// adapter stalled, tug is rebuilding the link on its own): forget the evidence, keep the verdict.
    pub fn interrupted(&mut self) {
        self.up_since = None;
        self.flaps.clear();
        self.refusals.clear();
    }

    /// Windows reported the link up (stamped when it said so).
    pub fn link_up(&mut self, at: Instant) {
        // A repeated up event mustn't restart the clock on a link that's already up.
        self.up_since.get_or_insert(at);
    }

    /// Windows reported the link down (stamped when it said so). `awaiting_unlock` is the status's
    /// "unlock your iPhone".
    pub fn link_down(&mut self, at: Instant, awaiting_unlock: bool) -> Option<Change> {
        if let Some(up) = self.up_since.take() {
            if at.saturating_duration_since(up) < SHORT_LINK {
                self.flaps.push(at);
            }
        }
        self.judge(at, awaiting_unlock)
    }

    /// tug dropped its objects for the link (a relink or a fresh open): whatever was up isn't timed
    /// any more.
    pub fn link_closed(&mut self) {
        self.up_since = None;
    }

    /// A connect attempt failed. `awaiting_unlock` is the status after the failure was handled.
    pub fn attempt_failed(&mut self, at: Instant, attempt: Attempt, awaiting_unlock: bool) -> Option<Change> {
        match attempt {
            Attempt::StaleBond => self.refusals.push((at, true)),
            Attempt::Refused => self.refusals.push((at, false)),
            // The phone is there and has its bond, it just won't share until it's unlocked: what
            // looked like flapping was the lock, not a forgotten pairing.
            Attempt::Locked => {
                self.flaps.clear();
                self.refusals.clear();
                return None;
            }
            Attempt::Other => {}
        }
        self.judge(at, awaiting_unlock)
    }

    fn judge(&mut self, now: Instant, awaiting_unlock: bool) -> Option<Change> {
        self.flaps.retain(|t| now.saturating_duration_since(*t) < FLAP_WINDOW);
        self.refusals
            .retain(|(t, _)| now.saturating_duration_since(*t) < REFUSAL_WINDOW);
        if self.forgotten || awaiting_unlock {
            return None;
        }
        let stale_bonds = self.refusals.iter().filter(|(_, stale)| *stale).count();
        let flapping = self.bonded && self.flaps.len() >= FLAPS;
        let stale_bond = stale_bonds >= REFUSALS;
        // Flaps are already trimmed to FLAP_WINDOW; refusals keep a longer window for the
        // stale-bond rule, so count only the ones in the flaps' window here.
        let recent_refusals = self
            .refusals
            .iter()
            .filter(|(t, _)| now.saturating_duration_since(*t) < FLAP_WINDOW)
            .count();
        let refused_while_flapping = self.bonded && recent_refusals >= REFUSALS && self.flaps.len() >= FLAP_EVIDENCE;
        if !(flapping || stale_bond || refused_while_flapping) {
            return None;
        }
        self.forgotten = true;
        Some(Change::Entered {
            flaps: self.flaps.len(),
            refusals: self.refusals.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: fn(u64) -> Duration = Duration::from_secs;
    const MS: fn(u64) -> Duration = Duration::from_millis;

    /// `n` link-ups, `period` apart, each up for `up` before dropping. Returns the first change and
    /// when the last edge happened.
    fn flap(
        w: &mut ForgottenWatch,
        start: Instant,
        n: usize,
        period: Duration,
        up: Duration,
    ) -> (Option<Change>, Instant) {
        let mut change = None;
        let mut t = start;
        for _ in 0..n {
            w.link_up(t);
            let down = t + up;
            if let Some(c) = w.link_down(down, false) {
                change.get_or_insert(c);
            }
            t += period;
        }
        (change, t)
    }

    #[test]
    fn a_forgotten_phone_flapping_every_two_seconds_is_caught_within_half_a_minute() {
        // The hardware case: up for ~0.6 s, down, up again ~2 s later, no session ever.
        let mut w = ForgottenWatch::new(true);
        let t0 = Instant::now();
        let (change, _) = flap(&mut w, t0, FLAPS, S(2), MS(600));
        assert_eq!(
            change,
            Some(Change::Entered {
                flaps: FLAPS,
                refusals: 0
            })
        );
        assert!(w.forgotten());
        // One less isn't enough.
        let mut w = ForgottenWatch::new(true);
        assert_eq!(flap(&mut w, t0, FLAPS - 1, S(2), MS(600)).0, None);
    }

    #[test]
    fn it_says_so_once_and_clears_the_moment_a_session_works() {
        let mut w = ForgottenWatch::new(true);
        let t0 = Instant::now();
        let (_, t) = flap(&mut w, t0, FLAPS, S(2), MS(600));
        // More flapping doesn't announce it again.
        assert_eq!(flap(&mut w, t, FLAPS, S(2), MS(600)).0, None);
        assert!(w.forgotten());
        assert_eq!(w.healthy(), Some(Change::Left));
        assert!(!w.forgotten());
        assert_eq!(w.healthy(), None, "only a real change is reported");
    }

    #[test]
    fn the_verdict_isnt_stuck_after_pair_again_or_a_new_phone() {
        let mut w = ForgottenWatch::new(true);
        flap(&mut w, Instant::now(), FLAPS, S(2), MS(600));
        assert_eq!(w.replace(false), Some(Change::Left));
        assert!(!w.forgotten());
        assert_eq!(w.replace(false), None);
    }

    #[test]
    fn an_ordinary_out_of_range_phone_is_never_called_forgotten() {
        // Out of range: the link stays down; connects fail as unreachable or timed out.
        let mut w = ForgottenWatch::new(true);
        let t0 = Instant::now();
        for i in 0..100 {
            assert_eq!(w.attempt_failed(t0 + S(i * 10), Attempt::Other, false), None);
        }
        assert!(!w.forgotten());
    }

    #[test]
    fn a_phone_at_the_edge_of_range_isnt_called_forgotten() {
        let t0 = Instant::now();
        // Links that stay up a while before range takes them: not short, however many.
        let mut w = ForgottenWatch::new(true);
        assert_eq!(flap(&mut w, t0, 50, S(10), S(6)).0, None);
        // Short drops, but spread out (one every 10 s): never ten in a minute.
        let mut w = ForgottenWatch::new(true);
        assert_eq!(flap(&mut w, t0, 50, S(10), MS(800)).0, None);
        // A burst that a working session interrupts starts over.
        let mut w = ForgottenWatch::new(true);
        let (_, t) = flap(&mut w, t0, FLAPS - 1, S(2), MS(600));
        assert_eq!(w.healthy(), None);
        assert_eq!(flap(&mut w, t, FLAPS - 1, S(2), MS(600)).0, None);
        assert!(!w.forgotten());
    }

    #[test]
    fn a_locked_phone_after_a_restart_isnt_called_forgotten() {
        let t0 = Instant::now();
        // While the status says "unlock your iPhone", nothing trips.
        let mut w = ForgottenWatch::new(true);
        let mut t = t0;
        for _ in 0..(FLAPS * 2) {
            w.link_up(t);
            assert_eq!(w.link_down(t + MS(500), true), None);
            t += S(2);
        }
        assert_eq!(w.attempt_failed(t, Attempt::Refused, true), None);
        assert_eq!(w.attempt_failed(t + S(1), Attempt::Refused, true), None);
        assert!(!w.forgotten());
        // A phone found locked wipes the evidence: what looked like flapping was the lock.
        let mut w = ForgottenWatch::new(true);
        let (_, t) = flap(&mut w, t0, FLAPS - 1, S(2), MS(600));
        assert_eq!(w.attempt_failed(t, Attempt::Locked, false), None);
        assert_eq!(flap(&mut w, t + S(1), 1, S(2), MS(600)).0, None);
        assert!(!w.forgotten());
    }

    #[test]
    fn adapter_hiccups_and_wakes_forget_the_evidence() {
        let t0 = Instant::now();
        let mut w = ForgottenWatch::new(true);
        let (_, t) = flap(&mut w, t0, FLAPS - 1, S(2), MS(600));
        // The PC woke, the radio changed, the adapter stalled, or tug relinked on its own.
        w.interrupted();
        assert_eq!(flap(&mut w, t, FLAPS - 1, S(2), MS(600)).0, None);
        assert!(!w.forgotten());
        // But a verdict already reached stays until a session works.
        let (change, t) = flap(&mut w, t + S(30), FLAPS, S(2), MS(600));
        assert!(change.is_some());
        w.interrupted();
        assert!(w.forgotten());
        assert_eq!(flap(&mut w, t, 1, S(2), MS(600)).0, None);
    }

    #[test]
    fn blips_on_a_live_subscription_never_say_forgotten() {
        // Seen in review: a working session whose link blips (down and up inside the grace, the
        // subscription surviving) fed every edge to the flap count and was called forgotten.
        let mut w = ForgottenWatch::new(true);
        let t0 = Instant::now();
        let mut t = t0;
        for _ in 0..(FLAPS * 3) {
            assert_eq!(w.edge(false, t, true, false), None);
            assert_eq!(w.edge(true, t + MS(300), true, false), None);
            t += S(2);
        }
        assert!(!w.forgotten());
        // The same edges with no working session are the forgotten phone's flapping.
        let mut w = ForgottenWatch::new(true);
        let mut t = t0;
        let mut change = None;
        for _ in 0..FLAPS {
            w.edge(true, t, false, false);
            change = change.or(w.edge(false, t + MS(600), false, false));
            t += S(2);
        }
        assert!(change.is_some());
    }

    #[test]
    fn a_stale_verdict_clears_on_the_next_notification() {
        let mut w = ForgottenWatch::new(true);
        let (change, t) = flap(&mut w, Instant::now(), FLAPS, S(2), MS(600));
        assert!(change.is_some());
        // A notification or media update arrived: the session works.
        assert_eq!(w.session_alive(), Some(Change::Left));
        assert!(!w.forgotten());
        assert_eq!(w.session_alive(), None, "only a real change is reported");
        // An edge on a link that's subscribed clears a verdict too.
        flap(&mut w, t, FLAPS, S(2), MS(600));
        assert!(w.forgotten());
        assert_eq!(w.edge(true, t + S(60), true, false), Some(Change::Left));
        assert!(!w.forgotten());
    }

    #[test]
    fn a_single_blip_or_a_few_isnt_forgotten() {
        let mut w = ForgottenWatch::new(true);
        let t0 = Instant::now();
        assert_eq!(flap(&mut w, t0, 1, S(2), MS(300)).0, None);
        assert_eq!(flap(&mut w, t0 + S(5), 3, S(2), MS(300)).0, None);
        assert!(!w.forgotten());
    }

    #[test]
    fn old_flaps_age_out_of_the_window() {
        let mut w = ForgottenWatch::new(true);
        let t0 = Instant::now();
        // Ten short ups, 7 s apart: the first has left the minute by the tenth.
        assert_eq!(flap(&mut w, t0, FLAPS, S(7), MS(600)).0, None);
    }

    #[test]
    fn repeated_up_events_dont_restart_the_clock() {
        let mut w = ForgottenWatch::new(true);
        let t0 = Instant::now();
        w.link_up(t0);
        w.link_up(t0 + S(4));
        // Up for 6 s in all: not a short link-up, even though the last up event was 2 s ago.
        w.link_down(t0 + S(6), false);
        assert!(w.flaps.is_empty());
        // A down with no up before it (a repeated down, or a link tug just reopened) isn't one either.
        w.link_down(t0 + S(7), false);
        w.link_up(t0 + S(8));
        w.link_closed();
        w.link_down(t0 + S(8) + MS(100), false);
        assert!(w.flaps.is_empty());
    }

    #[test]
    fn att_refusals_alone_never_mean_forgotten() {
        // A healthy phone refuses too: Share System Notifications off, or Don't Allow tapped. Pair
        // again would remove a working pairing, so refusals on a steady link never trip it.
        let t0 = Instant::now();
        let mut w = ForgottenWatch::new(true);
        for i in 0..20 {
            assert_eq!(w.attempt_failed(t0 + S(i * 5), Attempt::Refused, false), None);
        }
        assert!(!w.forgotten());
        // Nor does one stale-bond error mixed in with them.
        let mut w = ForgottenWatch::new(true);
        w.attempt_failed(t0, Attempt::Refused, false);
        assert_eq!(w.attempt_failed(t0 + S(5), Attempt::StaleBond, false), None);
        // Nor a few short link-ups (a blip or two) beside them.
        let mut w = ForgottenWatch::new(true);
        let (_, t) = flap(&mut w, t0, FLAP_EVIDENCE - 1, S(2), MS(600));
        w.attempt_failed(t, Attempt::Refused, false);
        assert_eq!(w.attempt_failed(t + S(1), Attempt::Refused, false), None);
        assert!(!w.forgotten());
    }

    #[test]
    fn refusals_with_flapping_in_the_same_window_mean_forgotten() {
        let t0 = Instant::now();
        let mut w = ForgottenWatch::new(true);
        let (_, t) = flap(&mut w, t0, FLAP_EVIDENCE, S(2), MS(600));
        assert_eq!(
            w.attempt_failed(t, Attempt::Refused, false),
            None,
            "one can be a glitch"
        );
        assert_eq!(
            w.attempt_failed(t + S(1), Attempt::Refused, false),
            Some(Change::Entered {
                flaps: FLAP_EVIDENCE,
                refusals: 2
            })
        );
        // Refusals first, then the flapping: the verdict comes on the flap that completes it.
        let mut w = ForgottenWatch::new(true);
        w.attempt_failed(t0, Attempt::Refused, false);
        w.attempt_failed(t0 + S(1), Attempt::Refused, false);
        assert!(flap(&mut w, t0 + S(2), FLAP_EVIDENCE, S(2), MS(600)).0.is_some());
        // Not the same window: the refusals are older than a minute when the flapping comes.
        let mut w = ForgottenWatch::new(true);
        w.attempt_failed(t0, Attempt::Refused, false);
        w.attempt_failed(t0 + S(1), Attempt::Refused, false);
        assert_eq!(
            flap(&mut w, t0 + FLAP_WINDOW + S(2), FLAP_EVIDENCE, S(2), MS(600)).0,
            None
        );
        // A phone that never worked here: flapping has other explanations there.
        let mut w = ForgottenWatch::new(false);
        let (_, t) = flap(&mut w, t0, FLAP_EVIDENCE, S(2), MS(600));
        w.attempt_failed(t, Attempt::Refused, false);
        assert_eq!(w.attempt_failed(t + S(1), Attempt::Refused, false), None);
    }

    #[test]
    fn two_stale_bond_errors_mean_forgotten() {
        let t0 = Instant::now();
        let mut w = ForgottenWatch::new(true);
        assert_eq!(
            w.attempt_failed(t0, Attempt::StaleBond, false),
            None,
            "one can be a glitch"
        );
        assert_eq!(
            w.attempt_failed(t0 + S(30), Attempt::StaleBond, false),
            Some(Change::Entered { flaps: 0, refusals: 2 })
        );
        // Too far apart: not a run.
        let mut w = ForgottenWatch::new(true);
        w.attempt_failed(t0, Attempt::StaleBond, false);
        assert_eq!(w.attempt_failed(t0 + REFUSAL_WINDOW, Attempt::StaleBond, false), None);
    }

    #[test]
    fn a_phone_that_never_worked_here_needs_the_definitive_error() {
        let t0 = Instant::now();
        // Just paired, never connected: flapping and ATT refusals have other explanations there
        // (and their own "pair again" rule), but the stale-bond error is still definitive.
        let mut w = ForgottenWatch::new(false);
        assert_eq!(flap(&mut w, t0, FLAPS * 2, S(2), MS(600)).0, None);
        w.attempt_failed(t0, Attempt::Refused, false);
        assert_eq!(w.attempt_failed(t0 + S(1), Attempt::Refused, false), None);
        let mut w = ForgottenWatch::new(false);
        w.attempt_failed(t0, Attempt::StaleBond, false);
        assert!(w.attempt_failed(t0 + S(5), Attempt::StaleBond, false).is_some());
        // Once a session has worked, the phone is bonded and the ordinary rules apply.
        let mut w = ForgottenWatch::new(false);
        w.healthy();
        assert!(flap(&mut w, t0, FLAPS, S(2), MS(600)).0.is_some());
    }
}
