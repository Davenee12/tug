//! bMessage (MAP 1.4 §3.1.3): the vCard-like envelope MAP uses for one message.
//!
//! iOS is strict about outgoing objects: every line ends in CRLF, and `LENGTH`
//! is the UTF-8 byte count from `BEGIN:MSG` through `END:MSG\r\n` inclusive.
//! A wrong count is accepted at the OBEX level and then silently discarded.

use super::charset::{self, Decoding};
use super::sms_pdu;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BMessage {
    pub read: bool,
    pub msg_type: String,
    pub folder: String,
    /// Sender's display name and address (phone number or email), if given.
    pub originator_name: Option<String>,
    pub originator_address: Option<String>,
    pub body: String,
    /// How the text's bytes were decoded; anything but `Utf8` is worth a (content-free) log line.
    pub decoding: Decoding,
    /// `charset::byte_pattern` of the text's bytes, when decoding needed a fallback.
    pub body_pattern: String,
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

/// Parse a bMessage already held as text (tests, and callers that have a `str`).
pub fn parse(raw: &str) -> BMessage {
    parse_bytes(raw.as_bytes())
}

/// Parse a bMessage from GetMessage, straight from its bytes. Tolerates LF-only line endings.
///
/// The envelope is ASCII, but the message text between `BEGIN:MSG` and `END:MSG` is taken as a
/// byte range and decoded on its own (`charset::decode`), so text that isn't the UTF-8 tug asked
/// for (UTF-16, Windows-1252, a native SMS PDU, a character cut off at the end) still reads as
/// words instead of U+FFFD. `LENGTH` counts the bytes from `BEGIN:MSG` through `END:MSG\r\n`
/// (MAP 1.4 §3.1.3); when it lands exactly on an `END:MSG` line it marks the end (so a line
/// reading "END:MSG" inside the text can't cut it short), otherwise the first `END:MSG` line does.
pub fn parse_bytes(raw: &[u8]) -> BMessage {
    let mut m = BMessage::default();
    let mut depth_benv = 0;
    let mut in_vcard = false;
    let mut originator_done = false;
    // Per BBODY: what the phone says about the text that follows.
    let mut charset_param: Option<String> = None;
    let mut encoding: Option<String> = None;
    let mut length: Option<usize> = None;
    let mut parts: Vec<String> = Vec::new();
    let mut pos = 0;
    while pos < raw.len() {
        let line_start = pos;
        let (line, next) = next_line(raw, pos);
        pos = next;
        if line == b"BEGIN:MSG" {
            let (content, after) = message_content(raw, line_start, next, length);
            let (text, how) = decode_content(content, charset_param.as_deref(), encoding.as_deref());
            if how.is_fallback() && !m.decoding.is_fallback() {
                m.decoding = how;
                m.body_pattern = charset::byte_pattern(content);
            }
            parts.push(text.replace("\r\n", "\n"));
            pos = after;
            continue;
        }
        let line = charset::decode_line(line);
        match line.as_str() {
            "BEGIN:BENV" => depth_benv += 1,
            "END:BENV" => depth_benv -= 1,
            "BEGIN:VCARD" => in_vcard = true,
            "END:VCARD" => {
                in_vcard = false;
                if depth_benv == 0 {
                    originator_done = true;
                }
            }
            "BEGIN:BBODY" => {
                charset_param = None;
                encoding = None;
                length = None;
            }
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
                    "CHARSET" if !in_vcard => charset_param = Some(value.trim().to_string()),
                    "ENCODING" if !in_vcard => encoding = Some(value.trim().to_string()),
                    "LENGTH" if !in_vcard => length = value.trim().parse().ok(),
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
    m.body = parts.join("\n");
    m
}

/// The line starting at `pos` without its line ending, and where the next one starts.
fn next_line(raw: &[u8], pos: usize) -> (&[u8], usize) {
    let rest = &raw[pos..];
    let (line, next) = match rest.iter().position(|&b| b == b'\n') {
        Some(nl) => (&rest[..nl], pos + nl + 1),
        None => (rest, raw.len()),
    };
    (line.strip_suffix(b"\r").unwrap_or(line), next)
}

/// The message text after a `BEGIN:MSG` line (which starts at `begin`; the text at `start`), and
/// where parsing resumes after its `END:MSG` line.
fn message_content(raw: &[u8], begin: usize, start: usize, length: Option<usize>) -> (&[u8], usize) {
    // LENGTH, when it lands exactly at the end of an END:MSG line.
    if let Some(end) = length.map(|l| begin + l).filter(|&e| e > start && e <= raw.len()) {
        let block = &raw[..end];
        let block = block
            .strip_suffix(b"\r\n")
            .or_else(|| block.strip_suffix(b"\n"))
            .unwrap_or(block);
        if let Some(before) = block.strip_suffix(b"END:MSG") {
            if before.len() >= start && (before.len() == start || before.ends_with(b"\n")) {
                return (&raw[start..trim_eol(raw, start, before.len())], end);
            }
        }
    }
    // Otherwise the first END:MSG line.
    let mut pos = start;
    while pos < raw.len() {
        let (line, next) = next_line(raw, pos);
        if line == b"END:MSG" {
            return (&raw[start..trim_eol(raw, start, pos)], next);
        }
        pos = next;
    }
    // No END:MSG (a cut-off object): everything left, up to LENGTH if that's shorter.
    let end = length
        .map(|l| begin + l)
        .filter(|&e| e > start && e < raw.len())
        .unwrap_or(raw.len());
    (&raw[start..trim_eol(raw, start, end)], raw.len())
}

/// `end` moved back over the line ending in front of it (the one that belongs to END:MSG), but
/// never before `start`.
fn trim_eol(raw: &[u8], start: usize, mut end: usize) -> usize {
    if end > start && raw[end - 1] == b'\n' {
        end -= 1;
        if end > start && raw[end - 1] == b'\r' {
            end -= 1;
        }
    }
    end
}

/// The text of one `BEGIN:MSG` block. A native body (`CHARSET:native`, or a GSM/CDMA `ENCODING`)
/// is an SMS PDU in hex; anything else is decoded as UTF-8 with fallbacks.
fn decode_content(content: &[u8], charset_param: Option<&str>, encoding: Option<&str>) -> (String, Decoding) {
    let native = charset_param.is_some_and(|c| c.eq_ignore_ascii_case("native"))
        || encoding.is_some_and(|e| {
            let e = e.to_ascii_uppercase();
            e.starts_with("G-") || e.starts_with("C-")
        });
    if native {
        if let Some(text) = std::str::from_utf8(content).ok().filter(|t| sms_pdu::looks_like_hex(t)) {
            if let Some(decoded) = sms_pdu::decode_hex_pdus(text) {
                return (decoded, Decoding::SmsPdu);
            }
        }
    }
    charset::decode(content)
}

/// Whether a body has any words in it: not empty, and not only U+FFFD (a character that didn't
/// survive) or U+FFFC (where an attachment sat).
pub fn has_text(body: &str) -> bool {
    body.chars()
        .any(|c| !c.is_whitespace() && c != '\u{FFFD}' && c != '\u{FFFC}')
}

/// The shape of a body that looks like iOS's attachment line in a language tug doesn't know yet
/// ("<label>: <count> <kind>", short, one line), as letters `W` and numbers `N` only, e.g.
/// `"W W: N W"`. None for English ("Attachment: 1 Photo", which the app understands) and for
/// anything that doesn't look like one. Logged (never the words) so more languages can be added.
pub fn unknown_attachment_shape(body: &str) -> Option<String> {
    let body = body.trim();
    if body.is_empty() || body.chars().count() > 48 || body.contains('\n') {
        return None;
    }
    let (label, rest) = body.split_once([':', '\u{FF1A}'])?;
    let label_words: Vec<&str> = label.split_whitespace().collect();
    let all_letters = label_words.iter().all(|w| w.chars().all(char::is_alphabetic));
    if label_words.is_empty() || label_words.len() > 3 || !all_letters {
        return None;
    }
    if label_words.len() == 1 && matches!(label_words[0].to_lowercase().as_str(), "attachment" | "attachments") {
        return None;
    }
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    let has_number = tokens.iter().any(|t| t.chars().any(|c| c.is_ascii_digit()));
    let has_word = tokens.iter().any(|t| t.chars().any(char::is_alphabetic));
    if tokens.is_empty() || tokens.len() > 4 || !has_number || !has_word {
        return None;
    }
    let class = |t: &&str| match (
        t.chars().any(char::is_alphabetic),
        t.chars().any(|c| c.is_ascii_digit()),
    ) {
        (true, true) => "WN",
        (false, true) => "N",
        (true, false) => "W",
        (false, false) => "P",
    };
    let label_shape = vec!["W"; label_words.len()].join(" ");
    let rest_shape: Vec<&str> = tokens.iter().map(class).collect();
    Some(format!("{label_shape}: {}", rest_shape.join(" ")))
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

    /// An incoming bMessage around `content` (raw bytes), with a correct LENGTH unless given one.
    fn incoming(content: &[u8], charset: &str, length: Option<usize>) -> Vec<u8> {
        let mut block = b"BEGIN:MSG\r\n".to_vec();
        block.extend_from_slice(content);
        block.extend_from_slice(b"\r\nEND:MSG\r\n");
        let mut out = format!(
            "BEGIN:BMSG\r\nVERSION:1.0\r\nSTATUS:UNREAD\r\nTYPE:SMS_GSM\r\nFOLDER:telecom/msg/inbox\r\n\
             BEGIN:VCARD\r\nVERSION:2.1\r\nN:Zoe\r\nTEL:+15559876543\r\nEND:VCARD\r\n\
             BEGIN:BENV\r\nBEGIN:BBODY\r\nCHARSET:{charset}\r\nLENGTH:{}\r\n",
            length.unwrap_or(block.len())
        )
        .into_bytes();
        out.extend(block);
        out.extend_from_slice(b"END:BBODY\r\nEND:BENV\r\nEND:BMSG\r\n");
        out
    }

    #[test]
    fn emoji_and_accents_survive() {
        let text = "Lunch? 🍕👍🏽\r\ncafé at 1 👨‍👩‍👧";
        let m = parse_bytes(&incoming(text.as_bytes(), "UTF-8", None));
        assert_eq!(m.body, "Lunch? 🍕👍🏽\ncafé at 1 👨‍👩‍👧");
        assert_eq!(m.decoding, Decoding::Utf8);
        assert_eq!(m.body_pattern, "", "nothing to log");
        assert_eq!(m.originator_address.as_deref(), Some("+15559876543"));
    }

    #[test]
    fn length_keeps_a_line_that_reads_end_msg_inside_the_text() {
        let m = parse_bytes(&incoming(b"a\r\nEND:MSG\r\nb", "UTF-8", None));
        assert_eq!(m.body, "a\nEND:MSG\nb");
        assert_eq!(m.folder, "telecom/msg/inbox", "the rest of the envelope still parses");
    }

    #[test]
    fn a_wrong_length_falls_back_to_the_end_msg_line() {
        for wrong in [3, 9999, 0] {
            let m = parse_bytes(&incoming("see you 👋".as_bytes(), "UTF-8", Some(wrong)));
            assert_eq!(m.body, "see you 👋", "LENGTH {wrong}");
        }
    }

    #[test]
    fn utf16_text_despite_asking_for_utf8() {
        let text: Vec<u8> = "Running late 🚗".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let m = parse_bytes(&incoming(&text, "UTF-8", None));
        assert_eq!(m.body, "Running late 🚗");
        assert_eq!(m.decoding, Decoding::Utf16Le);
        assert!(m.body_pattern.starts_with(&format!("{} bytes", text.len())));
    }

    #[test]
    fn windows_1252_text_reads_as_words_not_replacement_characters() {
        // "Café – 5€" in Windows-1252.
        let m = parse_bytes(&incoming(
            &[0x43, 0x61, 0x66, 0xE9, 0x20, 0x96, 0x20, 0x35, 0x80],
            "UTF-8",
            None,
        ));
        assert_eq!(m.body, "Café – 5€");
        assert_eq!(m.decoding, Decoding::Cp1252);
        assert!(!m.body.contains('\u{FFFD}'));
    }

    #[test]
    fn a_character_split_by_a_cut_off_object_is_dropped_not_garbled() {
        // The object ends mid-emoji, with no END:MSG: keep the words, drop the half character.
        let mut raw = incoming("ok see you 😊 later".as_bytes(), "UTF-8", Some(31));
        let cut = raw.windows(4).position(|w| w == "😊".as_bytes()).unwrap() + 2;
        raw.truncate(cut);
        let m = parse_bytes(&raw);
        assert_eq!(m.body, "ok see you ");
        assert_eq!(m.decoding, Decoding::Utf8CutShort);
    }

    #[test]
    fn a_native_sms_pdu_is_decoded() {
        let pdu = b"07917283010010F5040BC87238880900F10000993092516195800AE8329BFD4697D9EC37";
        let m = parse_bytes(&incoming(pdu, "native", None));
        assert_eq!(m.body, "hellohello");
        assert_eq!(m.decoding, Decoding::SmsPdu);
        // Hex-looking text in a UTF-8 body stays text.
        let m = parse_bytes(&incoming(b"cafe", "UTF-8", None));
        assert_eq!(m.body, "cafe");
    }

    #[test]
    fn a_replacement_character_from_the_phone_is_flagged() {
        let m = parse_bytes(&incoming("\u{FFFD}".as_bytes(), "UTF-8", None));
        assert_eq!(m.body, "\u{FFFD}");
        assert_eq!(m.decoding, Decoding::Utf8WithReplacement);
        assert_eq!(m.body_pattern, "3 bytes nul=0 high=3 first=ltt");
        assert!(!has_text(&m.body));
    }

    #[test]
    fn lf_only_line_endings_and_several_parts() {
        let raw = "BEGIN:BMSG\nTYPE:MMS\nBEGIN:BENV\nBEGIN:BBODY\nCHARSET:UTF-8\nBEGIN:MSG\nfirst\nEND:MSG\n\
                   END:BBODY\nBEGIN:BBODY\nBEGIN:MSG\nsecond\nEND:MSG\nEND:BBODY\nEND:BENV\nEND:BMSG\n";
        let m = parse(raw);
        assert_eq!(m.body, "first\nsecond");
        assert_eq!(m.msg_type, "MMS");
    }

    #[test]
    fn an_empty_body() {
        let m = parse_bytes(&incoming(b"", "UTF-8", None));
        assert_eq!(m.body, "");
        assert!(!has_text(&m.body));
    }

    #[test]
    fn has_text_needs_a_real_character() {
        assert!(has_text("ok"));
        assert!(has_text("\u{FFFD}a"));
        assert!(!has_text(""));
        assert!(!has_text(" \u{FFFD} "));
        assert!(!has_text("\u{FFFC}"));
    }

    #[test]
    fn unknown_attachment_shapes_are_described_without_words() {
        assert_eq!(unknown_attachment_shape("Anhang: 1 Foto").as_deref(), Some("W: N W"));
        assert_eq!(
            unknown_attachment_shape("Pièce jointe : 2 photos").as_deref(),
            Some("W W: N W")
        );
        assert_eq!(
            unknown_attachment_shape("添付ファイル：写真1枚").as_deref(),
            Some("W: WN")
        );
        // English is understood already; ordinary texts aren't attachment lines.
        assert_eq!(unknown_attachment_shape("Attachment: 1 Photo"), None);
        assert_eq!(unknown_attachment_shape("attachments: 2 videos"), None);
        assert_eq!(unknown_attachment_shape("see you at 5"), None);
        assert_eq!(
            unknown_attachment_shape("Note: bring 2 chairs and a table please"),
            None
        );
        assert_eq!(unknown_attachment_shape("Anhang: Foto"), None, "no count");
        assert_eq!(unknown_attachment_shape("a\nAnhang: 1 Foto"), None);
    }

    #[test]
    fn round_trips_composed_body() {
        let m = parse(std::str::from_utf8(&compose("+1555", "line one\nline two")).unwrap());
        assert_eq!(m.body, "line one\nline two");
        assert_eq!(m.folder, "telecom/msg/outbox");
    }
}
