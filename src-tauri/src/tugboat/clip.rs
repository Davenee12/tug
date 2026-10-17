//! Shared clipboard over Tugboat: the rules for sending what's on the PC's clipboard to the phone's
//! page. Pure (plain values in, plain values out), so the integration test and the unit tests below
//! cover every state without a phone or a real clipboard.
//!
//! - Only when the person asks ("Send clipboard to iPhone"): one press, one clipboard. A newer
//!   press replaces one the phone hasn't taken yet; nothing is ever queued behind it.
//! - The phone gets it at its next check-in and says so (`POST /api/clip`). Only then is it
//!   "delivered", and the PC lets go of the contents at once (the page keeps its own copy).
//! - If no phone takes it within `WINDOW` (2 minutes) it's dropped, never kept waiting longer.
//! - What the panel sees is the kind and the state, never the contents.

use std::time::{Duration, Instant};

use serde::Serialize;

/// A clipboard the phone hasn't taken within this long is dropped.
pub const WINDOW: Duration = Duration::from_secs(120);
/// A clipboard handed to the page just before the window closed may still be acknowledged for
/// this much longer (the reply can be in flight), so the phone showing it and the PC saying "not
/// sent" can't both happen.
pub const ACK_GRACE: Duration = Duration::from_secs(10);
/// Largest picture sent from the clipboard, once encoded as PNG. The page holds it in memory.
pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
/// Largest picture by pixels: 24 megapixels, room for a 6K screen or two 4K screens side by side.
/// The bitmap is read off the clipboard whole (4 bytes a pixel), decoded and PNG-encoded in
/// memory, so this bounds what one press can make tug hold.
pub const MAX_IMAGE_PIXELS: u64 = 24_000_000;
/// The file name the phone sees for a picture from the clipboard.
pub const IMAGE_NAME: &str = "Clipboard picture.png";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipKind {
    Text,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipState {
    /// Sent from the PC; the phone hasn't taken it yet.
    Waiting,
    /// The phone has it.
    Delivered,
    /// No phone took it within the window, so it was dropped.
    Expired,
}

/// The last clipboard sent, as the Tugboat panel shows it (never its contents). Mirrored in
/// `src/types/protocol.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipStatus {
    pub id: u32,
    pub kind: ClipKind,
    pub state: ClipState,
    /// When a waiting clipboard is dropped (Unix ms), for the panel's "time left".
    pub expires_at: i64,
}

/// "tugboat-clip": a clipboard reached the phone, or was dropped. Mirrored in `protocol.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipEvent {
    pub kind: ClipKind,
    pub state: ClipState,
}

/// What a waiting clipboard carries. A picture lives in the session's offers (so the phone fetches
/// it chunk by chunk, sealed, like any file from the PC); this names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    Text(String),
    Image { offer: String },
}

impl Payload {
    pub fn kind(&self) -> ClipKind {
        match self {
            Payload::Text(_) => ClipKind::Text,
            Payload::Image { .. } => ClipKind::Image,
        }
    }

    /// The offer to withdraw once this payload is done with.
    pub fn offer(&self) -> Option<&str> {
        match self {
            Payload::Image { offer } => Some(offer),
            Payload::Text(_) => None,
        }
    }
}

struct Waiting {
    id: u32,
    payload: Payload,
    sent: Instant,
    expires_at: i64,
    /// The page has been handed it at least once.
    served: bool,
}

enum Entry {
    Waiting(Waiting),
    /// Delivered or expired: only the record that it happened is kept, not the contents.
    Done {
        id: u32,
        kind: ClipKind,
        state: ClipState,
        expires_at: i64,
    },
}

/// The answer to the phone's acknowledgement.
#[derive(Debug, PartialEq, Eq)]
pub enum Ack {
    /// Newly delivered: the contents were let go of (and the offer to withdraw, for a picture).
    Delivered { kind: ClipKind, freed: Option<String> },
    /// Already delivered (the phone retried after losing the reply).
    Already,
    /// Not the clipboard on its way now (replaced, dropped, or never sent).
    Unknown,
}

/// The one clipboard on its way to the phone, at most.
#[derive(Default)]
pub struct Outbox {
    current: Option<Entry>,
}

impl Outbox {
    pub fn new() -> Outbox {
        Outbox::default()
    }

    /// Whether the waiting clipboard was handed to a page (tests check who it was handed to).
    #[cfg(test)]
    pub fn served(&self) -> bool {
        matches!(&self.current, Some(Entry::Waiting(w)) if w.served)
    }

