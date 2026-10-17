//! Files held for the phone by `tug boat` and AI tools (`send_file_to_phone`): the clipboard's
//! pending-offer pattern (`clip.rs`) for files. Pure (plain values in, plain values out), so every
//! state is tested without a phone.
//!
//! - Only after the person clicked Send on tug's card: the files go on offer like files dragged
//!   onto tug, and are held for a window (2 minutes unless the command asked for another) while
//!   the phone's page connects and downloads them.
//! - Delivered once the phone has fetched every file whole. The files stay on offer after that,
//!   like dragged ones: the page needs them listed for its Save button.
//! - If the window ends first they're withdrawn, so "timed out" is true: the phone can't fetch
//!   them later. A file the phone is fetching at that moment gets up to `GRACE` to finish.
//! - A file removed in tug's panel ends the hold as withdrawn (the rest are withdrawn too).
//! - What the panel sees is the names and when the hold ends, never paths.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde::Serialize;

/// How long a file still being fetched when the window ends may take to finish
/// (`tug_bridge::protocol::FINISH_GRACE_SECS`, which the bridge's time limits count on).
pub const GRACE: Duration = Duration::from_secs(60);
/// Finished holds remembered, so a late question about one still gets its answer.
const KEEP_DONE: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Still on offer; the phone has fetched `fetched` of the `total` files.
    Waiting { fetched: usize, total: usize },
    /// The phone fetched every file.
    Delivered,
    /// The window ended first.
    TimedOut,
    /// One of its files was taken off offer in tug.
    Withdrawn,
}

impl Verdict {
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Waiting { .. } => "waiting",
            Verdict::Delivered => "delivered",
            Verdict::TimedOut => "timed out",
            Verdict::Withdrawn => "withdrawn",
        }
    }
}

/// What's become of a hold. `downloads[i]` is how many times the phone fetched file `i` whole,
/// or `None` once it's no longer on offer; `moving` says the phone is fetching one right now.
pub fn assess(downloads: &[Option<u32>], age: Duration, window: Duration, moving: bool) -> Verdict {
    let fetched = downloads.iter().filter(|d| d.is_some_and(|n| n > 0)).count();
    if !downloads.is_empty() && fetched == downloads.len() {
        return Verdict::Delivered;
    }
    if downloads.is_empty() || downloads.iter().any(Option::is_none) {
        return Verdict::Withdrawn;
    }
    if age >= window + GRACE || (age >= window && !moving) {
        return Verdict::TimedOut;
    }
    Verdict::Waiting {
        fetched,
        total: downloads.len(),
    }
}

/// One `tug boat` (or `send_file_to_phone`) call's files.
#[derive(Debug, Clone)]
pub struct Hold {
    pub id: u32,
    /// The session's offer ids, one per file.
    pub offers: Vec<String>,
    /// What the phone sees them called.
    pub names: Vec<String>,
    pub since: Instant,
    pub window: Duration,
    /// When the window ends (Unix ms), for the panel.
    pub expires_at: i64,
}

/// Files held for the phone right now, as the Tugboat panel shows them: their names and when the
/// soonest hold ends. Mirrored in `src/types/protocol.ts` (`TugboatHeld`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldStatus {
    pub names: Vec<String>,
    pub expires_at: i64,
}

#[derive(Default)]
pub struct Holds {
    waiting: Vec<Hold>,
    done: VecDeque<(u32, Verdict)>,
    next_id: u32,
}

impl Holds {
    /// Start holding these offers for `window`. Returns the hold's id.
    pub fn add(&mut self, offers: Vec<String>, names: Vec<String>, window: Duration, now: Instant, now_ms: i64) -> u32 {
        self.next_id += 1;
        let id = self.next_id;
        self.waiting.push(Hold {
            id,
            offers,
            names,
            since: now,
            window,
            expires_at: now_ms + window.as_millis() as i64,
        });
        id
    }

    pub fn get(&self, id: u32) -> Option<&Hold> {
        self.waiting.iter().find(|h| h.id == id)
    }

    /// The ids of the holds still waiting.
    pub fn ids(&self) -> Vec<u32> {
        self.waiting.iter().map(|h| h.id).collect()
    }

