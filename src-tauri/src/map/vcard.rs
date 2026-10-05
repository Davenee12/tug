//! vCard (2.1/3.0) phonebook parsing for PBAP: names, phone numbers, and contact photos.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhonebookEntry {
    pub name: String,
    pub numbers: Vec<String>,
    /// The contact's photo as raw image bytes, when the phone inlined one (PHOTO). `None` for
    /// contacts without a photo, and for photos given only as a URI (never fetched). Validation
    /// and the size cap happen when the bytes are stored (`crate::contact_photos`).
    pub photo: Option<Vec<u8>>,
}

/// Parse a PBAP phonebook object (concatenated vCards). Entries without a name
/// or without a number are skipped; the phone's own "owner" card usually has both.
pub fn parse(raw: &str) -> Vec<PhonebookEntry> {
    let mut out = Vec::new();
    let mut fn_name: Option<String> = None;
    let mut n_name: Option<String> = None;
    let mut numbers: Vec<String> = Vec::new();
    let mut photo: Option<Vec<u8>> = None;
    for line in unfold(raw) {
        let Some((key, params, value)) = property(&line) else {
            continue;
        };
        match key.as_str() {
            "BEGIN" if value.eq_ignore_ascii_case("VCARD") => {
                fn_name = None;
                n_name = None;
                numbers.clear();
                photo = None;
            }
            "FN" if !value.trim().is_empty() => fn_name = Some(value.trim().to_string()),
            "N" => {
                if let Some(name) = structured_name(&value) {
                    n_name = Some(name);
                }
            }
            "TEL" if !value.trim().is_empty() => numbers.push(value.trim().to_string()),
            // Only the first inline photo on a card is kept (iOS sends one).
            "PHOTO" if photo.is_none() => photo = photo_bytes(&params, &value),
            "END" if value.eq_ignore_ascii_case("VCARD") => {
                if let Some(name) = fn_name.take().or(n_name.take()) {
                    if !numbers.is_empty() {
                        out.push(PhonebookEntry {
                            name,
                            numbers: std::mem::take(&mut numbers),
                            photo: photo.take(),
                        });
                    }
                }
                numbers.clear();
                photo = None;
            }
            _ => {}
        }
    }
    out
}

/// Decode an inline PHOTO value to image bytes. vCard 2.1 writes `PHOTO;ENCODING=BASE64;TYPE=JPEG:`
/// (folded over several space-indented lines, closed by a blank line), 3.0 writes
/// `PHOTO;ENCODING=b;TYPE=JPEG:`. Photos given as a URI (`VALUE=URI`, or an http/data value) are
/// skipped — tug never reaches out to fetch one. Returns `None` when there's nothing usable to decode.
fn photo_bytes(params: &[String], value: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let is_uri = params.iter().any(|p| p == "VALUE=URI")
        || value.starts_with("http://")
        || value.starts_with("https://")
        || value.starts_with("data:");
    if is_uri {
        return None;
    }
    let base64 = params
        .iter()
        .any(|p| matches!(p.as_str(), "ENCODING=BASE64" | "BASE64" | "ENCODING=B" | "B"));
    if !base64 {
        return None;
    }
    // Folding can leave spaces mid-value; base64 has none of its own, so drop all whitespace.
    let cleaned: String = value.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(cleaned)
        .ok()
        .filter(|b| !b.is_empty())
}

/// One content line as (upper-cased name, upper-cased params, decoded value).
/// Property names may carry a group prefix like "item1.TEL"; it's dropped.
pub(super) fn property(line: &str) -> Option<(String, Vec<String>, String)> {
    let (key_params, value) = line.split_once(':')?;
    let mut parts = key_params.split(';');
    let key = parts
        .next()
        .unwrap_or("")
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    let params: Vec<String> = parts.map(|p| p.to_ascii_uppercase()).collect();
    let value = decode(value, &params);
    Some((key, params, value))
}

/// `N:Family;Given;Middle;Prefix;Suffix` → "Given Family" (and whatever else is there).
pub(super) fn structured_name(value: &str) -> Option<String> {
    let f: Vec<&str> = value.split(';').collect();
    let name = [f.get(3), f.get(1), f.get(2), f.first(), f.get(4)]
        .into_iter()
        .flatten()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!name.is_empty()).then_some(name)
}

