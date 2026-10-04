//! bMessage (MAP 1.4 §3.1.3): the vCard-like envelope MAP uses for one message.
//!
//! iOS is strict about outgoing objects: every line ends in CRLF, and `LENGTH`
//! is the UTF-8 byte count from `BEGIN:MSG` through `END:MSG\r\n` inclusive.
//! A wrong count is accepted at the OBEX level and then silently discarded.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BMessage {
    pub read: bool,
    pub msg_type: String,
    pub folder: String,
    /// Sender's display name and address (phone number or email), if given.
    pub originator_name: Option<String>,
    pub originator_address: Option<String>,
    pub body: String,
}

/// Build a bMessage for PushMessage to `telecom/msg/outbox`.
///
/// `TYPE:SMS_GSM` is used for every recipient; iOS upgrades to iMessage itself
/// when the recipient is registered. Addresses containing `@` go in `EMAIL`.
pub fn compose(recipient: &str, text: &str) -> Vec<u8> {
    let recipient: String = recipient.chars().filter(|c| !c.is_control()).collect();
    let field = if recipient.contains('@') { "EMAIL" } else { "TEL" };
    let msg = message_block(text);
    let mut out = String::new();
    for line in [
        "BEGIN:BMSG",
        "VERSION:1.0",
        "STATUS:UNREAD",
        "TYPE:SMS_GSM",
        "FOLDER:telecom/msg/outbox",
        // Originator: empty vCard, outside the envelope.
        "BEGIN:VCARD",
        "VERSION:2.1",
        "N:;",
        "TEL:",
        "END:VCARD",
        "BEGIN:BENV",
        "BEGIN:VCARD",
        "VERSION:2.1",
    ] {
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.push_str(&format!("N:;{recipient}\r\n{field}:{recipient}\r\n"));
    out.push_str("END:VCARD\r\nBEGIN:BBODY\r\nCHARSET:UTF-8\r\n");
    out.push_str(&format!("LENGTH:{}\r\n", msg.len()));
    out.push_str(&msg);
    out.push_str("END:BBODY\r\nEND:BENV\r\nEND:BMSG\r\n");
    out.into_bytes()
}

/// `BEGIN:MSG\r\n<text with CRLF>\r\nEND:MSG\r\n`, with any line that would end the
/// block early neutralised.
fn message_block(text: &str) -> String {
    let normalised = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<String> = normalised
        .split('\n')
        .map(|l| {
            if l.trim() == "END:MSG" {
                format!(" {l}")
            } else {
                l.to_string()
            }
        })
        .collect();
    format!("BEGIN:MSG\r\n{}\r\nEND:MSG\r\n", lines.join("\r\n"))
}

/// Parse a bMessage from GetMessage. Tolerates LF-only line endings.
pub fn parse(raw: &str) -> BMessage {
    let mut m = BMessage::default();
    let mut depth_benv = 0;
    let mut in_vcard = false;
    let mut originator_done = false;
    let mut in_msg = false;
    let mut body: Vec<&str> = Vec::new();
    for line in raw.lines() {
        if in_msg {
            if line == "END:MSG" {
                in_msg = false;
            } else {
                body.push(line);
            }
            continue;
        }
        match line {
            "BEGIN:BENV" => depth_benv += 1,
            "END:BENV" => depth_benv -= 1,
            "BEGIN:VCARD" => in_vcard = true,
            "END:VCARD" => {
                in_vcard = false;
                if depth_benv == 0 {
                    originator_done = true;
                }
            }
            "BEGIN:MSG" => in_msg = true,
            _ => {
                let Some((key, value)) = line.split_once(':') else {
                    continue;
                };
                // vCard keys may carry parameters, e.g. "TEL;TYPE=CELL".
                let key = key.split(';').next().unwrap_or(key);
                let originator = in_vcard && depth_benv == 0 && !originator_done;
                match key {
                    "STATUS" if !in_vcard => m.read = value.eq_ignore_ascii_case("READ"),
                    "TYPE" if !in_vcard => m.msg_type = value.to_string(),
                    "FOLDER" if !in_vcard => m.folder = value.to_string(),
                    "FN" if originator && !value.is_empty() => m.originator_name = Some(value.to_string()),
                    "N" if originator && m.originator_name.is_none() => {
                        let name = value.split(';').filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
                        if !name.is_empty() {
                            m.originator_name = Some(name);
                        }
                    }
                    "TEL" | "EMAIL" if originator && !value.is_empty() => {
                        m.originator_address = Some(value.to_string())
                    }
                    _ => {}
                }
            }
        }
    }
    m.body = body.join("\n");
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn length_field(raw: &str) -> usize {
        raw.lines()
            .find_map(|l| l.strip_prefix("LENGTH:"))
            .unwrap()
            .parse()
            .unwrap()
    }

    #[test]
    fn compose_uses_crlf_and_exact_utf8_length() {
        let raw = String::from_utf8(compose("+15551234567", "Héllo 👋\nsecond line")).unwrap();
        assert!(!raw.replace("\r\n", "").contains('\n'), "every newline is CRLF");
        let start = raw.find("BEGIN:MSG\r\n").unwrap();
        let end = raw.find("END:MSG\r\n").unwrap() + "END:MSG\r\n".len();
        assert_eq!(
            length_field(&raw),
            raw[start..end].len(),
            "LENGTH counts bytes, inclusive"
        );
        assert!(raw.contains("Héllo 👋\r\nsecond line\r\n"));
        assert!(raw.contains("TEL:+15551234567\r\n"));
        assert!(raw.ends_with("END:BBODY\r\nEND:BENV\r\nEND:BMSG\r\n"));
    }

    #[test]
    fn compose_routes_email_recipients_and_strips_control_chars() {
        let raw = String::from_utf8(compose("zoe@example.com\r\nEND:BMSG", "hi")).unwrap();
        assert!(raw.contains("EMAIL:zoe@example.comEND:BMSG\r\n"), "no injected lines");
        assert_eq!(
            raw.lines().filter(|l| *l == "END:BMSG").count(),
            1,
            "only the real terminator starts a line"
        );
    }

    #[test]
    fn compose_neutralises_end_msg_in_text() {
        let raw = String::from_utf8(compose("+1555", "a\nEND:MSG\nb")).unwrap();
        assert_eq!(raw.matches("\r\nEND:MSG\r\n").count(), 1);
    }

    #[test]
    fn parses_incoming_message() {
        let raw = "BEGIN:BMSG\r\nVERSION:1.0\r\nSTATUS:UNREAD\r\nTYPE:SMS_GSM\r\nFOLDER:telecom/msg/inbox\r\n\
                   BEGIN:VCARD\r\nVERSION:2.1\r\nN:Zoe\r\nTEL;TYPE=CELL:+15559876543\r\nEND:VCARD\r\n\
                   BEGIN:BENV\r\nBEGIN:VCARD\r\nVERSION:2.1\r\nN:\r\nTEL:\r\nEND:VCARD\r\nBEGIN:BBODY\r\n\
                   CHARSET:UTF-8\r\nLENGTH:40\r\nBEGIN:MSG\r\nomw, 10 mins\r\nsee you\r\nEND:MSG\r\n\
                   END:BBODY\r\nEND:BENV\r\nEND:BMSG\r\n";
        let m = parse(raw);
        assert!(!m.read);
        assert_eq!(m.msg_type, "SMS_GSM");
        assert_eq!(m.folder, "telecom/msg/inbox");
        assert_eq!(m.originator_name.as_deref(), Some("Zoe"));
        assert_eq!(m.originator_address.as_deref(), Some("+15559876543"));
        assert_eq!(m.body, "omw, 10 mins\nsee you");
    }

    #[test]
    fn round_trips_composed_body() {
        let m = parse(std::str::from_utf8(&compose("+1555", "line one\nline two")).unwrap());
        assert_eq!(m.body, "line one\nline two");
        assert_eq!(m.folder, "telecom/msg/outbox");
    }
}
