//! Apple Notification Center Service (ANCS) wire protocol.
//!
//! Spec: https://developer.apple.com/library/archive/documentation/CoreBluetooth/Reference/AppleNotificationCenterServiceSpecification/
//!
//! Pure encode/decode only — no Bluetooth I/O — so it is unit-tested on any OS.
//! The iPhone is the Notification Provider; tug is the Notification Consumer.

use serde::Serialize;
use thiserror::Error;

pub const SERVICE: u128 = 0x7905F431_B5CE_4E99_A40F_4B1E122D00D0;
pub const NOTIFICATION_SOURCE: u128 = 0x9FBF120D_6301_42D9_8C58_25E699A21DBD;
pub const CONTROL_POINT: u128 = 0x69D1D8F3_45E1_49A8_9821_9BBDFDAAD9D9;
pub const DATA_SOURCE: u128 = 0x22EAC6E9_24D6_4BB5_BE44_B36ACE7C7BFB;

/// Control Point error codes returned as ATT protocol errors on write.
pub const ERR_UNKNOWN_COMMAND: u8 = 0xA0;
pub const ERR_INVALID_COMMAND: u8 = 0xA1;
pub const ERR_INVALID_PARAMETER: u8 = 0xA2;
pub const ERR_ACTION_FAILED: u8 = 0xA3;

const CMD_GET_NOTIFICATION_ATTRIBUTES: u8 = 0;
const CMD_GET_APP_ATTRIBUTES: u8 = 1;
const CMD_PERFORM_NOTIFICATION_ACTION: u8 = 2;

const ATTR_APP_IDENTIFIER: u8 = 0;
const ATTR_TITLE: u8 = 1;
const ATTR_SUBTITLE: u8 = 2;
const ATTR_MESSAGE: u8 = 3;
const ATTR_DATE: u8 = 5;
const ATTR_POSITIVE_ACTION_LABEL: u8 = 6;
const ATTR_NEGATIVE_ACTION_LABEL: u8 = 7;

const APP_ATTR_DISPLAY_NAME: u8 = 0;

/// Max bytes requested for the length-limited attributes. iOS truncates to these.
const TITLE_MAX: u16 = 128;
const SUBTITLE_MAX: u16 = 128;
const MESSAGE_MAX: u16 = 2048;

/// Attributes requested for every notification, in request order.
const REQUESTED_ATTRS: [u8; 7] = [
    ATTR_APP_IDENTIFIER,
    ATTR_TITLE,
    ATTR_SUBTITLE,
    ATTR_MESSAGE,
    ATTR_DATE,
    ATTR_POSITIVE_ACTION_LABEL,
    ATTR_NEGATIVE_ACTION_LABEL,
];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("packet too short: expected {expected} bytes, got {got}")]
    TooShort { expected: usize, got: usize },
    #[error("unknown event id {0}")]
    UnknownEvent(u8),
    #[error("data source sent command {got}, expected {expected}")]
    UnexpectedCommand { expected: u8, got: u8 },
    #[error("data source answered for notification {got}, expected {expected}")]
    UidMismatch { expected: u32, got: u32 },
    #[error("data source sent data nobody asked for")]
    Unsolicited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventId {
    Added,
    Modified,
    Removed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    #[default]
    Other,
    IncomingCall,
    MissedCall,
    Voicemail,
    Social,
    Schedule,
    Email,
    News,
    HealthAndFitness,
    BusinessAndFinance,
    Location,
    Entertainment,
}

