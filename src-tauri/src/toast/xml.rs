//! Pure half of the actionable pop-ups: what a toast offers ([`ToastSpec`]), how a button
//! press is written into the toast's `arguments` and read back ([`ToastAction`]), and the
//! toast XML itself. No WinRT here, so all of it is unit-tested.
//!
//! Toast XML reference: <https://learn.microsoft.com/windows/apps/design/shell/tiles-and-notifications/toast-schema>

use serde::Deserialize;

/// Longest title / body / name put on a toast (characters). Windows caps the whole toast
/// payload at 5 KB and only shows a few lines anyway; tug has the full text.
const TITLE_MAX: usize = 120;
const BODY_MAX: usize = 600;
const NAME_MAX: usize = 40;
/// Windows shows at most five buttons on a toast.
const ACTIONS_MAX: usize = 5;
/// Arguments longer than this weren't written by tug: refuse them.
const ARGS_MAX: usize = 512;
/// A reply address is a phone number or an email; anything longer isn't one.
const ADDRESS_MAX: usize = 64;
/// One-time codes are a handful of letters and digits.
const CODE_MAX: usize = 16;

/// The id of the text box on a reply toast; its text arrives in the activation's UserInput.
pub const REPLY_INPUT: &str = "reply";

/// What the frontend asks a pop-up to show and offer. It decides *whether* to toast
/// (toasts on/off, do-not-disturb, muted apps, the rate limit) and what applies to this
/// notification; this side only builds it. Mirrored in `src/types/protocol.ts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToastSpec {
    /// The notification's row id: what every button acts on.
    pub id: i64,
    pub title: String,
    pub body: String,
    /// Who it's from, for the reply box and the "Sent to …" confirmation.
    #[serde(default)]
    pub name: String,
    /// A text from a person tug can reply to over MAP: the reply box and Send.
    #[serde(default)]
    pub reply_to: Option<String>,
    /// A conversation: "Mark read" (read in tug, read and cleared on the phone).
    #[serde(default)]
    pub mark_read: bool,
    /// A one-time code in the notification: "Copy code".
    #[serde(default)]
    pub code: Option<String>,
    /// A missed call still on the phone with its "Dial" action: "Call back".
    #[serde(default)]
    pub call_back: bool,
    /// Clearable on the phone (never a ringing call): "Clear".
    #[serde(default)]
    pub clear: bool,
}

/// A press on the toast (its body or a button), as carried in the toast's arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastAction {
    /// The body: bring tug forward at this notification.
    Open { id: i64 },
    /// Send: the typed text (in UserInput, not here) goes to `to` over MAP.
    Reply { id: i64, to: String },
    /// Mark the conversation read.
    Read { id: i64 },
    /// Put the code on the clipboard.
    Copy { id: i64, code: String },
    /// Missed call: the phone's "Dial" (ANCS positive action).
    CallBack { id: i64 },
    /// Clear it on the phone (ANCS negative action).
    Clear { id: i64 },
}

impl ToastAction {
    pub fn id(&self) -> i64 {
        match self {
            Self::Open { id }
            | Self::Reply { id, .. }
            | Self::Read { id }
            | Self::Copy { id, .. }
            | Self::CallBack { id }
            | Self::Clear { id } => *id,
        }
    }
}

/// `a=reply&id=12&to=%2B13025550123`. Values are percent-encoded so a number, an email or
/// a code can never break out of its field (or the XML attribute it ends up in).
pub fn encode(action: &ToastAction) -> String {
    let (kind, extra) = match action {
        ToastAction::Open { .. } => ("open", None),
        ToastAction::Reply { to, .. } => ("reply", Some(("to", to.as_str()))),
        ToastAction::Read { .. } => ("read", None),
        ToastAction::Copy { code, .. } => ("copy", Some(("code", code.as_str()))),
        ToastAction::CallBack { .. } => ("call", None),
        ToastAction::Clear { .. } => ("clear", None),
    };
    let mut out = format!("a={kind}&id={}", action.id());
    if let Some((key, value)) = extra {
        out.push('&');
        out.push_str(key);
        out.push('=');
        out.push_str(&percent_encode(value));
    }
    out
}

