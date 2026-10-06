//! Forwarding uncaught frontend errors to the Rust log, so the next window crash is diagnosable.
//! A WebView2 crash (the one in the log on 2026-10-05) left nothing from the web side behind. The
//! frontend now hands its `window.onerror`, unhandled promise rejections and Vue errors to the
//! `log_frontend_error` command, which lands them in the same rotated log the diagnostics report
//! reads.
//!
//! Two safeguards keep this from becoming a liability: the summary is redacted and length-capped
//! (an error string should never carry a phone number or a message body, but we strip them anyway),
//! and a rate limiter caps how many land per window so a tight error loop can't flood the log.
//! Both pieces are pure so they can be unit-tested without a running app.

use std::time::{Duration, Instant};

/// At most `MAX_PER_WINDOW` forwarded errors per `WINDOW`; the rest are dropped silently.
pub const WINDOW: Duration = Duration::from_secs(10);
pub const MAX_PER_WINDOW: u32 = 5;

const MAX_NAME: usize = 60;
const MAX_MESSAGE: usize = 200;
const MAX_SOURCE: usize = 200;

/// A fixed-window rate limiter. Pure given `now`, so the command owns the clock and tests don't need
/// one. Each full window allows up to `MAX_PER_WINDOW` events; the window resets lazily on the first
/// event after it elapses.
#[derive(Default)]
pub struct RateLimiter {
    window_start: Option<Instant>,
    count: u32,
}

impl RateLimiter {
    /// Record an attempt at `now`; `true` if it's within the budget and should be logged.
    pub fn allow(&mut self, now: Instant) -> bool {
        let elapsed = self
            .window_start
            .is_none_or(|start| now.duration_since(start) >= WINDOW);
        if elapsed {
            self.window_start = Some(now);
            self.count = 0;
        }
        if self.count < MAX_PER_WINDOW {
            self.count += 1;
            true
        } else {
            false
        }
    }
}

/// Keep only the known kinds the frontend sends; anything else is normalised to "error" so a
/// malformed call can't put arbitrary text at the front of a log line.
fn kind_label(kind: &str) -> &'static str {
    match kind {
        "unhandledrejection" => "unhandled rejection",
        "vue" => "vue",
        _ => "error",
    }
}

/// Trim a field to `max` characters (not bytes — never split a char) and drop surrounding space.
fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "…"
}

/// A one-line, redacted summary of a frontend error for the log: kind, error name, the trimmed
/// message, and the top of the stack (or the Vue component info). Redaction strips anything
/// number- or email-shaped as a belt-and-braces measure; the message is never a message body.
pub fn summary(kind: &str, name: &str, message: &str, source: &str) -> String {
    let name = clip(name, MAX_NAME);
    let message = crate::diagnostics::redact(&clip(message, MAX_MESSAGE));
    let source = crate::diagnostics::redact(&clip(source, MAX_SOURCE));
    let mut out = format!("frontend {}", kind_label(kind));
    if !name.is_empty() {
        out.push_str(&format!(": {name}"));
    }
    if !message.is_empty() {
        out.push_str(&format!(": {message}"));
    }
    if !source.is_empty() {
        out.push_str(&format!(" @ {source}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_the_cap_then_blocks_until_the_window_rolls() {
        let mut rl = RateLimiter::default();
        let t0 = Instant::now();
        for _ in 0..MAX_PER_WINDOW {
            assert!(rl.allow(t0));
        }
        // Over budget within the same window.
        assert!(!rl.allow(t0));
        assert!(!rl.allow(t0 + Duration::from_secs(1)));
        // A fresh window opens the budget again.
        assert!(rl.allow(t0 + WINDOW));
        assert!(rl.allow(t0 + WINDOW + Duration::from_secs(1)));
    }

    #[test]
    fn summary_lays_out_the_fields_and_normalises_the_kind() {
        assert_eq!(
            summary("error", "TypeError", "x is not a function", "at render (App.vue:12)"),
            "frontend error: TypeError: x is not a function @ at render (App.vue:12)"
        );
        assert_eq!(
            summary("unhandledrejection", "Error", "boom", ""),
            "frontend unhandled rejection: Error: boom"
        );
        // An unknown kind can't inject its own text.
        assert_eq!(summary("<script>", "E", "", ""), "frontend error: E");
    }

    #[test]
    fn summary_redacts_and_caps_lengths() {
        // A stray number or email in a message is scrubbed.
        assert_eq!(
            summary("vue", "Error", "failed for +13025550142", "render"),
            "frontend vue: Error: failed for [number] @ render"
        );
        // A very long message is clipped (with an ellipsis), so one error can't be a log flood.
        let long = "a".repeat(500);
        let out = summary("error", "Error", &long, "");
        assert!(out.ends_with('…'));
        assert!(out.len() < 300);
    }
}
