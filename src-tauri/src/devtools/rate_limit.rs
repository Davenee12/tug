//! Per-tool rate limits, so a runaway AI agent can't hammer the phone or read the whole history
//! in a loop. A sliding window per tool; pure (the caller passes the time).

use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    pub max: usize,
    pub per_ms: i64,
}

const MINUTE: i64 = 60_000;

/// How often each tool may be called.
pub fn limit_for(tool: &str) -> Limit {
    match tool {
        // Texts go to real people: a handful per 10 minutes, each still confirmed by a click.
        "send_text" => Limit {
            max: 5,
            per_ms: 10 * MINUTE,
        },
        "media_control" => Limit {
            max: 30,
            per_ms: MINUTE,
        },
        "get_tugboat_file" => Limit {
            max: 20,
            per_ms: MINUTE,
        },
        "offer_files" => Limit {
            max: 10,
            per_ms: MINUTE,
        },
        "search_messages" => Limit {
            max: 30,
            per_ms: MINUTE,
        },
        _ => Limit {
            max: 60,
            per_ms: MINUTE,
        },
    }
}

#[derive(Default)]
pub struct RateLimiter {
    calls: HashMap<String, VecDeque<i64>>,
}

impl RateLimiter {
    /// Count a call to `tool` at `now_ms`, or say how many seconds until one would be allowed.
    pub fn check(&mut self, tool: &str, now_ms: i64) -> Result<(), u64> {
        let limit = limit_for(tool);
        let q = self.calls.entry(tool.to_string()).or_default();
        while q.front().is_some_and(|&t| t <= now_ms - limit.per_ms) {
            q.pop_front();
        }
        if q.len() >= limit.max {
            let oldest = *q.front().expect("non-empty when full");
            let wait_ms = (oldest + limit.per_ms - now_ms).max(1);
            return Err((wait_ms as u64).div_ceil(1000));
        }
        q.push_back(now_ms);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_the_limit_then_waits() {
        let mut r = RateLimiter::default();
        for i in 0..5 {
            assert_eq!(r.check("send_text", i * 1000), Ok(()));
        }
        // The first call (at 0) frees up after 10 minutes.
        assert_eq!(r.check("send_text", 60_000), Err(540));
        assert_eq!(r.check("send_text", 10 * MINUTE), Ok(()));
        assert!(r.check("send_text", 10 * MINUTE + 1).is_err());
    }

    #[test]
    fn tools_are_counted_separately() {
        let mut r = RateLimiter::default();
        for i in 0..60 {
            assert_eq!(r.check("phone_status", i), Ok(()));
        }
        assert!(r.check("phone_status", 61).is_err());
        assert_eq!(r.check("get_latest_code", 61), Ok(()));
    }

    #[test]
    fn refusals_dont_use_up_the_allowance() {
        let mut r = RateLimiter::default();
        for i in 0..30 {
            r.check("media_control", i).unwrap();
        }
        for i in 0..100 {
            assert!(r.check("media_control", 100 + i).is_err());
        }
        assert_eq!(r.check("media_control", MINUTE + 30), Ok(()));
    }

    #[test]
    fn waits_are_rounded_up_to_whole_seconds() {
        let mut r = RateLimiter::default();
        for _ in 0..20 {
            r.check("get_tugboat_file", 0).unwrap();
        }
        assert_eq!(r.check("get_tugboat_file", MINUTE - 1), Err(1));
    }
}