/// Read back what [`encode`] wrote. Anything else (too long, unknown action, a bad id, a
/// value that couldn't be a number or a code) is refused rather than half-understood.
pub fn decode(args: &str) -> Option<ToastAction> {
    if args.is_empty() || args.len() > ARGS_MAX {
        return None;
    }
    let (mut kind, mut id, mut to, mut code) = (None, None, None, None);
    for pair in args.split('&') {
        let (key, value) = pair.split_once('=')?;
        let value = percent_decode(value)?;
        let slot = match key {
            "a" => &mut kind,
            "id" => &mut id,
            "to" => &mut to,
            "code" => &mut code,
            _ => return None,
        };
        if slot.replace(value).is_some() {
            return None; // the same field twice
        }
    }
    let id: i64 = id?.parse().ok().filter(|id| *id > 0)?;
    let extra_ok = |present: bool, wanted: bool| present == wanted;
    let action = match kind?.as_str() {
        "open" => ToastAction::Open { id },
        "reply" => ToastAction::Reply {
            id,
            to: to.clone().filter(|t| valid_address(t))?,
        },
        "read" => ToastAction::Read { id },
        "copy" => ToastAction::Copy {
            id,
            code: code.clone().filter(|c| valid_code(c))?,
        },
        "call" => ToastAction::CallBack { id },
        "clear" => ToastAction::Clear { id },
        _ => return None,
    };
    let wants_to = matches!(action, ToastAction::Reply { .. });
    let wants_code = matches!(action, ToastAction::Copy { .. });
    (extra_ok(to.is_some(), wants_to) && extra_ok(code.is_some(), wants_code)).then_some(action)
}

fn valid_address(to: &str) -> bool {
    !to.trim().is_empty() && to.chars().count() <= ADDRESS_MAX && !to.chars().any(char::is_control)
}

fn valid_code(code: &str) -> bool {
    !code.is_empty() && code.len() <= CODE_MAX && code.chars().all(|c| c.is_ascii_alphanumeric())
}

fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = value
                    .get(i + 1..i + 3)
                    .filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'&' | b'=' => return None,
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Text made safe for XML: markup characters escaped, and characters XML 1.0 forbids
/// (most control characters) dropped, since `LoadXml` rejects the whole toast for one.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// At most `max` characters, ending in "…" when cut. Counts characters, not bytes, so an
/// emoji is never split.
pub fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

