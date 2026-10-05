//! MAP event reports (MAP 1.4 §3.1.7): what the phone PUTs to our Message Notification
//! Server (MNS) once `SetNotificationRegistration` has turned notifications on.
//!
//! Each report carries exactly one self-closing `<event …/>` element, e.g.
//! `<MAP-event-report version="1.0"><event type="NewMessage" handle="…" folder="TELECOM/MSG/INBOX"
//! msg_type="SMS_GSM"/></MAP-event-report>`, so the listing's attribute scanner is enough.
//! Pure; the MNS server that receives these (`map::mns`) feeds the parsed events to the worker.

use thiserror::Error;

use super::listing;

/// Message Notification Server service class (SDP), which tug would advertise.
pub const MNS_UUID: u128 = 0x0000_1133_0000_1000_8000_0080_5F9B_34FB;
/// OBEX Target the phone sends when it connects to our MNS.
pub const MNS_TARGET: [u8; 16] = [
    0xBB, 0x58, 0x2B, 0x41, 0x42, 0x0C, 0x11, 0xDB, 0xB0, 0xDE, 0x08, 0x00, 0x20, 0x0C, 0x9A, 0x66,
];
/// OBEX Type of the SendEvent PUT that carries a report.
pub const EVENT_REPORT_TYPE: &str = "x-bt/MAP-event-report";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    NewMessage,
    DeliverySuccess,
    SendingSuccess,
    DeliveryFailure,
    SendingFailure,
    MemoryFull,
    MemoryAvailable,
    MessageDeleted,
    MessageShift,
    ReadStatusChanged,
    /// A type we don't act on (MAP 1.3+ adds conversation and presence events).
    Unknown,
}

/// The wire names of the MAP 1.1 event types.
const EVENT_NAMES: [(&str, EventType); 10] = [
    ("NewMessage", EventType::NewMessage),
    ("DeliverySuccess", EventType::DeliverySuccess),
    ("SendingSuccess", EventType::SendingSuccess),
    ("DeliveryFailure", EventType::DeliveryFailure),
    ("SendingFailure", EventType::SendingFailure),
    ("MemoryFull", EventType::MemoryFull),
    ("MemoryAvailable", EventType::MemoryAvailable),
    ("MessageDeleted", EventType::MessageDeleted),
    ("MessageShift", EventType::MessageShift),
    ("ReadStatusChanged", EventType::ReadStatusChanged),
];

