//! The "send this?" confirmation: an AI tool (or `tug text`, `tug boat`) can only ask; the person
//! decides with a click in tug. One question at a time, answered at most once, and it expires
//! after 2 minutes as not sent. Pure state machine: the async waiting lives in `mod.rs`.
//!
//! ```text
//! open ──► Waiting ──Send──────► Approved   (then tug sends it)
//!             ├─────Don't send─► Declined
//!             ├─────2 minutes──► TimedOut
//!             └─────off/revoke─► Cancelled
//! ```

use serde::Serialize;

pub const CONFIRM_TIMEOUT_MS: i64 = 2 * 60 * 1000;

/// What the confirmation card shows. Mirrored in `src/types/protocol.ts` (`DevToolsConfirm`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmRequest {
    pub id: u64,
    /// Who's asking, tamed (`ClientInfo::asker`): "Claude Code (an AI tool)", "An AI tool", "tug command".
    pub tool: String,
    /// What it wants to send: `kind` says which.
    #[serde(flatten)]
    pub ask: Ask,
    /// Unix ms after which it counts as not sent.
    pub expires_at: i64,
}

/// What's being asked about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Ask {
    /// "<tool> wants to text <name>".
    Text {
        /// The contact's name (or the number, formatted, when it isn't a contact).
        to_name: String,
        /// The number it would go to.
        to_address: String,
        message: String,
    },
    /// "Send report.pdf (2.1 MB) to your iPhone?"
    Files { files: Vec<ConfirmFile> },
}

/// One file on the card: the name the phone will see, its size, and the folder it's in (so the
/// person sees *which* `notes.txt`, or that it's in `.ssh`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmFile {
    pub name: String,
    pub size: u64,
    /// `None` for what was piped into `tug boat -`.
    pub folder: Option<String>,
}

