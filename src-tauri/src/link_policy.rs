//! Decisions about the iPhone link that don't need Bluetooth: what a failed connect attempt means
//! for the status and the backoff, whether a connect should wait for Windows' link-up instead of
//! blocking on discovery, and when a just-adopted phone needs pairing again. Pure, so it's
//! unit-tested; `ble/actor/link.rs` does the I/O and asks these.

use std::time::Duration;

/// What a failed connect attempt failed with, reduced to what the policy cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// Windows answered promptly that the phone isn't there.
    Unreachable,
    /// Windows didn't finish in time.
    TimedOut,
    /// The phone is connected but isn't offering ANCS (locked after a restart, or mid-update).
    NotFound,
    /// No ANCS and not an Apple device (an Android phone): unlocking won't help, and retrying
    /// often won't either.
    NotAnIphone,
    /// Anything else (refused, closed, a Windows error).
    Other,
}

/// What tug knew before the failed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Before {
    /// Whether Windows reported the link connected when the attempt failed.
    pub linked: bool,
    /// The status already said "unlock your iPhone".
    pub awaiting_unlock: bool,
    /// The phone was already away (down past the blip grace, or unreachable).
    pub away: bool,
    /// Failed attempts in a row, including this one.
    pub failures: u32,
    /// The status said "Reconnecting…" (tug relinked on its own).
    pub reconnecting: bool,
    /// The phone has had a working connection this run (since tug started or it was adopted).
    pub had_session: bool,
    /// A freshly adopted phone still inside its grace period (see `in_adopt_grace`): its first
    /// failed connects mustn't call it away.
    pub adopt_grace: bool,
}

/// How long, and how many failed connects, a freshly paired phone gets before tug may call it
/// away. Seen live: "the iPhone is away" 21–26 s after a successful pair, while it was still
/// settling the new bond.
pub const ADOPT_GRACE: Duration = Duration::from_secs(90);
pub const ADOPT_GRACE_FAILURES: u32 = 2;

/// Whether a phone adopted `since_adopt` ago, with `failures` failed connects and no connection
/// yet, is still in its grace period: until it has both failed more than twice and had 90 s.
pub fn in_adopt_grace(connected_since_adopt: bool, failures: u32, since_adopt: Option<Duration>) -> bool {
    !connected_since_adopt && (failures <= ADOPT_GRACE_FAILURES || since_adopt.is_some_and(|d| d < ADOPT_GRACE))
}

/// After this long away, tug pokes the phone only every `AWAY_LONG_RETRY` (a link-up from Windows
/// still reconnects at once): a phone left at home all day needn't be tried every 30 s.
pub const AWAY_LONG_AFTER: Duration = Duration::from_secs(10 * 60);
pub const AWAY_LONG_RETRY_SECS: u32 = 180;

/// The wait before the next connect attempt, eased for a phone that's been away a long time.
pub fn away_retry_secs(normal: u32, away_for: Option<Duration>) -> u32 {
    if away_for.is_some_and(|d| d >= AWAY_LONG_AFTER) {
        normal.max(AWAY_LONG_RETRY_SECS)
    } else {
        normal
    }
}

/// What a failed attempt should leave behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct After {
    pub awaiting_unlock: bool,
    /// The lock follows a restart of a phone that was working ("Your iPhone restarted…").
    pub phone_restarted: bool,
    pub away: bool,
    /// Whether this attempt's error replaces `last_error`. While away it doesn't: the retries
    /// alternate between "out of range" and "not responding", and the status must stay steady.
    pub publish_error: bool,
    pub reconnecting: bool,
    /// Which backoff applies.
    pub backoff: Backoff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backoff {
    /// Connected but locked: back off far (up to minutes).
    Unlock,
    /// The link is up, the attempt failed anyway: retry soon.
    Connected,
    /// The phone isn't there.
    Idle,
}

/// Failed connect attempts after an automatic relink that still show as "Reconnecting…".
pub const RECONNECTING_ATTEMPTS: u32 = 3;