fn action(content: &str, args: &ToastAction, input: Option<&str>) -> String {
    // Foreground, not background: an unpackaged app has no background task to start, and
    // a foreground press is what reliably raises ToastNotification.Activated in tug (and,
    // when tug isn't running, starts it instead of doing nothing).
    let hint = input
        .map(|id| format!(r#" hint-inputId="{}""#, escape(id)))
        .unwrap_or_default();
    format!(
        r#"<action content="{}" arguments="{}" activationType="foreground"{hint}/>"#,
        escape(content),
        escape(&encode(args))
    )
}

/// The toast for one phone notification, with the buttons its spec allows.
pub fn notification_toast(spec: &ToastSpec) -> String {
    let id = spec.id;
    let name = clip(&spec.name, NAME_MAX);
    let mut inputs = String::new();
    let mut actions: Vec<String> = Vec::new();
    if let Some(to) = spec.reply_to.as_deref().filter(|t| valid_address(t)) {
        let placeholder = if name.is_empty() {
            "Reply".to_string()
        } else {
            format!("Reply to {name}")
        };
        inputs = format!(
            r#"<input id="{REPLY_INPUT}" type="text" placeHolderContent="{}"/>"#,
            escape(&placeholder)
        );
        let reply = ToastAction::Reply { id, to: to.to_string() };
        actions.push(action("Send", &reply, Some(REPLY_INPUT)));
    }
    if spec.mark_read {
        actions.push(action("Mark read", &ToastAction::Read { id }, None));
    }
    if let Some(code) = spec.code.as_deref().filter(|c| valid_code(c)) {
        let copy = ToastAction::Copy {
            id,
            code: code.to_string(),
        };
        actions.push(action("Copy code", &copy, None));
    }
    if spec.call_back {
        actions.push(action("Call back", &ToastAction::CallBack { id }, None));
    }
    // Mark read already clears a conversation's notification on the phone.
    if spec.clear && !spec.mark_read {
        actions.push(action("Clear", &ToastAction::Clear { id }, None));
    }
    actions.truncate(ACTIONS_MAX);
    let actions = if inputs.is_empty() && actions.is_empty() {
        String::new()
    } else {
        format!("<actions>{inputs}{}</actions>", actions.concat())
    };
    toast(&ToastAction::Open { id }, &spec.title, &spec.body, &actions, false)
}

/// A plain follow-up (a reply was sent, or something couldn't be done). Clicking it brings
/// tug forward at the notification it's about. `quiet` ones make no sound.
pub fn note_toast(id: i64, title: &str, body: &str, quiet: bool) -> String {
    toast(&ToastAction::Open { id }, title, body, "", quiet)
}

fn toast(launch: &ToastAction, title: &str, body: &str, actions: &str, quiet: bool) -> String {
    let title = clip(title, TITLE_MAX);
    let title = if title.is_empty() { "tug".to_string() } else { title };
    let body = clip(body, BODY_MAX);
    let body = if body.is_empty() {
        String::new()
    } else {
        format!("<text>{}</text>", escape(&body))
    };
    let audio = if quiet { r#"<audio silent="true"/>"# } else { "" };
    format!(
        r#"<toast launch="{}" activationType="foreground"><visual><binding template="ToastGeneric"><text hint-maxLines="1">{}</text>{body}</binding></visual>{actions}{audio}</toast>"#,
        escape(&encode(launch)),
        escape(&title)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ToastSpec {
        ToastSpec {
            id: 42,
            title: "Messages · Zoe".into(),
            body: "omw, 10 mins".into(),
            name: "Zoe".into(),
            ..Default::default()
        }
    }

    #[test]
    fn every_action_survives_a_round_trip() {
        for action in [
            ToastAction::Open { id: 1 },
            ToastAction::Reply {
                id: 2,
                to: "+13025550123".into(),
            },
            ToastAction::Reply {
                id: 3,
                to: "zoe&co@example.com".into(),
            },
            ToastAction::Read { id: 4 },
            ToastAction::Copy {
                id: 5,
                code: "482913".into(),
            },
            ToastAction::CallBack { id: 6 },
            ToastAction::Clear { id: i64::MAX },
        ] {
            assert_eq!(decode(&encode(&action)), Some(action.clone()), "{}", encode(&action));
        }
    }

    #[test]
    fn values_are_percent_encoded() {
        let args = encode(&ToastAction::Reply {
            id: 2,
            to: "+1 (302) 555-0123".into(),
        });
        assert_eq!(args, "a=reply&id=2&to=%2B1%20%28302%29%20555-0123");
        let email = encode(&ToastAction::Reply {
            id: 2,
            to: "a&b=c@x.com".into(),
        });
        assert!(!email["a=reply&id=2&to=".len()..].contains(['&', '=']), "{email}");
    }

    #[test]
    fn refuses_arguments_tug_didnt_write() {
        for bad in [
            "",
            "a=open",
            "id=3",
            "a=open&id=0",
            "a=open&id=-3",
            "a=open&id=x",
            "a=launch&id=3",
            "a=open&id=3&id=4",
            "a=open&id=3&extra=1",
            "a=open&id=3&to=%2B1",
            "a=reply&id=3",
            "a=reply&id=3&to=",
            "a=reply&id=3&to=%0A",
            "a=copy&id=3",
            "a=copy&id=3&code=48%2091",
            "a=copy&id=3&code=12345678901234567",
            "a=open&id=3&code=1",
            "a=open&id=%ZZ",
            "a=open&id=%4",
            "a=open&id=%+1",
            "a=reply&id=3&to=%FF",
        ] {
            assert_eq!(decode(bad), None, "{bad}");
        }
        assert_eq!(decode(&format!("a=open&id=1&to={}", "x".repeat(600))), None);
        let long_to = "1".repeat(ADDRESS_MAX + 1);
        assert_eq!(decode(&format!("a=reply&id=1&to={long_to}")), None);
    }

    #[test]
    fn escapes_markup_and_drops_what_xml_forbids() {
        assert_eq!(
            escape(r#"<b>"Tom" & 'Jerry'</b>"#),
            "&lt;b&gt;&quot;Tom&quot; &amp; &apos;Jerry&apos;&lt;/b&gt;"
        );
        assert_eq!(escape("a\u{0}b\u{7}c\td\ne\u{FFFF}"), "abc\td\ne");
        assert_eq!(escape("emoji 🚗 ok"), "emoji 🚗 ok");
    }

    #[test]
    fn clips_by_character_with_an_ellipsis() {
        assert_eq!(clip("  short  ", 10), "short");
        assert_eq!(clip("abcdefghij", 10), "abcdefghij");
        assert_eq!(clip("abcdefghijk", 10), "abcdefghi…");
        assert_eq!(clip("🚗🚗🚗🚗", 3), "🚗🚗…");
    }

    #[test]
    fn a_plain_notification_only_opens_tug() {
        let xml = notification_toast(&spec());
        assert_eq!(
            xml,
            r#"<toast launch="a=open&amp;id=42" activationType="foreground"><visual><binding template="ToastGeneric"><text hint-maxLines="1">Messages · Zoe</text><text>omw, 10 mins</text></binding></visual></toast>"#
        );
    }

    #[test]
    fn a_text_from_a_person_gets_a_reply_box_send_and_mark_read() {
        let xml = notification_toast(&ToastSpec {
            reply_to: Some("+13025550123".into()),
            mark_read: true,
            clear: true,
            ..spec()
        });
        assert!(
            xml.contains(r#"<input id="reply" type="text" placeHolderContent="Reply to Zoe"/>"#),
            "{xml}"
        );
        assert!(xml.contains(
            r#"<action content="Send" arguments="a=reply&amp;id=42&amp;to=%2B13025550123" activationType="foreground" hint-inputId="reply"/>"#
        ));
        assert!(
            xml.contains(r#"<action content="Mark read" arguments="a=read&amp;id=42" activationType="foreground"/>"#)
        );
        assert!(!xml.contains("Clear"), "mark read already clears: {xml}");
        // Input first, then the buttons, all inside one <actions>.
        assert!(xml.contains("<actions><input"));
    }

    #[test]
    fn codes_missed_calls_and_clearable_notifications_get_their_buttons() {
        let code = notification_toast(&ToastSpec {
            code: Some("482913".into()),
            clear: true,
            ..spec()
        });
        assert!(code.contains(r#"<action content="Copy code" arguments="a=copy&amp;id=42&amp;code=482913""#));
        assert!(code.contains(r#"<action content="Clear" arguments="a=clear&amp;id=42""#));
        let missed = notification_toast(&ToastSpec {
            call_back: true,
            clear: true,
            ..spec()
        });
        assert!(missed.contains(r#"<action content="Call back" arguments="a=call&amp;id=42""#));
        assert!(!missed.contains("<input"));
    }

    #[test]
    fn bad_reply_addresses_and_codes_get_no_button() {
        let xml = notification_toast(&ToastSpec {
            reply_to: Some("  ".into()),
            code: Some("48 2913".into()),
            ..spec()
        });
        assert!(!xml.contains("<actions>"), "{xml}");
    }

    #[test]
    fn hostile_text_cant_break_the_xml() {
        let xml = notification_toast(&ToastSpec {
            title: r#"Messages · "><toast>"#.into(),
            body: "</text><action content=\"x\"/>\u{1}".into(),
            name: "<Zoe>".into(),
            reply_to: Some("+1302\"/><x".into()),
            ..spec()
        });
        assert_eq!(xml.matches("<toast").count(), 1, "{xml}");
        assert_eq!(xml.matches("<action ").count(), 1, "{xml}");
        assert!(xml.contains("Reply to &lt;Zoe&gt;"));
        assert!(!xml.contains('\u{1}'));
    }

    #[test]
    fn long_text_is_clipped() {
        let xml = notification_toast(&ToastSpec {
            title: "t".repeat(500),
            body: "b".repeat(5000),
            ..spec()
        });
        assert!(xml.len() < 1200, "{}", xml.len());
        assert!(xml.contains('…'));
    }

    #[test]
    fn an_empty_title_still_says_tug() {
        let xml = note_toast(7, "", "", true);
        assert_eq!(
            xml,
            r#"<toast launch="a=open&amp;id=7" activationType="foreground"><visual><binding template="ToastGeneric"><text hint-maxLines="1">tug</text></binding></visual><audio silent="true"/></toast>"#
        );
    }

    #[test]
    fn the_spec_reads_the_frontends_camel_case() {
        let spec: ToastSpec = serde_json::from_str(
            r#"{"id":9,"title":"Phone · Mum","body":"Missed Call","name":"Mum","replyTo":null,"markRead":false,"code":null,"callBack":true,"clear":true}"#,
        )
        .unwrap();
        assert_eq!(spec.id, 9);
        assert!(spec.call_back && spec.clear && !spec.mark_read);
    }
}