impl Ask {
    pub fn is_files(&self) -> bool {
        matches!(self, Ask::Files { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Approved,
    Declined,
    TimedOut,
    Cancelled,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Busy;

#[derive(Default)]
pub struct Confirmations {
    pending: Option<ConfirmRequest>,
    next_id: u64,
}

impl Confirmations {
    pub fn pending(&self) -> Option<&ConfirmRequest> {
        self.pending.as_ref()
    }

    /// Ask. Refused while another question is still waiting (an expired one is cleared first).
    pub fn open(&mut self, tool: String, ask: Ask, now_ms: i64) -> Result<ConfirmRequest, Busy> {
        if let Some(p) = &self.pending {
            if now_ms < p.expires_at {
                return Err(Busy);
            }
            self.pending = None;
        }
        self.next_id += 1;
        let req = ConfirmRequest {
            id: self.next_id,
            tool,
            ask,
            expires_at: now_ms + CONFIRM_TIMEOUT_MS,
        };
        self.pending = Some(req.clone());
        Ok(req)
    }

    /// The person clicked. Only the question on screen can be answered, only once, and not after
    /// it expired (a late click counts as timed out, never as Send).
    pub fn decide(&mut self, id: u64, send: bool, now_ms: i64) -> Option<Resolution> {
        let p = self.pending.as_ref().filter(|p| p.id == id)?;
        let r = if now_ms >= p.expires_at {
            Resolution::TimedOut
        } else if send {
            Resolution::Approved
        } else {
            Resolution::Declined
        };
        self.pending = None;
        Some(r)
    }

    /// The wait ran out. `None` if it was already answered.
    pub fn expire(&mut self, id: u64) -> Option<Resolution> {
        self.pending.as_ref().filter(|p| p.id == id)?;
        self.pending = None;
        Some(Resolution::TimedOut)
    }

    /// Developer tools were switched off or access revoked: drop the question.
    pub fn cancel(&mut self) -> Option<u64> {
        self.pending.take().map(|p| p.id)
    }

    /// One switch was turned off: drop the question only if it's that switch's kind.
    pub fn cancel_if(&mut self, which: impl Fn(&Ask) -> bool) -> Option<u64> {
        if self.pending.as_ref().is_some_and(|p| which(&p.ask)) {
            self.cancel()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text() -> Ask {
        Ask::Text {
            to_name: "Sam".into(),
            to_address: "+15550100".into(),
            message: "On my way".into(),
        }
    }

    fn files() -> Ask {
        Ask::Files {
            files: vec![ConfirmFile {
                name: "report.pdf".into(),
                size: 2_100_000,
                folder: Some(r"C:\Users\me\Documents".into()),
            }],
        }
    }

    fn open(c: &mut Confirmations, now: i64) -> Result<ConfirmRequest, Busy> {
        c.open("claude-code".into(), text(), now)
    }

    #[test]
    fn a_file_card_is_answered_the_same_way() {
        let mut c = Confirmations::default();
        let r = c.open("Claude Code".into(), files(), 0).unwrap();
        // Busy for a text too: one card at a time, whatever it's about.
        assert_eq!(open(&mut c, 1), Err(Busy));
        assert_eq!(c.decide(r.id, false, 10), Some(Resolution::Declined));
        let r = c.open("Claude Code".into(), files(), 20).unwrap();
        assert_eq!(
            c.decide(r.id, true, CONFIRM_TIMEOUT_MS + 20),
            Some(Resolution::TimedOut)
        );
    }

    #[test]
    fn switching_one_kind_off_drops_only_its_card() {
        let mut c = Confirmations::default();
        let r = c.open("Claude Code".into(), files(), 0).unwrap();
        assert_eq!(
            c.cancel_if(|a| !a.is_files()),
            None,
            "the texts switch leaves a file card"
        );
        assert_eq!(c.cancel_if(Ask::is_files), Some(r.id));
        assert!(c.pending().is_none());
    }

    #[test]
    fn json_shapes_match_protocol_ts() {
        let mut c = Confirmations::default();
        let t = c.open("Claude Code (an AI tool)".into(), text(), 0).unwrap();
        assert_eq!(
            serde_json::to_value(&t).unwrap(),
            serde_json::json!({"id": 1, "tool": "Claude Code (an AI tool)", "kind": "text", "toName": "Sam",
                "toAddress": "+15550100", "message": "On my way", "expiresAt": CONFIRM_TIMEOUT_MS})
        );
        c.cancel();
        let f = c.open("tug command".into(), files(), 0).unwrap();
        assert_eq!(
            serde_json::to_value(&f).unwrap(),
            serde_json::json!({"id": 2, "tool": "tug command", "kind": "files",
                "files": [{"name": "report.pdf", "size": 2_100_000, "folder": "C:\\Users\\me\\Documents"}],
                "expiresAt": CONFIRM_TIMEOUT_MS})
        );
    }

    #[test]
    fn the_card_waits_as_long_as_the_bridge_expects() {
        assert_eq!(CONFIRM_TIMEOUT_MS as u64, tug_bridge::protocol::CONFIRM_SECS * 1000);
    }

    #[test]
    fn send_approves_once() {
        let mut c = Confirmations::default();
        let r = open(&mut c, 0).unwrap();
        assert_eq!(r.expires_at, CONFIRM_TIMEOUT_MS);
        assert_eq!(c.decide(r.id, true, 1_000), Some(Resolution::Approved));
        // A double click, or Don't send after Send, does nothing.
        assert_eq!(c.decide(r.id, true, 1_001), None);
        assert_eq!(c.decide(r.id, false, 1_002), None);
        assert_eq!(c.expire(r.id), None);
        assert!(c.pending().is_none());
    }

    #[test]
    fn dont_send_declines() {
        let mut c = Confirmations::default();
        let r = open(&mut c, 0).unwrap();
        assert_eq!(c.decide(r.id, false, 5), Some(Resolution::Declined));
    }

    #[test]
    fn one_question_at_a_time() {
        let mut c = Confirmations::default();
        let first = open(&mut c, 0).unwrap();
        assert_eq!(open(&mut c, 10), Err(Busy));
        assert_eq!(c.decide(first.id, false, 20), Some(Resolution::Declined));
        let second = open(&mut c, 30).unwrap();
        assert_ne!(first.id, second.id);
        // An answer meant for the old card can't approve the new one.
        assert_eq!(c.decide(first.id, true, 40), None);
        assert_eq!(c.pending().map(|p| p.id), Some(second.id));
    }

    #[test]
    fn it_times_out_as_not_sent() {
        let mut c = Confirmations::default();
        let r = open(&mut c, 0).unwrap();
        assert_eq!(c.expire(r.id), Some(Resolution::TimedOut));
        assert_eq!(c.decide(r.id, true, 1), None);
    }

    #[test]
    fn a_late_click_is_never_a_send() {
        let mut c = Confirmations::default();
        let r = open(&mut c, 0).unwrap();
        assert_eq!(c.decide(r.id, true, CONFIRM_TIMEOUT_MS), Some(Resolution::TimedOut));
    }

    #[test]
    fn an_expired_question_doesnt_block_the_next() {
        let mut c = Confirmations::default();
        let r = open(&mut c, 0).unwrap();
        let next = open(&mut c, CONFIRM_TIMEOUT_MS + 1).unwrap();
        assert_ne!(r.id, next.id);
        assert_eq!(c.decide(r.id, true, CONFIRM_TIMEOUT_MS + 2), None);
    }

    #[test]
    fn cancel_drops_the_question() {
        let mut c = Confirmations::default();
        let r = open(&mut c, 0).unwrap();
        assert_eq!(c.cancel(), Some(r.id));
        assert_eq!(c.decide(r.id, true, 1), None);
        assert_eq!(c.cancel(), None);
    }
}