/// The outcome of a failed connect attempt.
///
/// - "Unlock your iPhone" is set by a connected attempt that finds no ANCS, cleared only by a
///   connected attempt failing some other way (or by success/wake, elsewhere). A failure while the
///   link is down says nothing about the lock, so it keeps it, and keeps its long backoff.
/// - "Away" is set when the phone can't be reached with the link down, and is sticky: only a
///   successful connect clears it. While away, failed retries don't change what's shown.
pub fn after_failure(before: Before, failure: Failure) -> After {
    let awaiting_unlock = if before.linked {
        failure == Failure::NotFound
    } else {
        before.awaiting_unlock
    };
    let unreachable = matches!(failure, Failure::Unreachable | Failure::TimedOut);
    let away = before.away || (!before.linked && unreachable && !before.adopt_grace);
    // ANCS gone on a live link after a working session: iOS withholds it until the first unlock
    // after a restart, so the phone restarted (rather than "never set up" or "forgot this PC").
    let phone_restarted = awaiting_unlock && before.had_session;
    let reconnecting = !away && before.reconnecting && !awaiting_unlock && before.failures < RECONNECTING_ATTEMPTS;
    // Not an iPhone: nothing will change soon, so retry as rarely as a locked phone (but never
    // say "unlock").
    let backoff = if awaiting_unlock || (before.linked && failure == Failure::NotAnIphone) {
        Backoff::Unlock
    } else if before.linked {
        Backoff::Connected
    } else {
        Backoff::Idle
    };
    After {
        awaiting_unlock,
        phone_restarted,
        away,
        publish_error: !before.away,
        reconnecting,
        backoff,
    }
}

/// Whether a connect attempt should show "Connecting…". Background retries for a phone that's
/// away, or that keeps failing with the link down, mustn't flip the status every few seconds;
/// only the first attempt, or one on a link Windows reports up, says so.
pub fn shows_connecting(failures: u32, linked: bool, away: bool) -> bool {
    linked || (failures == 0 && !away)
}

/// The wait before reconnecting after a real link-down. Keeps any longer backoff already running
/// for a locked phone (max, not overwrite), so its minutes-long unlock backoff survives the link
/// flapping underneath it.
pub fn retry_after_link_down(current: u32, ordinary: u32, awaiting_unlock: bool) -> u32 {
    if awaiting_unlock {
        current.max(ordinary)
    } else {
        ordinary
    }
}

/// The wait before connecting once Windows reports the link up. Normally at once; a flapping link
/// gets a short settle; a locked phone keeps its unlock backoff (the link coming up doesn't unlock
/// it, and cutting the wait is what made attempts 31 s apart all night).
pub fn retry_on_link_up(current: u32, flapping: bool, settle: u32, awaiting_unlock: bool) -> u32 {
    if awaiting_unlock {
        current
    } else if flapping {
        current.min(settle)
    } else {
        0
    }
}

/// How often a connect attempt still pokes a link Windows reports down, instead of waiting for
/// its link-up event (which connects at once).
pub const LINK_DOWN_POKE: Duration = Duration::from_secs(60);

/// Whether `connect` should skip the (blocking) GATT discovery and wait for Windows' link-up:
/// there's an opened link, Windows says it's down, and it was poked recently. Otherwise every
/// retry for an away phone blocked the actor for up to 30 s, and clicks waited behind it.
pub fn wait_for_link_up(have_link: bool, linked: bool, since_poke: Option<Duration>) -> bool {
    have_link && !linked && since_poke.is_some_and(|d| d < LINK_DOWN_POKE)
}

/// Discovery budget for a reconnect to a phone that has connected before; the first connect after
/// adopting a phone keeps the full budget (a fresh bond can wait on the phone's "Allow").
pub const RECONNECT_DISCOVERY: Duration = Duration::from_secs(10);

/// Whether a timed-out connect should throw away Windows' device/session objects for the phone
/// and open fresh ones (a timeout can leave them stuck: switching back to a phone needed a
/// restart). Only when the link was up (setup itself hung) or the phone hasn't connected since it
/// was adopted. A timeout while Windows reports the link down is just an away phone: keep the
/// link, whose ConnectionStatusChanged handler connects the moment it comes back, and wait for
/// that instead of polling with fresh objects.
pub fn drop_link_on_timeout(linked: bool, connected_since_adopt: bool) -> bool {
    linked || !connected_since_adopt
}

/// Timed-out connects after adopting a phone, with no success yet, before tug says to pair again.
pub const ADOPT_TIMEOUTS_BEFORE_REPAIR: u32 = 2;

/// Whether a just-adopted phone that has never connected should be shown "pair again" guidance
/// instead of looping: switching back to a phone could need a fresh pairing on the phone's side.
pub fn suggests_pair_again(connected_since_adopt: bool, timeouts_since_adopt: u32) -> bool {
    !connected_since_adopt && timeouts_since_adopt >= ADOPT_TIMEOUTS_BEFORE_REPAIR
}

/// What reading the ANCS subscription back after a wake found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeCheck {
    /// The phone answered (on or off): the link works.
    Answered,
    /// Windows closed tug's objects for it.
    Closed,
    /// No answer within the bound.
    TimedOut,
    /// Some other error: the read failed but the link may be fine.
    Failed,
}

/// Whether a wake keeps the link instead of rebuilding it. Every wake used to relink even a healthy
/// link; now only one that isn't up and subscribed, or whose check came back closed or unanswered,
/// is rebuilt.
pub fn keep_link_after_wake(linked: bool, subscribed: bool, check: WakeCheck) -> bool {
    linked && subscribed && matches!(check, WakeCheck::Answered | WakeCheck::Failed)
}