/// Join folded lines (a line starting with space or tab continues the previous one).
pub(super) fn unfold(raw: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for line in raw.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)) {
        if let (Some(rest), Some(prev)) = (line.strip_prefix([' ', '\t']), lines.last_mut()) {
            prev.push_str(rest);
        } else {
            lines.push(line.to_string());
        }
    }
    lines
}

/// vCard 2.1 may quoted-printable-encode non-ASCII values; 3.0 escapes `\,` etc.
fn decode(value: &str, params: &[String]) -> String {
    let qp = params
        .iter()
        .any(|p| p == "ENCODING=QUOTED-PRINTABLE" || p == "QUOTED-PRINTABLE");
    let text = if qp {
        let bytes = value.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'=' && i + 2 < bytes.len() {
                if let Ok(b) = u8::from_str_radix(&value[i + 1..i + 3], 16) {
                    out.push(b);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    } else {
        value.to_string()
    };
    text.replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\n", " ")
        .replace("\\\\", "\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vcard30_entries() {
        let raw = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Priya Shah\r\nN:Shah;Priya;;;\r\n\
                   TEL;TYPE=CELL:(214) 555-0186\r\nTEL;TYPE=HOME:+1 972 555 0100\r\nEND:VCARD\r\n\
                   BEGIN:VCARD\r\nVERSION:3.0\r\nFN:zoe 💜\r\nitem1.TEL:+13025550173\r\nEND:VCARD\r\n\
                   BEGIN:VCARD\r\nVERSION:3.0\r\nFN:No Number\r\nEND:VCARD\r\n";
        let got = parse(raw);
        assert_eq!(got.len(), 2, "cards without numbers are skipped");
        assert_eq!(got[0].name, "Priya Shah");
        assert_eq!(got[0].numbers, vec!["(214) 555-0186", "+1 972 555 0100"]);
        assert_eq!(got[0].photo, None);
        assert_eq!(got[1].name, "zoe 💜");
        assert_eq!(got[1].numbers, vec!["+13025550173"]);
    }

    #[test]
    fn falls_back_to_n_and_unfolds_and_decodes() {
        let raw = "BEGIN:VCARD\nVERSION:2.1\nN;CHARSET=UTF-8;ENCODING=QUOTED-PRINTABLE:M=C3=BCller;J=C3=BCrgen\nTEL:\n +4930123\nEND:VCARD\n";
        let got = parse(raw);
        assert_eq!(
            got,
            vec![PhonebookEntry {
                name: "Jürgen Müller".into(),
                numbers: vec!["+4930123".into()],
                photo: None,
            }]
        );
    }

    // "Hi" as JPEG-ish and PNG-ish bytes, base64-encoded, for the photo fixtures below.
    // Base64("Hi") == "SGk=".
    #[test]
    fn parses_vcard30_inline_photo() {
        let raw = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Pic Person\r\nTEL:+13025550000\r\n\
                   PHOTO;ENCODING=b;TYPE=JPEG:SGk=\r\nEND:VCARD\r\n";
        let got = parse(raw);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].photo.as_deref(), Some(&b"Hi"[..]));
    }

    #[test]
    fn parses_vcard21_folded_base64_photo_ended_by_blank_line() {
        // vCard 2.1 folds the base64 over space-indented lines and closes the block with a blank
        // line; the TEL after it must still be read. "SGVsbG8gdHVn" == "Hello tug".
        let raw = "BEGIN:VCARD\n\
                   VERSION:2.1\n\
                   FN:Folded Photo\n\
                   PHOTO;ENCODING=BASE64;TYPE=JPEG:SGVs\n bG8g\n dHVn\n\
                   \n\
                   TEL:+13025551234\n\
                   END:VCARD\n";
        let got = parse(raw);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "Folded Photo");
        assert_eq!(got[0].numbers, vec!["+13025551234"]);
        assert_eq!(got[0].photo.as_deref(), Some(&b"Hello tug"[..]));
    }

    #[test]
    fn ignores_uri_photos() {
        let uri = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Linked\r\nTEL:+1302\r\n\
                   PHOTO;VALUE=URI:https://example.com/a.jpg\r\nEND:VCARD\r\n";
        assert_eq!(parse(uri)[0].photo, None);
        let bare = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Linked2\r\nTEL:+1302\r\n\
                    PHOTO:https://example.com/b.jpg\r\nEND:VCARD\r\n";
        assert_eq!(parse(bare)[0].photo, None);
    }
}