impl Category {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::IncomingCall,
            2 => Self::MissedCall,
            3 => Self::Voicemail,
            4 => Self::Social,
            5 => Self::Schedule,
            6 => Self::Email,
            7 => Self::News,
            8 => Self::HealthAndFitness,
            9 => Self::BusinessAndFinance,
            10 => Self::Location,
            11 => Self::Entertainment,
            _ => Self::Other,
        }
    }

    /// Stable string used in the database and the frontend.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Other => "other",
            Self::IncomingCall => "incomingCall",
            Self::MissedCall => "missedCall",
            Self::Voicemail => "voicemail",
            Self::Social => "social",
            Self::Schedule => "schedule",
            Self::Email => "email",
            Self::News => "news",
            Self::HealthAndFitness => "healthAndFitness",
            Self::BusinessAndFinance => "businessAndFinance",
            Self::Location => "location",
            Self::Entertainment => "entertainment",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventFlags {
    pub silent: bool,
    pub important: bool,
    pub pre_existing: bool,
    pub positive_action: bool,
    pub negative_action: bool,
}

impl EventFlags {
    pub fn from_bits(b: u8) -> Self {
        Self {
            silent: b & 0x01 != 0,
            important: b & 0x02 != 0,
            pre_existing: b & 0x04 != 0,
            positive_action: b & 0x08 != 0,
            negative_action: b & 0x10 != 0,
        }
    }

    pub fn bits(self) -> u8 {
        (self.silent as u8)
            | (self.important as u8) << 1
            | (self.pre_existing as u8) << 2
            | (self.positive_action as u8) << 3
            | (self.negative_action as u8) << 4
    }
}

/// One 8-byte packet from the Notification Source characteristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationEvent {
    pub event: EventId,
    pub flags: EventFlags,
    pub category: Category,
    pub category_count: u8,
    pub uid: u32,
}

pub fn parse_notification_source(b: &[u8]) -> Result<NotificationEvent, ParseError> {
    if b.len() < 8 {
        return Err(ParseError::TooShort {
            expected: 8,
            got: b.len(),
        });
    }
    let event = match b[0] {
        0 => EventId::Added,
        1 => EventId::Modified,
        2 => EventId::Removed,
        other => return Err(ParseError::UnknownEvent(other)),
    };
    Ok(NotificationEvent {
        event,
        flags: EventFlags::from_bits(b[1]),
        category: Category::from_u8(b[2]),
        category_count: b[3],
        uid: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
    })
}

/// Control Point: Get Notification Attributes for `uid`.
pub fn get_notification_attributes(uid: u32) -> Vec<u8> {
    let mut out = vec![CMD_GET_NOTIFICATION_ATTRIBUTES];
    out.extend_from_slice(&uid.to_le_bytes());
    for attr in REQUESTED_ATTRS {
        out.push(attr);
        let max = match attr {
            ATTR_TITLE => Some(TITLE_MAX),
            ATTR_SUBTITLE => Some(SUBTITLE_MAX),
            ATTR_MESSAGE => Some(MESSAGE_MAX),
            _ => None,
        };
        if let Some(max) = max {
            out.extend_from_slice(&max.to_le_bytes());
        }
    }
    out
}

/// Control Point probe: attributes for a UID that never exists. An authorised
/// consumer gets `ERR_INVALID_PARAMETER`; when iOS isn't sharing notifications
/// with this accessory it refuses the write with ATT 0x03 (Write Not Permitted)
/// even though the CCCD subscriptions succeeded.
pub fn probe() -> Vec<u8> {
    get_notification_attributes(u32::MAX)
}

/// ATT error iOS returns on Control Point writes when notification sharing is off.
pub const ATT_WRITE_NOT_PERMITTED: u8 = 0x03;

/// Control Point: Get App Attributes (display name) for a bundle id.
pub fn get_app_attributes(app_id: &str) -> Vec<u8> {
    let mut out = vec![CMD_GET_APP_ATTRIBUTES];
    out.extend_from_slice(app_id.as_bytes());
    out.push(0);
    out.push(APP_ATTR_DISPLAY_NAME);
    out
}

