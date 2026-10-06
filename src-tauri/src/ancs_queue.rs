//! The ANCS Control Point request queue: one request in flight at a time (Data
//! Source responses carry no request id beyond the notification UID, so they
//! must not interleave), with retries, timeouts and a record of whether every
//! notification's details were actually fetched.
//!
//! Pure state, so the rules that decide what the user sees — retry or give up,
//! and whether the post-reconnect sweep may mark anything "cleared" — are tested
//! here rather than inside the Bluetooth actor.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Request {
    Notification(u32),
    App(String),
}

/// A request is tried this many times (first send + retries) before giving up.
pub const MAX_ATTEMPTS: u8 = 3;

#[derive(Debug, Default)]
pub struct RequestQueue {
    queue: VecDeque<Request>,
    inflight: Option<(Request, Instant)>,
    attempts: HashMap<Request, u8>,
    /// Some notification's details could not be fetched even after retries, so
    /// "not refreshed this session" no longer implies "cleared on the phone".
    gave_up_on_notification: bool,
}

impl RequestQueue {
    pub fn push(&mut self, r: Request) {
        let inflight = self.inflight.as_ref().map(|(i, _)| i);
        if inflight != Some(&r) && !self.queue.contains(&r) {
            self.queue.push_back(r);
        }
    }

    /// The phone removed this notification: stop trying to fetch it.
    pub fn forget_notification(&mut self, uid: u32) {
        let r = Request::Notification(uid);
        self.queue.retain(|q| *q != r);
        self.attempts.remove(&r);
        // If it's the one in flight, drop it too so the queue advances now rather than waiting for
        // its fetch to time out — the phone won't answer for a notification it just removed.
        if self.inflight() == Some(&r) {
            self.inflight = None;
        }
    }

    /// Start the next request if none is in flight.
    pub fn start_next(&mut self, now: Instant) -> Option<Request> {
        if self.inflight.is_some() {
            return None;
        }
        let r = self.queue.pop_front()?;
        *self.attempts.entry(r.clone()).or_default() += 1;
        self.inflight = Some((r.clone(), now));
        Some(r)
    }

    pub fn inflight(&self) -> Option<&Request> {
        self.inflight.as_ref().map(|(r, _)| r)
    }

    /// Restart the in-flight request's clock (e.g. after the channel was busy
    /// carrying a late reply to an earlier request).
    pub fn touch(&mut self, now: Instant) {
        if let Some((_, t)) = self.inflight.as_mut() {
            *t = now;
        }
    }

    /// A complete response for `r` arrived — whether or not it's the one in flight
    /// (a late reply to a timed-out request still carries valid data).
    pub fn complete(&mut self, r: &Request) {
        if self.inflight() == Some(r) {
            self.inflight = None;
        }
        self.queue.retain(|q| q != r);
        self.attempts.remove(r);
    }

    /// The in-flight request failed transiently (write error, timeout, garbled
    /// reply): queue it again, or give up after `MAX_ATTEMPTS`. Returns the
    /// request that was given up on, if any.
    pub fn fail_inflight(&mut self) -> Option<Request> {
        let (r, _) = self.inflight.take()?;
        if self.attempts.get(&r).copied().unwrap_or(0) < MAX_ATTEMPTS {
            self.queue.push_back(r);
            return None;
        }
        self.attempts.remove(&r);
        if matches!(r, Request::Notification(_)) {
            self.gave_up_on_notification = true;
        }
        Some(r)
    }

    /// The phone answered that the target doesn't exist (e.g. the notification
    /// was dismissed before we asked): drop it, no retry, not a failure.
    pub fn drop_inflight(&mut self) {
        if let Some((r, _)) = self.inflight.take() {
            self.attempts.remove(&r);
        }
    }

    pub fn timed_out(&self, now: Instant, timeout: Duration) -> bool {
        self.inflight
            .as_ref()
            .is_some_and(|(_, t)| now.duration_since(*t) > timeout)
    }

    pub fn is_idle(&self) -> bool {
        self.inflight.is_none() && self.queue.is_empty()
    }