    /// A new clipboard to send. Returns the one it replaces, if the phone hadn't taken it yet (its
    /// picture offer, if any, is withdrawn by the caller).
    pub fn send(&mut self, id: u32, payload: Payload, now: Instant, now_ms: i64) -> Option<Payload> {
        let expires_at = now_ms + WINDOW.as_millis() as i64;
        let previous = self.current.replace(Entry::Waiting(Waiting {
            id,
            payload,
            sent: now,
            expires_at,
            served: false,
        }));
        match previous {
            Some(Entry::Waiting(w)) => Some(w.payload),
            _ => None,
        }
    }

    /// What the page gets at a check-in: the waiting clipboard, if its window is still open.
    pub fn for_page(&mut self, now: Instant) -> Option<(u32, &Payload)> {
        match &mut self.current {
            Some(Entry::Waiting(w)) if now.duration_since(w.sent) < WINDOW => {
                w.served = true;
                Some((w.id, &w.payload))
            }
            _ => None,
        }
    }

    /// The phone says it has clipboard `id`.
    pub fn ack(&mut self, id: u32, now: Instant) -> Ack {
        match &self.current {
            Some(Entry::Waiting(w)) if w.id == id => {
                let age = now.duration_since(w.sent);
                if age >= WINDOW && !(w.served && age < WINDOW + ACK_GRACE) {
                    return Ack::Unknown;
                }
            }
            Some(Entry::Done {
                id: done,
                state: ClipState::Delivered,
                ..
            }) if *done == id => return Ack::Already,
            _ => return Ack::Unknown,
        }
        let Some(Entry::Waiting(w)) = self.current.take() else {
            return Ack::Unknown;
        };
        let kind = w.payload.kind();
        let freed = w.payload.offer().map(str::to_string);
        self.current = Some(Entry::Done {
            id: w.id,
            kind,
            state: ClipState::Delivered,
            expires_at: w.expires_at,
        });
        Ack::Delivered { kind, freed }
    }

    /// Drop a clipboard no phone took in time. Returns it (to log and withdraw its offer) when it
    /// was dropped just now. One the page was handed late gets `ACK_GRACE` to say so first.
    pub fn tick(&mut self, now: Instant) -> Option<Payload> {
        let due = match &self.current {
            Some(Entry::Waiting(w)) => {
                let age = now.duration_since(w.sent);
                let limit = if w.served { WINDOW + ACK_GRACE } else { WINDOW };
                age >= limit
            }
            _ => false,
        };
        if !due {
            return None;
        }
        let Some(Entry::Waiting(w)) = self.current.take() else {
            return None;
        };
        self.current = Some(Entry::Done {
            id: w.id,
            kind: w.payload.kind(),
            state: ClipState::Expired,
            expires_at: w.expires_at,
        });
        Some(w.payload)
    }

    /// A waiting picture: the clipboard's id and its offer (the PC checks the phone really fetched
    /// it before taking its word that it has it).
    pub fn waiting_offer(&self) -> Option<(u32, &str)> {
        match &self.current {
            Some(Entry::Waiting(w)) => w.payload.offer().map(|o| (w.id, o)),
            _ => None,
        }
    }

    pub fn status(&self) -> Option<ClipStatus> {
        self.current.as_ref().map(|e| match e {
            Entry::Waiting(w) => ClipStatus {
                id: w.id,
                kind: w.payload.kind(),
                state: ClipState::Waiting,
                expires_at: w.expires_at,
            },
            Entry::Done {
                id,
                kind,
                state,
                expires_at,
            } => ClipStatus {
                id: *id,
                kind: *kind,
                state: *state,
                expires_at: *expires_at,
            },
        })
    }
}

/// What was on the PC's clipboard when the person pressed Send (see `clipboard::read_for_sharing`).
#[derive(Debug, PartialEq, Eq)]
pub enum Reading {
    Text(String),
    /// More text than Tugboat sends at once (it wasn't read in full).
    TooLong,
    /// A picture, as the clipboard's device-independent bitmap (`CF_DIB`).
    Image(Vec<u8>),
    /// A picture bigger than Tugboat sends (it wasn't read).
    ImageTooBig,
    /// Nothing at all.
    Empty,
    /// Something else: files copied in Explorer, say.
    Other,
}

/// A clipboard ready to send: text, or a picture already encoded as PNG.
#[derive(Debug, PartialEq, Eq)]
pub enum Prepared {
    Text(String),
    Image(Vec<u8>),
}

/// Why a clipboard can't be sent, in the words the person sees.
pub fn refuse(why: Refused) -> &'static str {
    match why {
        Refused::Empty => "Your clipboard is empty. Copy some text or a picture first.",
        Refused::TooLong => "That's too much text to send at once.",
        Refused::TooBig => "That picture is too big to send.",
        Refused::Other => "Only text and pictures can be sent from your clipboard. To send files, drag them onto tug.",
        Refused::BadImage => "tug couldn't read the picture on your clipboard.",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    Empty,
    TooLong,
    TooBig,
    Other,
    BadImage,
}