/// Control Point: Perform Notification Action. Positive is e.g. "Answer",
/// negative is e.g. "Decline" / "Clear" — labels come from the notification.
pub fn perform_action(uid: u32, positive: bool) -> Vec<u8> {
    let mut out = vec![CMD_PERFORM_NOTIFICATION_ACTION];
    out.extend_from_slice(&uid.to_le_bytes());
    out.push(if positive { 0 } else { 1 });
    out
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationAttributes {
    pub app_id: String,
    pub title: String,
    pub subtitle: String,
    pub message: String,
    /// ISO-8601 local time without zone, converted from ANCS `yyyyMMdd'T'HHmmSS`.
    pub date: Option<String>,
    pub positive_label: String,
    pub negative_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    Notification {
        uid: u32,
        attrs: NotificationAttributes,
    },
    App {
        app_id: String,
        display_name: Option<String>,
    },
}

#[derive(Debug, Clone)]
enum Expected {
    Notification { uid: u32 },
    App { app_id: String },
}

/// Reassembles Data Source responses, which iOS fragments across as many
/// GATT notifications as the MTU requires. Exactly one request may be in
/// flight at a time; the caller serialises Control Point writes.
#[derive(Debug, Default)]
pub struct Reassembler {
    buf: Vec<u8>,
    expected: Option<Expected>,
}

impl Reassembler {
    pub fn expect_notification(&mut self, uid: u32) {
        self.buf.clear();
        self.expected = Some(Expected::Notification { uid });
    }

    pub fn expect_app(&mut self, app_id: &str) {
        self.buf.clear();
        self.expected = Some(Expected::App {
            app_id: app_id.to_owned(),
        });
    }

    pub fn reset(&mut self) {
        self.buf.clear();
        self.expected = None;
    }

    /// Feed one Data Source chunk. `Ok(None)` means more fragments are needed.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Option<Response>, ParseError> {
        let Some(expected) = self.expected.clone() else {
            return Err(ParseError::Unsolicited);
        };
        self.buf.extend_from_slice(chunk);
        let result = match expected {
            Expected::Notification { uid } => self.try_notification(uid),
            Expected::App { app_id } => self.try_app(&app_id),
        };
        match &result {
            Ok(Some(_)) | Err(_) => self.reset(),
            Ok(None) => {}
        }
        result
    }

    fn try_notification(&self, expected_uid: u32) -> Result<Option<Response>, ParseError> {
        let b = &self.buf;
        if b.len() < 5 {
            return Ok(None);
        }
        if b[0] != CMD_GET_NOTIFICATION_ATTRIBUTES {
            return Err(ParseError::UnexpectedCommand {
                expected: CMD_GET_NOTIFICATION_ATTRIBUTES,
                got: b[0],
            });
        }
        let uid = u32::from_le_bytes([b[1], b[2], b[3], b[4]]);
        if uid != expected_uid {
            return Err(ParseError::UidMismatch {
                expected: expected_uid,
                got: uid,
            });
        }
        let Some(tuples) = read_tuples(&b[5..], REQUESTED_ATTRS.len()) else {
            return Ok(None);
        };
        let mut attrs = NotificationAttributes::default();
        for (id, value) in tuples {
            let text = String::from_utf8_lossy(value).into_owned();
            match id {
                ATTR_APP_IDENTIFIER => attrs.app_id = text,
                ATTR_TITLE => attrs.title = text,
                ATTR_SUBTITLE => attrs.subtitle = text,
                ATTR_MESSAGE => attrs.message = text,
                ATTR_DATE => attrs.date = ancs_date_to_iso(&text),
                ATTR_POSITIVE_ACTION_LABEL => attrs.positive_label = text,
                ATTR_NEGATIVE_ACTION_LABEL => attrs.negative_label = text,
                _ => {}
            }
        }
        Ok(Some(Response::Notification { uid, attrs }))
    }

    fn try_app(&self, expected_app: &str) -> Result<Option<Response>, ParseError> {
        let b = &self.buf;
        if b.is_empty() {
            return Ok(None);
        }
        if b[0] != CMD_GET_APP_ATTRIBUTES {
            return Err(ParseError::UnexpectedCommand {
                expected: CMD_GET_APP_ATTRIBUTES,
                got: b[0],
            });
        }
        let Some(nul) = b[1..].iter().position(|&c| c == 0) else {
            return Ok(None);
        };
        let app_id = String::from_utf8_lossy(&b[1..1 + nul]).into_owned();
        if app_id != expected_app {
            // iOS echoes the id we sent; anything else means the stream is out of sync.
            return Err(ParseError::Unsolicited);
        }
        let Some(tuples) = read_tuples(&b[2 + nul..], 1) else {
            return Ok(None);
        };
        let display_name = tuples
            .into_iter()
            .find(|(id, _)| *id == APP_ATTR_DISPLAY_NAME)
            .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
            .filter(|s| !s.is_empty());
        Ok(Some(Response::App { app_id, display_name }))
    }
}

