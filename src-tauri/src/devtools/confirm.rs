//! The "send this text?" confirmation: an AI tool (or `tug text`) can only ask; the person
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
    /// Who's asking, as it named itself ("claude-code", "tug command").
    pub tool: String,
    /// The contact's name (or the number, formatted, when it isn't a contact).
    pub to_name: String,
    /// The number it would go to.
    pub to_address: String,
    pub message: String,
    /// Unix ms after which it counts as not sent.
    pub expires_at: i64,
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
    pub fn open(
        &mut self,
        tool: String,
        to_name: String,
        to_address: String,
        message: String,
        now_ms: i64,
    ) -> Result<ConfirmRequest, Busy> {
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
            to_name,
            to_address,
            message,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(c: &mut Confirmations, now: i64) -> Result<ConfirmRequest, Busy> {
        c.open(
            "claude-code".into(),
            "Sam".into(),
            "+15550100".into(),
            "On my way".into(),
            now,
        )
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