/// Decide what to send from what the clipboard held: text as it is (blank text counts as
/// nothing), a picture converted to PNG within the size limits.
pub fn prepare(reading: Reading, max_text: usize) -> Result<Prepared, Refused> {
    match reading {
        Reading::Text(t) if t.trim().is_empty() => Err(Refused::Empty),
        Reading::Text(t) if t.len() > max_text => Err(Refused::TooLong),
        Reading::Text(t) => Ok(Prepared::Text(t)),
        Reading::TooLong => Err(Refused::TooLong),
        Reading::Image(dib) => {
            let png = super::dib::to_png(&dib, MAX_IMAGE_PIXELS).map_err(|e| match e {
                super::dib::DibError::TooBig => Refused::TooBig,
                _ => Refused::BadImage,
            })?;
            if png.len() > MAX_IMAGE_BYTES {
                return Err(Refused::TooBig);
            }
            Ok(Prepared::Image(png))
        }
        Reading::ImageTooBig => Err(Refused::TooBig),
        Reading::Empty => Err(Refused::Empty),
        Reading::Other => Err(Refused::Other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_790_000_000_000;

    fn text(s: &str) -> Payload {
        Payload::Text(s.to_string())
    }

    #[test]
    fn waits_for_the_phone_then_lets_go_of_the_contents_once_delivered() {
        let start = Instant::now();
        let mut out = Outbox::new();
        assert!(out.status().is_none());
        assert_eq!(out.send(1, text("hello"), start, T0), None);
        let st = out.status().unwrap();
        assert_eq!((st.id, st.kind, st.state), (1, ClipKind::Text, ClipState::Waiting));
        assert_eq!(st.expires_at, T0 + 120_000);

        // The page picks it up at its next check-in, and says it has it.
        let (id, payload) = out.for_page(start + Duration::from_secs(5)).unwrap();
        assert_eq!((id, payload), (1, &text("hello")));
        assert_eq!(
            out.ack(1, start + Duration::from_secs(6)),
            Ack::Delivered {
                kind: ClipKind::Text,
                freed: None
            }
        );
        assert_eq!(out.status().unwrap().state, ClipState::Delivered);
        // The PC no longer has it to hand out.
        assert!(out.for_page(start + Duration::from_secs(7)).is_none());
        // A retried acknowledgement (the reply was lost) is fine; a stranger id isn't.
        assert_eq!(out.ack(1, start + Duration::from_secs(8)), Ack::Already);
        assert_eq!(out.ack(2, start + Duration::from_secs(8)), Ack::Unknown);
        // Nothing left to expire.
        assert_eq!(out.tick(start + WINDOW * 2), None);
        assert_eq!(out.status().unwrap().state, ClipState::Delivered);
    }

    #[test]
    fn nothing_is_kept_waiting_past_two_minutes() {
        let start = Instant::now();
        let mut out = Outbox::new();
        out.send(1, text("secret-ish"), start, T0);
        assert_eq!(out.tick(start + WINDOW - Duration::from_millis(1)), None);
        // The page can't be handed it after the window, even before the tick has run.
        assert!(out.for_page(start + WINDOW).is_none());
        assert_eq!(out.tick(start + WINDOW), Some(text("secret-ish")));
        assert_eq!(out.status().unwrap().state, ClipState::Expired);
        // A late acknowledgement for a clipboard the page was never handed doesn't count.
        assert_eq!(out.ack(1, start + WINDOW + Duration::from_secs(1)), Ack::Unknown);
        assert!(out.for_page(start + WINDOW).is_none());
        assert_eq!(out.tick(start + WINDOW * 3), None, "dropped once");
    }

    #[test]
    fn a_clipboard_handed_over_at_the_last_moment_can_still_be_acknowledged() {
        let start = Instant::now();
        let mut out = Outbox::new();
        out.send(4, text("just in time"), start, T0);
        assert!(out.for_page(start + WINDOW - Duration::from_millis(100)).is_some());
        // The tick waits for the reply rather than calling it lost...
        assert_eq!(out.tick(start + WINDOW + Duration::from_secs(1)), None);
        assert_eq!(
            out.ack(4, start + WINDOW + Duration::from_secs(2)),
            Ack::Delivered {
                kind: ClipKind::Text,
                freed: None
            }
        );
        // ...but not for ever.
        let mut late = Outbox::new();
        late.send(5, text("gone"), start, T0);
        late.for_page(start + Duration::from_secs(119));
        assert_eq!(late.ack(5, start + WINDOW + ACK_GRACE), Ack::Unknown);
        assert_eq!(late.tick(start + WINDOW + ACK_GRACE), Some(text("gone")));
    }

    #[test]
    fn a_newer_press_replaces_one_still_waiting() {
        let start = Instant::now();
        let mut out = Outbox::new();
        out.send(1, Payload::Image { offer: "offerA".into() }, start, T0);
        assert_eq!(out.waiting_offer(), Some((1, "offerA")));
        let replaced = out.send(2, text("newer"), start + Duration::from_secs(3), T0 + 3000);
        assert_eq!(replaced, Some(Payload::Image { offer: "offerA".into() }));
        assert_eq!(out.waiting_offer(), None);
        assert_eq!(
            out.ack(1, start + Duration::from_secs(4)),
            Ack::Unknown,
            "the old one is gone"
        );
        assert_eq!(out.for_page(start + Duration::from_secs(4)).unwrap().0, 2);
        // A press after delivery replaces nothing.
        out.ack(2, start + Duration::from_secs(5));
        assert_eq!(out.send(3, text("third"), start + Duration::from_secs(6), T0), None);
    }

    #[test]
    fn a_picture_frees_its_offer_when_delivered_or_dropped() {
        let start = Instant::now();
        let mut out = Outbox::new();
        let pic = Payload::Image { offer: "offerB".into() };
        out.send(7, pic.clone(), start, T0);
        out.for_page(start);
        assert_eq!(
            out.ack(7, start + Duration::from_secs(1)),
            Ack::Delivered {
                kind: ClipKind::Image,
                freed: Some("offerB".into())
            }
        );
        let mut dropped = Outbox::new();
        dropped.send(8, pic.clone(), start, T0);
        assert_eq!(
            dropped.tick(start + WINDOW).and_then(|p| p.offer().map(String::from)),
            Some("offerB".into())
        );
    }

    #[test]
    fn prepares_text_and_refuses_what_it_cant_send() {
        assert_eq!(
            prepare(Reading::Text("hi there".into()), 100),
            Ok(Prepared::Text("hi there".into()))
        );
        assert_eq!(prepare(Reading::Text("  \r\n\t".into()), 100), Err(Refused::Empty));
        assert_eq!(prepare(Reading::Text("x".repeat(101)), 100), Err(Refused::TooLong));
        assert_eq!(prepare(Reading::TooLong, 100), Err(Refused::TooLong));
        assert_eq!(prepare(Reading::Empty, 100), Err(Refused::Empty));
        assert_eq!(prepare(Reading::Other, 100), Err(Refused::Other));
        assert_eq!(prepare(Reading::ImageTooBig, 100), Err(Refused::TooBig));
        assert_eq!(prepare(Reading::Image(vec![1, 2, 3]), 100), Err(Refused::BadImage));
        // Every refusal has words for the person.
        for r in [
            Refused::Empty,
            Refused::TooLong,
            Refused::TooBig,
            Refused::Other,
            Refused::BadImage,
        ] {
            assert!(!refuse(r).is_empty());
        }
    }

    #[test]
    fn prepares_a_picture_as_png() {
        let dib = super::super::dib::tests::dib_24(2, 2, false);
        match prepare(Reading::Image(dib), 100).unwrap() {
            Prepared::Image(png) => assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n"),
            other => panic!("expected a picture, got {other:?}"),
        }
    }

    #[test]
    fn refuses_a_picture_over_24_megapixels_before_reading_it() {
        use super::super::dib::tests::header;
        // Only the header: the size alone decides, nothing is allocated for the pixels.
        let over = header(40, 5000, 5000, 24, 0);
        assert!(matches!(prepare(Reading::Image(over), 100), Err(Refused::TooBig)));
        // Exactly at the limit is let through to the bitmap check (this one has no pixels).
        let at = header(40, 6000, 4000, 24, 0);
        assert!(matches!(prepare(Reading::Image(at), 100), Err(Refused::BadImage)));
    }

    #[test]
    fn json_shapes_match_protocol_ts() {
        let st = ClipStatus {
            id: 3,
            kind: ClipKind::Image,
            state: ClipState::Waiting,
            expires_at: 5,
        };
        assert_eq!(
            serde_json::to_string(&st).unwrap(),
            r#"{"id":3,"kind":"image","state":"waiting","expiresAt":5}"#
        );
        let ev = ClipEvent {
            kind: ClipKind::Text,
            state: ClipState::Delivered,
        };
        assert_eq!(
            serde_json::to_string(&ev).unwrap(),
            r#"{"kind":"text","state":"delivered"}"#
        );
    }
}
