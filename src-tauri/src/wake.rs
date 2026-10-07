//! Telling a real PC sleep from a stalled thread.
//!
//! The v0.5.10 heartbeat compared wall-clock time between 1 s ticks on the Bluetooth thread. When a
//! Windows Bluetooth call blocked that thread for ~11 s while the adapter hung (2026-10-06), the
//! late tick read as "the PC slept" and tug tore down a link that was recovering by itself. Now the
//! heartbeat runs on a thread of its own, and a wake is judged by Windows' clocks rather than by
//! how late a tick was: interrupt time keeps counting while the PC sleeps, *unbiased* interrupt time
//! doesn't, so how much further the first moved than the second over one beat is time spent asleep.
//! A thread that was merely held up moves both alike and reads as no sleep at all.
//!
//! Two more signals cover sleep the unbiased clock may not exclude (Modern Standby, the only sleep
//! this PC's firmware offers, keeps the system in S0): Windows' own resume notification, and the
//! heartbeat's own thread being frozen for a long time, which only happens when Windows suspends the
//! whole process (as Modern Standby does to desktop apps). The Bluetooth thread can no longer hold
//! the heartbeat up, so an adapter stall can't produce either.
//!
//! Pure, so the decision is unit-tested; reading the clocks is in `ble::actor::heartbeat`.

use std::time::Duration;

/// Sleep of at least this long between two heartbeats is a wake worth reconnecting for.
pub const RESUME_GAP: Duration = Duration::from_secs(10);

/// The heartbeat's own thread frozen this long, with no sleep on the clocks, means Windows suspended
/// the whole process: Modern Standby does that to desktop apps. Far longer than any scheduling delay
/// a thread that only sleeps could see, so a busy or stalled PC can't trip it.
pub const FROZEN_GAP: Duration = Duration::from_secs(60);

/// After a wake, further wake signals are ignored for this many heartbeats: one resume can show up
/// as both a clock jump and Windows' notification a beat or two apart, and should relink once.
pub const QUIET_BEATS: u32 = 10;

/// Both clocks at one heartbeat, in 100 ns units since boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clocks {
    /// Interrupt time: keeps counting while the PC sleeps or hibernates.
    pub with_sleep: u64,
    /// Unbiased interrupt time: stops while the PC sleeps or hibernates.
    pub awake: u64,
}

fn from_ticks(ticks: u64) -> Duration {
    Duration::from_nanos(ticks.saturating_mul(100))
}

/// Time the PC spent asleep between two readings: how much further the sleep-inclusive clock moved
/// than the awake-only one. A stalled thread moves both alike, so it reads as zero.
pub fn slept_between(prev: Clocks, now: Clocks) -> Duration {
    let total = now.with_sleep.saturating_sub(prev.with_sleep);
    let awake = now.awake.saturating_sub(prev.awake);
    from_ticks(total.saturating_sub(awake))
}

/// How long one heartbeat really took (sleep included).
pub fn beat_length(prev: Clocks, now: Clocks) -> Duration {
    from_ticks(now.with_sleep.saturating_sub(prev.with_sleep))
}

/// Which signal said the PC woke, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeCause {
    /// The clocks show the PC asleep (sleep or hibernate).
    Slept,
    /// Windows reported a resume.
    Resumed,
    /// The whole process was suspended for minutes (Modern Standby).
    Frozen,
}

/// What one heartbeat means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Beat {
    Normal,
    /// The PC slept and woke: reconnect. `away` is roughly how long it was gone, for the log.
    Woke {
        away: Duration,
        cause: WakeCause,
    },
    /// The heartbeat was held up this long but nothing says the PC slept. Not a wake: logged so a
    /// missed wake (or a stalled PC) shows up in the log.
    HeldUp {
        by: Duration,
    },
}

/// The heartbeat's memory: the last clock reading and the post-wake quiet period.
#[derive(Debug, Clone)]
pub struct WakeWatch {
    prev: Clocks,
    quiet: u32,
}

impl WakeWatch {
    pub fn new(now: Clocks) -> Self {
        Self { prev: now, quiet: 0 }
    }

