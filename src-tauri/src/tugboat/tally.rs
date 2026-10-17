//! Logging what other devices on the network do to Tugboat without letting them fill tug's log:
//! any device on the Wi-Fi can send requests that are refused, or open connections that go
//! nowhere, as fast as it likes. The first event is logged; the rest of that minute are counted
//! and reported with the next line (or when Tugboat closes).

use std::time::{Duration, Instant};

/// At most one line per this long, per kind of event.
pub const WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub struct Tally {
    window: Duration,
    /// When the last logged line went out.
    logged_at: Option<Instant>,
    /// Events since then that weren't logged.
    held: u64,
}

impl Default for Tally {
    fn default() -> Tally {
        Tally::new(WINDOW)
    }
}

impl Tally {
    pub fn new(window: Duration) -> Tally {
        Tally {
            window,
            logged_at: None,
            held: 0,
        }
    }

    /// An event happened at `now`. `Some(held)` when it should be logged, `held` being how many
    /// were held back since the last line; `None` to stay quiet (it's counted).
    pub fn note(&mut self, now: Instant) -> Option<u64> {
        match self.logged_at {
            Some(at) if now.saturating_duration_since(at) < self.window => {
                self.held += 1;
                None
            }
            _ => {
                self.logged_at = Some(now);
                Some(std::mem::take(&mut self.held))
            }
        }
    }

    /// Events held back and not yet reported (and forget them).
    pub fn flush(&mut self) -> u64 {
        std::mem::take(&mut self.held)
    }
}

/// " (and 12 more since the last report)", or nothing.
pub fn more(held: u64) -> String {
    if held == 0 {
        String::new()
    } else {
        format!(" (and {held} more since the last report)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_a_minute_with_a_count_of_the_rest() {
        let t0 = Instant::now();
        let mut t = Tally::new(Duration::from_secs(60));
        assert_eq!(t.note(t0), Some(0), "the first is logged");
        for i in 1..=500 {
            assert_eq!(t.note(t0 + Duration::from_millis(i)), None, "a flood stays quiet");
        }
        // A minute on, the next one is logged with how many were held back.
        assert_eq!(t.note(t0 + Duration::from_secs(61)), Some(500));
        assert_eq!(t.note(t0 + Duration::from_secs(62)), None);
        assert_eq!(t.flush(), 1, "what's left is reported when Tugboat closes");
        assert_eq!(t.flush(), 0);
    }

    #[test]
    fn says_how_many_more() {
        assert_eq!(more(0), "");
        assert_eq!(more(3), " (and 3 more since the last report)");
    }
}