    /// The hold is over: forget it, and remember how it ended.
    pub fn finish(&mut self, id: u32, verdict: Verdict) -> Option<Hold> {
        let i = self.waiting.iter().position(|h| h.id == id)?;
        let hold = self.waiting.remove(i);
        self.done.push_back((id, verdict));
        while self.done.len() > KEEP_DONE {
            self.done.pop_front();
        }
        Some(hold)
    }

    /// How a finished hold ended.
    pub fn outcome(&self, id: u32) -> Option<Verdict> {
        self.done.iter().find(|(i, _)| *i == id).map(|(_, v)| *v)
    }

    pub fn status(&self) -> Option<HeldStatus> {
        let soonest = self.waiting.iter().map(|h| h.expires_at).min()?;
        Some(HeldStatus {
            names: self.waiting.iter().flat_map(|h| h.names.iter().cloned()).collect(),
            expires_at: soonest,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: Duration = Duration::from_secs(120);
    const S: Duration = Duration::from_secs(1);

    #[test]
    fn delivered_once_every_file_is_fetched() {
        assert_eq!(
            assess(&[Some(0), Some(0)], 10 * S, W, false),
            Verdict::Waiting { fetched: 0, total: 2 }
        );
        assert_eq!(
            assess(&[Some(1), Some(0)], 10 * S, W, true),
            Verdict::Waiting { fetched: 1, total: 2 }
        );
        assert_eq!(assess(&[Some(1), Some(2)], 10 * S, W, false), Verdict::Delivered);
        // Delivered wins even right at the end of the window.
        assert_eq!(assess(&[Some(1)], W + GRACE, W, false), Verdict::Delivered);
    }

    #[test]
    fn times_out_when_the_window_ends_unless_a_file_is_still_coming() {
        assert_eq!(
            assess(&[Some(0)], W - S, W, false),
            Verdict::Waiting { fetched: 0, total: 1 }
        );
        assert_eq!(assess(&[Some(0)], W, W, false), Verdict::TimedOut);
        // The phone is fetching right now: it gets the grace to finish...
        assert_eq!(
            assess(&[Some(0)], W + 30 * S, W, true),
            Verdict::Waiting { fetched: 0, total: 1 }
        );
        // ...but not for ever.
        assert_eq!(assess(&[Some(0)], W + GRACE, W, true), Verdict::TimedOut);
    }

    #[test]
    fn a_file_taken_off_offer_ends_it() {
        assert_eq!(assess(&[Some(1), None], 5 * S, W, false), Verdict::Withdrawn);
        assert_eq!(assess(&[], 5 * S, W, false), Verdict::Withdrawn);
    }

    #[test]
    fn holds_are_kept_until_finished_then_remembered() {
        let now = Instant::now();
        let mut h = Holds::default();
        assert!(h.status().is_none());
        let a = h.add(vec!["o1".into()], vec!["report.pdf".into()], W, now, 1_000);
        let b = h.add(
            vec!["o2".into(), "o3".into()],
            vec!["a.png".into(), "b.png".into()],
            Duration::from_secs(30),
            now,
            2_000,
        );
        assert_ne!(a, b);
        assert_eq!(h.ids(), vec![a, b]);
        let st = h.status().unwrap();
        assert_eq!(st.names, vec!["report.pdf", "a.png", "b.png"]);
        assert_eq!(st.expires_at, 2_000 + 30_000, "the soonest end");
        assert_eq!(h.get(a).unwrap().offers, vec!["o1".to_string()]);

        assert!(h.finish(b, Verdict::Delivered).is_some());
        assert!(h.finish(b, Verdict::TimedOut).is_none(), "finished once");
        assert_eq!(h.outcome(b), Some(Verdict::Delivered));
        assert_eq!(h.outcome(a), None);
        assert_eq!(h.status().unwrap().names, vec!["report.pdf"]);
        h.finish(a, Verdict::TimedOut);
        assert!(h.status().is_none());

        // Only the last few outcomes are kept.
        for _ in 0..KEEP_DONE {
            let id = h.add(vec![], vec![], W, now, 0);
            h.finish(id, Verdict::Withdrawn);
        }
        assert_eq!(h.outcome(a), None);
    }

    #[test]
    fn the_panel_sees_names_and_the_end_in_protocol_ts_shape() {
        let st = HeldStatus {
            names: vec!["report.pdf".into()],
            expires_at: 5,
        };
        assert_eq!(
            serde_json::to_string(&st).unwrap(),
            r#"{"names":["report.pdf"],"expiresAt":5}"#
        );
    }
}