impl EventType {
    /// The spec spells these exactly; matching ignores case so a sloppy phone still works.
    pub fn parse(s: &str) -> Self {
        let s = s.trim();
        EVENT_NAMES
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(s))
            .map_or(EventType::Unknown, |&(_, kind)| kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MnsEvent {
    pub kind: EventType,
    /// The message's MAP handle; absent for MemoryFull/MemoryAvailable.
    pub handle: Option<String>,
    /// Lowercase last path segment (`TELECOM/MSG/INBOX` → `inbox`), see [`folder_name`].
    pub folder: Option<String>,
    /// MessageShift only: where the message was before, normalized the same way.
    pub old_folder: Option<String>,
    /// `SMS_GSM`, `SMS_CDMA`, `MMS`, `EMAIL`…, as sent.
    pub msg_type: Option<String>,
    /// MAP 1.2+ `yyyyMMddTHHmmss` (see `listing::datetime_to_iso`), as sent.
    pub datetime: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EventReportError {
    #[error("event report has no <event> element")]
    NoEvent,
    #[error("event report's <event> element is never closed")]
    Unterminated,
    #[error("event report's <event> has no type")]
    NoType,
}

/// Parse the body of a SendEvent PUT. A report holds one event; anything after the
/// first `<event>` is ignored. Unknown types parse (as [`EventType::Unknown`]) so the
/// caller can still log them or fall back to a refresh.
pub fn parse_event_report(body: &[u8]) -> Result<MnsEvent, EventReportError> {
    let xml = String::from_utf8_lossy(body);
    let mut rest: &str = &xml;
    while let Some(start) = rest.find("<event") {
        let after = &rest[start + 6..];
        // Must be the element `event`, not e.g. `<events`.
        if !after.starts_with(|c: char| c.is_whitespace() || c == '/' || c == '>') {
            rest = after;
            continue;
        }
        let end = listing::tag_end(after).ok_or(EventReportError::Unterminated)?;
        let attrs = listing::attributes(&after[..end]);
        let get = |k: &str| {
            attrs
                .get(k)
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        let kind = get("type")
            .map(|t| EventType::parse(&t))
            .ok_or(EventReportError::NoType)?;
        return Ok(MnsEvent {
            kind,
            handle: get("handle"),
            folder: get("folder").as_deref().and_then(folder_name),
            old_folder: get("old_folder").as_deref().and_then(folder_name),
            msg_type: get("msg_type"),
            datetime: get("datetime"),
        });
    }
    Err(EventReportError::NoEvent)
}

/// The folder a MAP path names, lowercase: iOS sends `TELECOM/MSG/INBOX`, other phones
/// `telecom/msg/inbox` or just `inbox`, and the worker only cares which folder it is.
pub fn folder_name(path: &str) -> Option<String> {
    path.rsplit(['/', '\\'])
        .map(str::trim)
        .find(|s| !s.is_empty())
        .map(str::to_ascii_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(event: &str) -> Vec<u8> {
        format!("<?xml version=\"1.0\"?>\n<MAP-event-report version=\"1.0\">\n  {event}\n</MAP-event-report>\n")
            .into_bytes()
    }

    #[test]
    fn parses_an_ios_new_message() {
        let body = report(
            r#"<event type="NewMessage" handle="0400000000000071" folder="TELECOM/MSG/INBOX" msg_type="SMS_GSM" datetime="20261005T091502"/>"#,
        );
        assert_eq!(
            parse_event_report(&body),
            Ok(MnsEvent {
                kind: EventType::NewMessage,
                handle: Some("0400000000000071".into()),
                folder: Some("inbox".into()),
                old_folder: None,
                msg_type: Some("SMS_GSM".into()),
                datetime: Some("20261005T091502".into()),
            })
        );
    }

    #[test]
    fn ios_sending_success_lands_in_sent() {
        let body = report(
            r#"<event type="SendingSuccess" handle="20000100009" folder="telecom/msg/sent" msg_type="SMS_GSM" />"#,
        );
        let e = parse_event_report(&body).unwrap();
        assert_eq!(e.kind, EventType::SendingSuccess);
        assert_eq!(e.handle.as_deref(), Some("20000100009"));
        assert_eq!(e.folder.as_deref(), Some("sent"));
        assert_eq!(e.datetime, None, "MAP 1.0 reports carry no datetime");
    }

    #[test]
    fn parses_every_event_type() {
        for (name, kind) in EVENT_NAMES {
            let body = report(&format!(
                r#"<event type="{name}" handle="1" folder="TELECOM/MSG/INBOX" msg_type="MMS"/>"#
            ));
            assert_eq!(parse_event_report(&body).unwrap().kind, kind, "{name}");
        }
        assert_eq!(EventType::parse("newmessage"), EventType::NewMessage);
        assert_eq!(EventType::parse(" READSTATUSCHANGED "), EventType::ReadStatusChanged);
    }

    #[test]
    fn unknown_types_still_parse() {
        let body = report(r#"<event type="ConversationChanged" handle="7" folder="TELECOM/MSG/INBOX"/>"#);
        let e = parse_event_report(&body).unwrap();
        assert_eq!(e.kind, EventType::Unknown);
        assert_eq!(e.handle.as_deref(), Some("7"));
    }

    #[test]
    fn message_shift_normalizes_both_folders() {
        let body = report(
            r#"<event type="MessageShift" handle="20000100009" folder="TELECOM/MSG/SENT" old_folder="TELECOM/MSG/OUTBOX" msg_type="SMS_GSM"/>"#,
        );
        let e = parse_event_report(&body).unwrap();
        assert_eq!(e.kind, EventType::MessageShift);
        assert_eq!(e.folder.as_deref(), Some("sent"));
        assert_eq!(e.old_folder.as_deref(), Some("outbox"));
    }

    #[test]
    fn missing_and_empty_attributes_are_none() {
        let e = parse_event_report(&report(r#"<event type="MemoryFull"/>"#)).unwrap();
        assert_eq!(
            e,
            MnsEvent {
                kind: EventType::MemoryFull,
                handle: None,
                folder: None,
                old_folder: None,
                msg_type: None,
                datetime: None,
            }
        );
        let e = parse_event_report(&report(
            r#"<event type="NewMessage" handle="" folder="" msg_type=" "/>"#,
        ))
        .unwrap();
        assert_eq!((e.handle, e.folder, e.msg_type), (None, None, None));
    }

    #[test]
    fn rejects_reports_without_a_usable_event() {
        assert_eq!(parse_event_report(b""), Err(EventReportError::NoEvent));
        assert_eq!(
            parse_event_report(b"<MAP-event-report version=\"1.0\"></MAP-event-report>"),
            Err(EventReportError::NoEvent)
        );
        assert_eq!(
            parse_event_report(b"<events type=\"NewMessage\"/><event-x type=\"NewMessage\"/>"),
            Err(EventReportError::NoEvent),
            "only the element `event` counts"
        );
        assert_eq!(
            parse_event_report(&[0xFF, 0xFE, 0x00, 0x80]),
            Err(EventReportError::NoEvent)
        );
        assert_eq!(
            parse_event_report(&report(r#"<event handle="1" folder="TELECOM/MSG/INBOX"/>"#)),
            Err(EventReportError::NoType)
        );
        assert_eq!(
            parse_event_report(&report(r#"<event type="" handle="1"/>"#)),
            Err(EventReportError::NoType)
        );
        assert_eq!(
            parse_event_report(b"<MAP-event-report><event type=\"NewMessage\" handle=\"1\""),
            Err(EventReportError::Unterminated)
        );
        assert_eq!(
            parse_event_report(b"<event type=\"NewMessage\" handle=\"1>"),
            Err(EventReportError::Unterminated),
            "an unclosed quote swallows the `>`"
        );
    }

    #[test]
    fn decodes_entities_and_tolerates_layout() {
        let body = report(
            "<event type = 'NewMessage' handle='&#x31;&#50;3' folder=\"TELECOM/MSG/IN&amp;BOX\"\n       msg_type=\"SMS&#95;GSM\" datetime=\"x &gt; y\"></event>",
        );
        let e = parse_event_report(&body).unwrap();
        assert_eq!(e.kind, EventType::NewMessage);
        assert_eq!(e.handle.as_deref(), Some("123"));
        assert_eq!(e.folder.as_deref(), Some("in&box"));
        assert_eq!(e.msg_type.as_deref(), Some("SMS_GSM"));
        assert_eq!(e.datetime.as_deref(), Some("x > y"));
    }

    #[test]
    fn takes_the_first_event_and_ignores_trailing_nuls() {
        let mut body = report(
            r#"<event type="DeliverySuccess" handle="1" folder="TELECOM/MSG/SENT"/><event type="NewMessage" handle="2"/>"#,
        );
        body.extend_from_slice(&[0, 0]);
        let e = parse_event_report(&body).unwrap();
        assert_eq!(e.kind, EventType::DeliverySuccess);
        assert_eq!(e.handle.as_deref(), Some("1"));
    }

    #[test]
    fn folder_names_are_the_lowercase_last_segment() {
        assert_eq!(folder_name("TELECOM/MSG/INBOX").as_deref(), Some("inbox"));
        assert_eq!(folder_name("telecom/msg/sent/").as_deref(), Some("sent"));
        assert_eq!(folder_name("Outbox").as_deref(), Some("outbox"));
        assert_eq!(folder_name("telecom\\msg\\deleted").as_deref(), Some("deleted"));
        assert_eq!(folder_name(""), None);
        assert_eq!(folder_name(" / "), None);
    }
}
