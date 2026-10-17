//! Rust functions that Windows calls back into: window procedures, window enumeration, power
//! notifications. Nothing may unwind out of one. A panic that reaches an `extern "system"`
//! boundary aborts the process, and an exception escaping a window procedure ends it with
//! STATUS_FATAL_USER_CALLBACK_EXCEPTION (0xc000041d), the second half of the voice typing crash in
//! v0.5.18.
//!
//! `guarded` runs a callback's body, and if it panics, logs that and answers with the fallback
//! instead (for a window procedure, Windows' default handling). Release builds use
//! `panic = "abort"`, where a panic can't be caught at all, so each callback is also written not
//! to panic (no `unwrap`, no slicing that can fail). The guard keeps a debug build or a test alive
//! long enough to say where.

use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Run `body`; if it panics, log it under `what` and return `fallback()` instead.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn guarded<R>(what: &str, body: impl FnOnce() -> R, fallback: impl FnOnce() -> R) -> R {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(r) => r,
        Err(payload) => {
            log::error!(
                "{what}: a callback panicked ({}); carried on",
                panic_text(payload.as_ref())
            );
            fallback()
        }
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn panic_text(payload: &(dyn Any + Send)) -> &str {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s
    } else {
        "no message"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_the_answer_through() {
        assert_eq!(guarded("test", || 7, || 0), 7);
    }

    #[test]
    fn a_panic_becomes_the_fallback() {
        let got = guarded(
            "test",
            || -> i32 {
                let v: Vec<i32> = Vec::new();
                v[3]
            },
            || -1,
        );
        assert_eq!(got, -1);
        assert_eq!(guarded("test", || -> u8 { panic!("boom") }, || 9), 9);
    }

    #[test]
    fn names_what_panicked() {
        assert_eq!(panic_text(&"static"), "static");
        assert_eq!(panic_text(&String::from("owned")), "owned");
        assert_eq!(panic_text(&42u8), "no message");
    }
}