    /// Judge one heartbeat from the clocks now and whether Windows reported a resume since the last.
    pub fn beat(&mut self, now: Clocks, resumed: bool) -> Beat {
        let prev = std::mem::replace(&mut self.prev, now);
        let slept = slept_between(prev, now);
        let took = beat_length(prev, now);
        let cause = if slept >= RESUME_GAP {
            Some(WakeCause::Slept)
        } else if resumed {
            Some(WakeCause::Resumed)
        } else if took >= FROZEN_GAP {
            Some(WakeCause::Frozen)
        } else {
            None
        };
        let quiet = self.quiet;
        self.quiet = self.quiet.saturating_sub(1);
        match cause {
            // The same resume seen again by another signal: it has already been acted on. Sleep on
            // the clocks always counts, though: that's a second sleep, not an echo of the first.
            Some(cause) if quiet > 0 && cause != WakeCause::Slept => Beat::Normal,
            Some(cause) => {
                self.quiet = QUIET_BEATS;
                let away = if cause == WakeCause::Slept { slept } else { took };
                Beat::Woke { away, cause }
            }
            None if took >= RESUME_GAP => Beat::HeldUp { by: took },
            None => Beat::Normal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: u64 = 10_000_000;

    fn at(with_sleep_secs: u64, awake_secs: u64) -> Clocks {
        Clocks {
            with_sleep: with_sleep_secs * SEC,
            awake: awake_secs * SEC,
        }
    }

    #[test]
    fn sleep_is_the_gap_between_the_two_clocks() {
        assert_eq!(
            slept_between(at(100, 100), at(101, 101)),
            Duration::ZERO,
            "a normal beat"
        );
        assert_eq!(slept_between(at(100, 100), at(3700, 101)), Duration::from_secs(3599));
        // A stalled thread: both clocks moved 11 s.
        assert_eq!(slept_between(at(100, 100), at(111, 111)), Duration::ZERO);
        // Clocks out of step (a reading taken mid-tick) can't underflow.
        assert_eq!(slept_between(at(100, 101), at(101, 103)), Duration::ZERO);
        assert_eq!(slept_between(at(101, 100), at(100, 100)), Duration::ZERO);
    }

    #[test]
    fn a_normal_heartbeat_is_nothing() {
        let mut w = WakeWatch::new(at(0, 0));
        assert_eq!(w.beat(at(1, 1), false), Beat::Normal);
        assert_eq!(w.beat(at(3, 3), false), Beat::Normal, "scheduling jitter");
    }

    #[test]
    fn a_stalled_thread_is_not_a_wake() {
        // 2026-10-06: the heartbeat shared the Bluetooth thread, a WinRT call blocked it ~11 s while
        // the adapter hung, and the late tick was taken for a wake. Both clocks moved alike.
        let mut w = WakeWatch::new(at(100, 100));
        assert_eq!(
            w.beat(at(111, 111), false),
            Beat::HeldUp {
                by: Duration::from_secs(11)
            }
        );
        assert_eq!(w.beat(at(112, 112), false), Beat::Normal);
        // Even a long stall, short of a suspended process.
        assert_eq!(
            w.beat(at(152, 152), false),
            Beat::HeldUp {
                by: Duration::from_secs(40)
            }
        );
    }

    #[test]
    fn sleep_on_the_clocks_is_a_wake() {
        let mut w = WakeWatch::new(at(100, 100));
        assert_eq!(
            w.beat(at(3701, 101), false),
            Beat::Woke {
                away: Duration::from_secs(3600),
                cause: WakeCause::Slept
            }
        );
        let mut w = WakeWatch::new(at(100, 100));
        assert!(
            matches!(w.beat(at(100 + 11, 101), false), Beat::Woke { .. }),
            "10 s asleep is enough"
        );
        let mut w = WakeWatch::new(at(100, 100));
        assert_eq!(
            w.beat(at(109, 101), false),
            Beat::Normal,
            "8 s on the clocks isn't (a short beat around a dozing tick)"
        );
    }

    #[test]
    fn windows_reporting_a_resume_is_a_wake() {
        // Modern Standby can keep the unbiased clock running, so the clocks show nothing.
        let mut w = WakeWatch::new(at(100, 100));
        assert_eq!(
            w.beat(at(101, 101), true),
            Beat::Woke {
                away: Duration::from_secs(1),
                cause: WakeCause::Resumed
            }
        );
    }

    #[test]
    fn a_process_frozen_for_minutes_is_a_wake() {
        // Modern Standby suspends desktop apps; if the unbiased clock kept running and Windows'
        // notification didn't arrive, the heartbeat thread was still frozen for the whole standby.
        let mut w = WakeWatch::new(at(100, 100));
        assert_eq!(
            w.beat(at(100 + 7200, 100 + 7200), false),
            Beat::Woke {
                away: Duration::from_secs(7200),
                cause: WakeCause::Frozen
            }
        );
        let mut w = WakeWatch::new(at(100, 100));
        assert!(
            matches!(w.beat(at(160, 160), false), Beat::Woke { .. }),
            "a minute frozen"
        );
    }

    #[test]
    fn a_second_sleep_soon_after_a_wake_still_counts() {
        // Windows' notification first, then the lid closes again before the quiet period is over.
        let mut w = WakeWatch::new(at(100, 100));
        assert!(matches!(w.beat(at(101, 101), true), Beat::Woke { .. }));
        assert_eq!(
            w.beat(at(1902, 102), false),
            Beat::Woke {
                away: Duration::from_secs(1800),
                cause: WakeCause::Slept
            }
        );
        // ...and its own echo is still swallowed.
        assert_eq!(w.beat(at(1903, 103), true), Beat::Normal);
    }

    #[test]
    fn one_resume_seen_twice_relinks_once() {
        let mut w = WakeWatch::new(at(100, 100));
        assert!(
            matches!(w.beat(at(3701, 101), false), Beat::Woke { .. }),
            "the clocks first"
        );
        assert_eq!(w.beat(at(3702, 102), true), Beat::Normal, "then Windows' notification");
        // The quiet period runs out after QUIET_BEATS normal beats; a later resume counts again.
        let mut t = 102;
        for _ in 1..QUIET_BEATS {
            t += 1;
            assert_eq!(w.beat(at(3600 + t, t), false), Beat::Normal);
        }
        t += 1;
        assert!(matches!(w.beat(at(3600 + t, t), true), Beat::Woke { .. }));
    }
}