/// How long a wake waits for that check before rebuilding the link anyway.
pub const WAKE_CHECK: Duration = Duration::from_secs(10);

/// How often the radio is read again while it isn't On: a stale "off" (a missed StateChanged) used
/// to block every connect until tug restarted.
pub const RADIO_RECHECK_SECS: u32 = 45;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wake_keeps_a_healthy_link_and_rebuilds_a_dead_one() {
        assert!(keep_link_after_wake(true, true, WakeCheck::Answered));
        assert!(
            keep_link_after_wake(true, true, WakeCheck::Failed),
            "a read error alone isn't a dead link"
        );
        assert!(!keep_link_after_wake(true, true, WakeCheck::Closed));
        assert!(!keep_link_after_wake(true, true, WakeCheck::TimedOut));
        assert!(!keep_link_after_wake(false, true, WakeCheck::Answered), "down: rebuild");
        assert!(
            !keep_link_after_wake(true, false, WakeCheck::Answered),
            "not subscribed: rebuild"
        );
    }

    fn before() -> Before {
        Before {
            linked: false,
            awaiting_unlock: false,
            away: false,
            failures: 1,
            reconnecting: false,
            had_session: false,
            adopt_grace: false,
        }
    }

    #[test]
    fn ancs_gone_after_a_working_session_means_the_phone_restarted() {
        let linked = Before {
            linked: true,
            ..before()
        };
        let a = after_failure(
            Before {
                had_session: true,
                ..linked
            },
            Failure::NotFound,
        );
        assert!(a.awaiting_unlock && a.phone_restarted);
        // Locked when tug started (no session yet this run): just "unlock".
        let a = after_failure(linked, Failure::NotFound);
        assert!(a.awaiting_unlock && !a.phone_restarted);
        // Other failures on a live link aren't a restart.
        let a = after_failure(
            Before {
                had_session: true,
                ..linked
            },
            Failure::Other,
        );
        assert!(!a.phone_restarted);
    }

    #[test]
    fn a_freshly_paired_phone_isnt_called_away_during_its_grace() {
        let fresh = Before {
            adopt_grace: true,
            ..before()
        };
        assert!(!after_failure(fresh, Failure::Unreachable).away);
        assert!(after_failure(before(), Failure::Unreachable).away);
        // The grace: until it has failed more than twice AND had 90 s.
        let s = Duration::from_secs;
        assert!(in_adopt_grace(false, 1, Some(s(10))));
        assert!(in_adopt_grace(false, 5, Some(s(30))), "within 90 s");
        assert!(in_adopt_grace(false, 2, Some(s(200))), "two failures or fewer");
        assert!(!in_adopt_grace(false, 3, Some(s(91))));
        assert!(!in_adopt_grace(true, 0, Some(s(1))), "connected since: no grace needed");
    }

    #[test]
    fn a_phone_away_for_long_is_poked_every_three_minutes() {
        let m = |n: u64| Some(Duration::from_secs(n * 60));
        assert_eq!(away_retry_secs(30, None), 30);
        assert_eq!(away_retry_secs(30, m(9)), 30);
        assert_eq!(away_retry_secs(30, m(10)), 180);
        assert_eq!(away_retry_secs(30, m(120)), 180);
        assert_eq!(away_retry_secs(300, m(60)), 300, "never shortens a longer wait");
    }

    #[test]
    fn a_connected_attempt_without_ancs_asks_for_an_unlock_and_backs_off_far() {
        let a = after_failure(
            Before {
                linked: true,
                ..before()
            },
            Failure::NotFound,
        );
        assert!(a.awaiting_unlock);
        assert_eq!(a.backoff, Backoff::Unlock);
        assert!(!a.away);
    }

    #[test]
    fn a_phone_that_isnt_an_iphone_never_asks_for_an_unlock_but_backs_off_far() {
        let a = after_failure(
            Before {
                linked: true,
                awaiting_unlock: true,
                ..before()
            },
            Failure::NotAnIphone,
        );
        assert!(!a.awaiting_unlock);
        assert_eq!(a.backoff, Backoff::Unlock);
    }

    #[test]
    fn a_link_down_failure_keeps_the_unlock_state_and_its_backoff() {
        // The bug: the link dropping under a locked phone cleared "unlock" and its 2 min backoff.
        let a = after_failure(
            Before {
                awaiting_unlock: true,
                ..before()
            },
            Failure::Unreachable,
        );
        assert!(a.awaiting_unlock);
        assert_eq!(a.backoff, Backoff::Unlock);
    }

    #[test]
    fn a_connected_attempt_failing_otherwise_clears_the_unlock_state() {
        let a = after_failure(
            Before {
                linked: true,
                awaiting_unlock: true,
                ..before()
            },
            Failure::Other,
        );
        assert!(!a.awaiting_unlock);
        assert_eq!(a.backoff, Backoff::Connected);
    }

    #[test]
    fn an_unreachable_phone_is_away_and_stays_away_through_retries() {
        let first = after_failure(before(), Failure::TimedOut);
        assert!(first.away);
        assert!(first.publish_error, "entering away says why once");
        assert!(!first.reconnecting);
        assert_eq!(first.backoff, Backoff::Idle);
        // Later retries fail in other ways; the status mustn't change.
        let next = after_failure(
            Before {
                away: true,
                failures: 2,
                ..before()
            },
            Failure::Other,
        );
        assert!(next.away, "sticky until a successful connect");
        assert!(!next.publish_error, "a retry doesn't replace the shown error");
        let linked = after_failure(
            Before {
                away: true,
                linked: true,
                ..before()
            },
            Failure::Other,
        );
        assert!(linked.away, "only success clears it");
    }

    #[test]
    fn reconnecting_shows_for_a_few_failed_attempts_then_waits() {
        let r = |failures, reconnecting, awaiting_unlock, linked| {
            after_failure(
                Before {
                    linked,
                    awaiting_unlock,
                    away: false,
                    failures,
                    reconnecting,
                    had_session: false,
                    adopt_grace: false,
                },
                if awaiting_unlock {
                    Failure::NotFound
                } else {
                    Failure::Other
                },
            )
            .reconnecting
        };
        assert!(r(1, true, false, true));
        assert!(r(2, true, false, true));
        assert!(!r(RECONNECTING_ATTEMPTS, true, false, true), "now an ordinary wait");
        assert!(!r(1, true, true, true), "a locked phone asks to be unlocked");
        assert!(!r(1, false, false, true), "only after tug relinked on its own");
        assert!(
            !after_failure(
                Before {
                    reconnecting: true,
                    ..before()
                },
                Failure::Unreachable
            )
            .reconnecting,
            "an away phone isn't 'reconnecting'"
        );
    }

    #[test]
    fn background_retries_dont_flip_the_status_to_connecting() {
        assert!(shows_connecting(0, false, false), "the first attempt");
        assert!(
            !shows_connecting(1, false, false),
            "a background retry with the link down"
        );
        assert!(!shows_connecting(0, false, true), "away: one steady state");
        assert!(shows_connecting(5, true, true), "Windows says the link is up");
    }

    #[test]
    fn a_link_down_keeps_a_longer_unlock_backoff() {
        assert_eq!(retry_after_link_down(120, 4, true), 120);
        assert_eq!(retry_after_link_down(1, 4, true), 4);
        assert_eq!(retry_after_link_down(120, 4, false), 4);
    }

    #[test]
    fn a_link_up_connects_now_unless_flapping_or_locked() {
        assert_eq!(retry_on_link_up(30, false, 5, false), 0);
        assert_eq!(retry_on_link_up(30, true, 5, false), 5);
        assert_eq!(retry_on_link_up(3, true, 5, false), 3);
        assert_eq!(retry_on_link_up(120, false, 5, true), 120, "keeps the unlock backoff");
    }

    #[test]
    fn a_down_link_waits_for_windows_with_a_slow_poke() {
        let s = Duration::from_secs;
        assert!(!wait_for_link_up(false, false, Some(s(1))), "no link: open one");
        assert!(!wait_for_link_up(true, true, Some(s(1))), "link up: set up services");
        assert!(!wait_for_link_up(true, false, None), "never poked: try once");
        assert!(wait_for_link_up(true, false, Some(s(5))), "poked recently: wait");
        assert!(!wait_for_link_up(true, false, Some(LINK_DOWN_POKE)), "poke due");
    }

    #[test]
    fn a_new_phone_that_keeps_timing_out_asks_to_pair_again() {
        assert!(!suggests_pair_again(false, 1));
        assert!(suggests_pair_again(false, ADOPT_TIMEOUTS_BEFORE_REPAIR));
        assert!(
            !suggests_pair_again(true, 9),
            "a phone that connected since is just out of range"
        );
    }

    #[test]
    fn a_timeout_with_the_link_down_keeps_the_link_and_waits() {
        // An away phone: keep the link (its link-up handler connects at once) and wait for it.
        assert!(!drop_link_on_timeout(false, true));
        // Setup hung on a live link: Windows' objects may be stuck, so start fresh.
        assert!(drop_link_on_timeout(true, true));
        // A just-adopted phone that hasn't connected yet: fresh objects (switching back to a phone).
        assert!(drop_link_on_timeout(false, false));
        assert!(drop_link_on_timeout(true, false));
        // Kept, the next attempt waits for Windows' link-up instead of polling discovery.
        assert!(wait_for_link_up(true, false, Some(Duration::ZERO)));
    }
}