/// Reads `count` `[id:u8][len:u16 LE][value]` tuples, or `None` if incomplete.
fn read_tuples(mut b: &[u8], count: usize) -> Option<Vec<(u8, &[u8])>> {
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if b.len() < 3 {
            return None;
        }
        let id = b[0];
        let len = u16::from_le_bytes([b[1], b[2]]) as usize;
        if b.len() < 3 + len {
            return None;
        }
        out.push((id, &b[3..3 + len]));
        b = &b[3 + len..];
    }
    Some(out)
}

/// If `chunk` is the start of a notification-attributes response, the UID it's for.
/// Used to recognise a late reply to an earlier request, whatever was expected.
pub fn notification_response_uid(chunk: &[u8]) -> Option<u32> {
    (chunk.len() >= 5 && chunk[0] == CMD_GET_NOTIFICATION_ATTRIBUTES)
        .then(|| u32::from_le_bytes([chunk[1], chunk[2], chunk[3], chunk[4]]))
}

/// `20261004T153012` → `2026-10-04T15:30:12`.
pub fn ancs_date_to_iso(s: &str) -> Option<String> {
    let b = s.as_bytes();
    if b.len() != 15 || b[8] != b'T' || !s[..8].bytes().chain(s[9..].bytes()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!(
        "{}-{}-{}T{}:{}:{}",
        &s[0..4],
        &s[4..6],
        &s[6..8],
        &s[9..11],
        &s[11..13],
        &s[13..15]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuple(id: u8, v: &str) -> Vec<u8> {
        let mut out = vec![id];
        out.extend_from_slice(&(v.len() as u16).to_le_bytes());
        out.extend_from_slice(v.as_bytes());
        out
    }

    fn notification_response(uid: u32) -> Vec<u8> {
        let mut out = vec![0];
        out.extend_from_slice(&uid.to_le_bytes());
        for (id, v) in [
            (0, "com.apple.MobileSMS"),
            (1, "Jane Doe"),
            (2, ""),
            (3, "Are we still meeting at 5?"),
            (5, "20261004T153012"),
            (6, ""),
            (7, "Clear"),
        ] {
            out.extend(tuple(id, v));
        }
        out
    }

    #[test]
    fn parses_notification_source() {
        let ev = parse_notification_source(&[0, 0b0001_0100, 4, 3, 0x2A, 0, 0, 0]).unwrap();
        assert_eq!(ev.event, EventId::Added);
        assert!(ev.flags.pre_existing && ev.flags.negative_action && !ev.flags.silent);
        assert_eq!(ev.category, Category::Social);
        assert_eq!(ev.category_count, 3);
        assert_eq!(ev.uid, 42);
    }

    #[test]
    fn rejects_short_and_unknown_events() {
        assert_eq!(
            parse_notification_source(&[0, 0]),
            Err(ParseError::TooShort { expected: 8, got: 2 })
        );
        assert_eq!(
            parse_notification_source(&[9, 0, 0, 0, 0, 0, 0, 0]),
            Err(ParseError::UnknownEvent(9))
        );
    }

    #[test]
    fn flags_round_trip() {
        for b in 0..32u8 {
            assert_eq!(EventFlags::from_bits(b).bits(), b);
        }
    }

    #[test]
    fn encodes_get_notification_attributes() {
        let req = get_notification_attributes(0x01020304);
        assert_eq!(&req[..5], &[0, 4, 3, 2, 1]);
        // AppId, Title+max, Subtitle+max, Message+max, Date, Pos, Neg
        assert_eq!(&req[5..], &[0, 1, 128, 0, 2, 128, 0, 3, 0, 8, 5, 6, 7]);
    }

    #[test]
    fn probe_targets_a_uid_ios_never_issues() {
        let p = probe();
        assert_eq!(&p[..5], &[0, 0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(
            &p[5..],
            &get_notification_attributes(0)[5..],
            "same attribute list as a real request"
        );
    }

    #[test]
    fn encodes_app_attributes_and_actions() {
        assert_eq!(get_app_attributes("a.b"), vec![1, b'a', b'.', b'b', 0, 0]);
        assert_eq!(perform_action(7, true), vec![2, 7, 0, 0, 0, 0]);
        assert_eq!(perform_action(7, false), vec![2, 7, 0, 0, 0, 1]);
    }

    #[test]
    fn reassembles_fragmented_notification() {
        let full = notification_response(42);
        let mut r = Reassembler::default();
        r.expect_notification(42);
        // Typical 20-byte ATT payloads on a default MTU.
        let mut result = None;
        for chunk in full.chunks(20) {
            assert!(result.is_none(), "completed before the last fragment");
            result = r.push(chunk).unwrap();
        }
        let Some(Response::Notification { uid, attrs }) = result else {
            panic!("no response")
        };
        assert_eq!(uid, 42);
        assert_eq!(attrs.app_id, "com.apple.MobileSMS");
        assert_eq!(attrs.title, "Jane Doe");
        assert_eq!(attrs.message, "Are we still meeting at 5?");
        assert_eq!(attrs.date.as_deref(), Some("2026-10-04T15:30:12"));
        assert_eq!(attrs.negative_label, "Clear");
        assert_eq!(
            r.push(&[0]),
            Err(ParseError::Unsolicited),
            "reassembler resets after completion"
        );
    }

    #[test]
    fn detects_uid_mismatch() {
        let mut r = Reassembler::default();
        r.expect_notification(1);
        assert_eq!(
            r.push(&notification_response(2)),
            Err(ParseError::UidMismatch { expected: 1, got: 2 })
        );
    }

    #[test]
    fn reassembles_app_attributes() {
        let mut resp = vec![1];
        resp.extend_from_slice(b"net.whatsapp.WhatsApp\0");
        resp.extend(tuple(0, "WhatsApp"));
        let mut r = Reassembler::default();
        r.expect_app("net.whatsapp.WhatsApp");
        assert_eq!(r.push(&resp[..10]).unwrap(), None);
        assert_eq!(
            r.push(&resp[10..]).unwrap(),
            Some(Response::App {
                app_id: "net.whatsapp.WhatsApp".into(),
                display_name: Some("WhatsApp".into())
            })
        );
    }

    #[test]
    fn recognises_the_start_of_a_notification_response() {
        assert_eq!(notification_response_uid(&notification_response(42)), Some(42));
        assert_eq!(notification_response_uid(&[1, b'a', 0]), None, "app response");
        assert_eq!(notification_response_uid(&[0, 1, 0]), None, "too short to name a UID");
    }

    #[test]
    fn converts_dates() {
        assert_eq!(
            ancs_date_to_iso("20261004T153012").as_deref(),
            Some("2026-10-04T15:30:12")
        );
        assert_eq!(ancs_date_to_iso("2026-10-04"), None);
        assert_eq!(ancs_date_to_iso("2026100XT153012"), None);
    }
}