    /// True if every notification the phone announced had its details fetched
    /// (or was explicitly gone). Only then may unrefreshed rows be treated as
    /// cleared while tug was away.
    pub fn all_notifications_fetched(&self) -> bool {
        !self.gave_up_on_notification
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Duration = Duration::from_secs(5);

    #[test]
    fn one_in_flight_at_a_time_and_deduped() {
        let mut q = RequestQueue::default();
        let now = Instant::now();
        q.push(Request::Notification(1));
        q.push(Request::Notification(1));
        q.push(Request::Notification(2));
        assert_eq!(q.start_next(now), Some(Request::Notification(1)));
        assert_eq!(q.start_next(now), None, "serialised");
        q.push(Request::Notification(1));
        q.complete(&Request::Notification(1));
        assert_eq!(
            q.start_next(now),
            Some(Request::Notification(2)),
            "1 not re-queued while in flight"
        );
        q.complete(&Request::Notification(2));
        assert!(q.is_idle());
    }

    #[test]
    fn transient_failures_retry_then_give_up() {
        let mut q = RequestQueue::default();
        let now = Instant::now();
        q.push(Request::Notification(7));
        for attempt in 1..=MAX_ATTEMPTS {
            assert_eq!(q.start_next(now), Some(Request::Notification(7)), "attempt {attempt}");
            let gave_up = q.fail_inflight();
            assert_eq!(gave_up.is_some(), attempt == MAX_ATTEMPTS);
        }
        assert!(q.is_idle());
        assert!(!q.all_notifications_fetched(), "sweep must not trust missing rows now");
    }

    #[test]
    fn gone_on_the_phone_is_not_a_failure() {
        let mut q = RequestQueue::default();
        q.push(Request::Notification(7));
        q.start_next(Instant::now());
        q.drop_inflight();
        assert!(q.is_idle());
        assert!(q.all_notifications_fetched());
    }

    #[test]
    fn app_name_failures_do_not_block_the_sweep() {
        let mut q = RequestQueue::default();
        q.push(Request::App("com.example".into()));
        for _ in 0..MAX_ATTEMPTS {
            q.start_next(Instant::now());
            q.fail_inflight();
        }
        assert!(q.all_notifications_fetched());
    }

    #[test]
    fn late_reply_completes_its_request_without_disturbing_the_current_one() {
        let mut q = RequestQueue::default();
        let now = Instant::now();
        q.push(Request::Notification(1));
        q.push(Request::Notification(2));
        q.start_next(now);
        q.fail_inflight(); // 1 timed out and was re-queued behind 2
        assert_eq!(q.start_next(now), Some(Request::Notification(2)));
        // 1's slow reply finally lands.
        q.complete(&Request::Notification(1));
        assert_eq!(
            q.inflight(),
            Some(&Request::Notification(2)),
            "current request untouched"
        );
        q.complete(&Request::Notification(2));
        assert!(q.is_idle(), "1 isn't fetched a second time");
    }

    #[test]
    fn times_out_and_touch_restarts_the_clock() {
        let mut q = RequestQueue::default();
        let start = Instant::now();
        q.push(Request::Notification(1));
        q.start_next(start);
        assert!(!q.timed_out(start + Duration::from_secs(4), T));
        assert!(q.timed_out(start + Duration::from_secs(6), T));
        q.touch(start + Duration::from_secs(6));
        assert!(!q.timed_out(start + Duration::from_secs(10), T));
    }

    #[test]
    fn forgetting_a_removed_notification_stops_retries() {
        let mut q = RequestQueue::default();
        q.push(Request::Notification(3));
        q.push(Request::Notification(4));
        q.forget_notification(3);
        assert_eq!(q.start_next(Instant::now()), Some(Request::Notification(4)));
    }

    #[test]
    fn forgetting_the_in_flight_notification_advances_without_a_timeout() {
        let mut q = RequestQueue::default();
        let now = Instant::now();
        q.push(Request::Notification(3));
        q.push(Request::Notification(4));
        assert_eq!(q.start_next(now), Some(Request::Notification(3)));
        // Removed on the phone while its fetch was in flight: don't wait for the fetch to time out.
        q.forget_notification(3);
        assert_eq!(q.inflight(), None, "in-flight fetch dropped");
        assert_eq!(q.start_next(now), Some(Request::Notification(4)));
    }
}
