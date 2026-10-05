//! MAP-msg-listing (MAP 1.4 §3.1.6): the folder index returned by GetMessagesListing.
//!
//! The format is flat — one self-closing `<msg …/>` element per message — so a small
//! attribute scanner is enough and avoids pulling in an XML crate.

use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListedMessage {
    pub handle: String,
    pub subject: String,
    /// `yyyyMMddTHHmmss`, the phone's local time.
    pub datetime: String,
    pub sender_name: String,
    pub sender_addressing: String,
    pub msg_type: String,
    pub read: bool,
}

pub fn parse(xml: &str) -> Vec<ListedMessage> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<msg") {
        let after = &rest[start + 4..];
        // Must be the element `msg`, not e.g. `<msgx`.
        if !after.starts_with(|c: char| c.is_whitespace() || c == '/' || c == '>') {
            rest = after;
            continue;
        }
        let Some(end) = tag_end(after) else { break };
        let attrs = attributes(&after[..end]);
        let get = |k: &str| attrs.get(k).cloned().unwrap_or_default();
        if let Some(handle) = attrs.get("handle") {
            out.push(ListedMessage {
                handle: handle.clone(),
                subject: get("subject"),
                datetime: get("datetime"),
                sender_name: get("sender_name"),
                sender_addressing: get("sender_addressing"),
                msg_type: get("type"),
                read: get("read").eq_ignore_ascii_case("yes"),
            });
        }
        rest = &after[end..];
    }
    out
}

/// Index of the `>` that closes a tag, skipping any inside quoted attribute values:
/// XML allows a raw `>` there (`subject="5 > 3"`), and stopping at it would lose every
/// attribute after the subject, read status included.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices() {
        match quote {
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '>' => return Some(i),
            Some(q) if c == q => quote = None,
            _ => {}
        }
    }
    None
}

/// A listing `datetime` as phone-local ISO (`2026-10-04T18:30:12`). MAP allows a
/// UTC offset after the local time (`20261004T183012-0400`) or a `Z`; the local
/// part is kept either way, matching how notification times are stored. Anything
/// else is `None`.
pub fn datetime_to_iso(s: &str) -> Option<String> {
    let s = s.trim();
    let local = s.get(..15)?;
    let zone = &s[15..];
    let zone_ok = match zone.as_bytes() {
        [] | [b'Z'] => true,
        [b'+' | b'-', rest @ ..] => rest.len() == 4 && rest.iter().all(u8::is_ascii_digit),
        _ => false,
    };
    zone_ok.then(|| crate::ancs::ancs_date_to_iso(local)).flatten()
}

/// `key="value"` / `key='value'` pairs, entity-decoded.
fn attributes(s: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut rest = s;
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim().trim_start_matches('/').trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let Some(close) = after[1..].find(quote) else { break };
        let key = key.rsplit(char::is_whitespace).next().unwrap_or(&key).to_string();
        map.insert(key, decode_entities(&after[1..1 + close]));
        rest = &after[close + 2..];
    }
    map
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let Some(semi) = tail.find(';').filter(|&i| i <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_listing() {
        let xml = r#"<?xml version="1.0"?>
<MAP-msg-listing version = "1.0">
  <msg handle = "20000100001" subject = "omw, 10 mins" datetime = "20261004T183012"
       sender_name = "Tay" sender_addressing = "+15559876543" type = "SMS_GSM" size = "12" read = "no" />
  <msg handle="20000100002" subject="Fish &amp; chips &#x1F35F;" datetime="20261004T120000"
       sender_name='Sam' sender_addressing="sam@icloud.com" type="SMS_GSM" read="yes"/>
</MAP-msg-listing>"#;
        let msgs = parse(xml);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].handle, "20000100001");
        assert_eq!(msgs[0].sender_name, "Tay");
        assert_eq!(msgs[0].sender_addressing, "+15559876543");
        assert!(!msgs[0].read);
        assert_eq!(msgs[1].subject, "Fish & chips 🍟");
        assert_eq!(msgs[1].sender_name, "Sam");
        assert!(msgs[1].read);
    }

    #[test]
    fn a_raw_angle_bracket_in_a_subject_keeps_the_rest_of_the_message() {
        let xml = r#"<MAP-msg-listing>
  <msg handle="1" subject="5 > 3, trust me" datetime="20261004T183012" sender_addressing="+15559876543" read="yes"/>
  <msg handle="2" subject='she said "> ok"' datetime="20261004T183100" read="no"/>
</MAP-msg-listing>"#;
        let msgs = parse(xml);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].subject, "5 > 3, trust me");
        assert_eq!(msgs[0].datetime, "20261004T183012");
        assert_eq!(msgs[0].sender_addressing, "+15559876543");
        assert!(msgs[0].read, "attributes after the subject survive");
        assert_eq!(msgs[1].subject, "she said \"> ok\"");
        assert!(!msgs[1].read);
    }

    #[test]
    fn datetimes_with_or_without_a_zone_keep_their_time() {
        let local = Some("2026-10-04T18:30:12".to_string());
        assert_eq!(datetime_to_iso("20261004T183012"), local);
        assert_eq!(datetime_to_iso("20261004T183012-0400"), local);
        assert_eq!(datetime_to_iso("20261004T183012+0530"), local);
        assert_eq!(datetime_to_iso("20261004T183012Z"), local);
        assert_eq!(datetime_to_iso("20261004T183012-04"), None);
        assert_eq!(datetime_to_iso("20261004T1830"), None);
        assert_eq!(datetime_to_iso("2026-10-04 18:30"), None);
        assert_eq!(datetime_to_iso("20261004T183012-04é0"), None);
    }

    #[test]
    fn ignores_non_msg_elements_and_garbage() {
        assert!(parse("<MAP-msg-listing><msgx handle=\"1\"/></MAP-msg-listing>").is_empty());
        assert!(parse("<msg subject=\"no handle\"/>").is_empty());
        assert_eq!(decode_entities("a & b &bogus; c"), "a & b &bogus; c");
    }
}
