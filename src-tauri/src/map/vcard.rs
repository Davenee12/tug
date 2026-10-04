//! vCard (2.1/3.0) phonebook parsing for PBAP: just names and phone numbers.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhonebookEntry {
    pub name: String,
    pub numbers: Vec<String>,
}

/// Parse a PBAP phonebook object (concatenated vCards). Entries without a name
/// or without a number are skipped; the phone's own "owner" card usually has both.
pub fn parse(raw: &str) -> Vec<PhonebookEntry> {
    let mut out = Vec::new();
    let mut fn_name: Option<String> = None;
    let mut n_name: Option<String> = None;
    let mut numbers: Vec<String> = Vec::new();
    for line in unfold(raw) {
        let Some((key_params, value)) = line.split_once(':') else {
            continue;
        };
        let mut parts = key_params.split(';');
        // Property names may carry a group prefix like "item1.TEL".
        let key = parts
            .next()
            .unwrap_or("")
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        let params: Vec<String> = parts.map(|p| p.to_ascii_uppercase()).collect();
        let value = decode(value, &params);
        match key.as_str() {
            "BEGIN" if value.eq_ignore_ascii_case("VCARD") => {
                fn_name = None;
                n_name = None;
                numbers.clear();
            }
            "FN" if !value.trim().is_empty() => fn_name = Some(value.trim().to_string()),
            "N" => {
                // N:Family;Given;Middle;Prefix;Suffix → "Given Family"
                let f: Vec<&str> = value.split(';').collect();
                let name = [f.get(3), f.get(1), f.get(2), f.first(), f.get(4)]
                    .into_iter()
                    .flatten()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                if !name.is_empty() {
                    n_name = Some(name);
                }
            }
            "TEL" if !value.trim().is_empty() => numbers.push(value.trim().to_string()),
            "END" if value.eq_ignore_ascii_case("VCARD") => {
                if let Some(name) = fn_name.take().or(n_name.take()) {
                    if !numbers.is_empty() {
                        out.push(PhonebookEntry {
                            name,
                            numbers: std::mem::take(&mut numbers),
                        });
                    }
                }
                numbers.clear();
            }
            _ => {}
        }
    }
    out
}

/// Join folded lines (a line starting with space or tab continues the previous one).
fn unfold(raw: &str) -> Vec<String> {
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
        let raw = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Daviel James\r\nN:James;Daviel;;;\r\n\
                   TEL;TYPE=CELL:(214) 223-0313\r\nTEL;TYPE=HOME:+1 972 555 0100\r\nEND:VCARD\r\n\
                   BEGIN:VCARD\r\nVERSION:3.0\r\nFN:tay 🤎\r\nitem1.TEL:+13026698133\r\nEND:VCARD\r\n\
                   BEGIN:VCARD\r\nVERSION:3.0\r\nFN:No Number\r\nEND:VCARD\r\n";
        let got = parse(raw);
        assert_eq!(got.len(), 2, "cards without numbers are skipped");
        assert_eq!(got[0].name, "Daviel James");
        assert_eq!(got[0].numbers, vec!["(214) 223-0313", "+1 972 555 0100"]);
        assert_eq!(got[1].name, "tay 🤎");
        assert_eq!(got[1].numbers, vec!["+13026698133"]);
    }

    #[test]
    fn falls_back_to_n_and_unfolds_and_decodes() {
        let raw = "BEGIN:VCARD\nVERSION:2.1\nN;CHARSET=UTF-8;ENCODING=QUOTED-PRINTABLE:M=C3=BCller;J=C3=BCrgen\nTEL:\n +4930123\nEND:VCARD\n";
        let got = parse(raw);
        assert_eq!(
            got,
            vec![PhonebookEntry {
                name: "Jürgen Müller".into(),
                numbers: vec!["+4930123".into()]
            }]
        );
    }
}
