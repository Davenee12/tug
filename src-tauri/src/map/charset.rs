//! Turning the bytes of a message body into text without losing it to U+FFFD.
//!
//! tug asks the phone for UTF-8 (MAP `Charset` = UTF-8), and the iPhone normally obliges. When a
//! body isn't valid UTF-8 anyway, decoding it lossily turns every bad byte into U+FFFD and the
//! words are gone for good, so this tries the likely encodings first: a byte-order mark, UTF-16
//! (spotted by its NUL bytes), CESU-8 (an emoji written as two UTF-16 halves), a character cut off
//! at the very end, and finally Windows-1252 for bytes that aren't UTF-8 at all. Which one was
//! needed is reported as a [`Decoding`], so the caller can log it (never the text).

/// How a body's bytes were turned into text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Decoding {
    /// Valid UTF-8, as asked for.
    #[default]
    Utf8,
    /// Valid UTF-8 that the phone itself sent with U+FFFD in it: nothing to recover here.
    Utf8WithReplacement,
    /// UTF-8 behind a byte-order mark (dropped).
    Utf8Bom,
    /// UTF-16, little-endian (byte-order mark or NUL pattern).
    Utf16Le,
    /// UTF-16, big-endian (byte-order mark or NUL pattern).
    Utf16Be,
    /// CESU-8: characters beyond U+FFFF written as two encoded UTF-16 halves.
    Cesu8,
    /// UTF-8 whose last character was cut off (by a length that fell mid-character): dropped.
    Utf8CutShort,
    /// Mostly UTF-8 with a few stray bytes, each read as Windows-1252.
    Utf8AndCp1252,
    /// Not UTF-8 at all: read as Windows-1252.
    Cp1252,
    /// A native SMS PDU (hex), decoded from GSM 7-bit, 8-bit or UCS-2.
    SmsPdu,
    /// A byte or two that aren't text in any encoding: kept as one U+FFFD so the app can say
    /// the text didn't come through, rather than show a stray symbol.
    Unreadable,
}

impl Decoding {
    /// Whether anything other than plain UTF-8 was needed (worth a debug line).
    pub fn is_fallback(self) -> bool {
        !matches!(self, Decoding::Utf8)
    }
}

/// Decode a message body. `bytes` is the text between `BEGIN:MSG` and `END:MSG`.
pub fn decode(bytes: &[u8]) -> (String, Decoding) {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        if let Ok(s) = std::str::from_utf8(rest) {
            return (s.to_string(), Decoding::Utf8Bom);
        }
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        if let Some(s) = utf16(rest, false) {
            return (s, Decoding::Utf16Le);
        }
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        if let Some(s) = utf16(rest, true) {
            return (s, Decoding::Utf16Be);
        }
    }
    // Before UTF-8: ASCII written as UTF-16 is valid UTF-8 too (every other byte NUL).
    if let Some(big_endian) = looks_like_utf16(bytes) {
        if let Some(s) = utf16(bytes, big_endian) {
            return (
                s,
                if big_endian {
                    Decoding::Utf16Be
                } else {
                    Decoding::Utf16Le
                },
            );
        }
    }
    match std::str::from_utf8(bytes) {
        Ok(s) if s.contains('\u{FFFD}') => return (s.to_string(), Decoding::Utf8WithReplacement),
        Ok(s) => return (s.to_string(), Decoding::Utf8),
        Err(e) if e.error_len().is_none() && e.valid_up_to() > 0 && cut_short(bytes, e.valid_up_to()) => {
            // Only the last character is incomplete: everything before it is fine.
            let s = std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or_default();
            return (s.to_string(), Decoding::Utf8CutShort);
        }
        Err(_) => {}
    }
    if let Some(s) = cesu8(bytes) {
        return (s, Decoding::Cesu8);
    }
    let mut out = String::with_capacity(bytes.len());
    let mut any_utf8_multibyte = false;
    let mut any_valid_text = false;
    for chunk in bytes.utf8_chunks() {
        let valid = chunk.valid();
        any_utf8_multibyte |= valid.bytes().any(|b| b >= 0x80);
        any_valid_text |= !valid.trim().is_empty();
        out.push_str(valid);
        out.extend(chunk.invalid().iter().map(|&b| cp1252(b)));
    }
    if !any_valid_text && bytes.len() <= 2 {
        // One or two stray bytes and nothing else: not a word in any encoding.
        return ("\u{FFFD}".to_string(), Decoding::Unreadable);
    }
    let how = if any_utf8_multibyte {
        Decoding::Utf8AndCp1252
    } else {
        Decoding::Cp1252
    };
    (out, how)
}

/// Header values (names, folders): the same rules, without the report.
pub fn decode_line(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => decode(bytes).0,
    }
}

/// A content-free description of a body's bytes for the log: its length, any byte-order mark,
/// how many bytes are NUL or above 0x7F, and the classes of the first 8 bytes
/// (`a` ASCII text, `c` control, `0` NUL, `t` UTF-8 continuation 80–BF, `l` UTF-8 lead C2–F4,
/// `x` never valid in UTF-8). Never the bytes themselves.
pub fn byte_pattern(bytes: &[u8]) -> String {
    let bom = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        " bom=utf8"
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        " bom=utf16le"
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        " bom=utf16be"
    } else {
        ""
    };
    let nul = bytes.iter().filter(|&&b| b == 0).count();
    let high = bytes.iter().filter(|&&b| b >= 0x80).count();
    let first: String = bytes
        .iter()
        .take(8)
        .map(|&b| match b {
            0 => '0',
            0x09 | 0x0A | 0x0D | 0x20..=0x7E => 'a',
            0x01..=0x1F | 0x7F => 'c',
            0x80..=0xBF => 't',
            0xC2..=0xF4 => 'l',
            _ => 'x',
        })
        .collect();
    format!("{} bytes{bom} nul={nul} high={high} first={first}", bytes.len())
}

/// Whether an incomplete last character (from `valid_up_to` on) is UTF-8 cut short rather than
/// one Windows-1252 letter: two or more bytes of a sequence, or one lead byte after text that is
/// already UTF-8 beyond ASCII. A lone high byte after plain ASCII ("caf" + E9) is more likely
/// "café" in Windows-1252 than half a character.
fn cut_short(bytes: &[u8], valid_up_to: usize) -> bool {
    bytes.len() - valid_up_to >= 2 || bytes[..valid_up_to].iter().any(|&b| b >= 0x80)
}

/// `Some(big_endian)` when the bytes have UTF-16's tell: an even length and NUL bytes on one side
/// of most pairs but not the other (Latin text in UTF-16 is half NULs).
fn looks_like_utf16(bytes: &[u8]) -> Option<bool> {
    if bytes.len() < 2 || !bytes.len().is_multiple_of(2) {
        return None;
    }
    let pairs = bytes.len() / 2;
    let nul_even = bytes.iter().step_by(2).filter(|&&b| b == 0).count();
    let nul_odd = bytes.iter().skip(1).step_by(2).filter(|&&b| b == 0).count();
    // At least 30% of pairs NUL on one side, (almost) none on the other.
    let many = |n: usize| n * 10 >= pairs * 3;
    let few = |n: usize| n * 10 < pairs;
    if many(nul_odd) && few(nul_even) {
        Some(false)
    } else if many(nul_even) && few(nul_odd) {
        Some(true)
    } else {
        None
    }
}

fn utf16(bytes: &[u8], big_endian: bool) -> Option<String> {
    let (pairs, rest) = bytes.as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    let units: Vec<u16> = pairs
        .iter()
        .map(|&p| {
            if big_endian {
                u16::from_be_bytes(p)
            } else {
                u16::from_le_bytes(p)
            }
        })
        .collect();
    String::from_utf16(&units).ok()
}

/// CESU-8 / "modified UTF-8": like UTF-8, but a character beyond U+FFFF is two three-byte
/// sequences, one per UTF-16 surrogate (ED A0–AF xx, ED B0–BF xx). None unless the bytes are
/// exactly that (and contain at least one such pair).
fn cesu8(bytes: &[u8]) -> Option<String> {
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len());
    let mut saw_surrogate = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let cont = |k: usize| {
            bytes
                .get(i + k)
                .filter(|&&c| (0x80..=0xBF).contains(&c))
                .map(|&c| (c & 0x3F) as u32)
        };
        let cp = match b {
            0x00..=0x7F => {
                i += 1;
                b as u32
            }
            0xC0..=0xDF => {
                let c1 = cont(1)?;
                i += 2;
                ((b as u32 & 0x1F) << 6) | c1
            }
            0xE0..=0xEF => {
                let (c1, c2) = (cont(1)?, cont(2)?);
                i += 3;
                ((b as u32 & 0x0F) << 12) | (c1 << 6) | c2
            }
            _ => return None,
        };
        if (0xD800..=0xDFFF).contains(&cp) {
            saw_surrogate = true;
        }
        units.push(cp as u16);
    }
    if !saw_surrogate {
        return None;
    }
    String::from_utf16(&units).ok()
}

/// One byte as Windows-1252 (WHATWG's mapping: the five unassigned bytes stay C1 controls).
pub fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}', '\u{02C6}',
        '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008D}', '\u{017D}', '\u{008F}', '\u{0090}', '\u{2018}',
        '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}', '\u{02DC}', '\u{2122}', '\u{0161}',
        '\u{203A}', '\u{0153}', '\u{009D}', '\u{017E}', '\u{0178}',
    ];
    match b {
        0x80..=0x9F => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16le(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
    }

    fn utf16be(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
    }

    #[test]
    fn plain_utf8_and_emoji_pass_straight_through() {
        assert_eq!(
            decode("see you at 6".as_bytes()),
            ("see you at 6".into(), Decoding::Utf8)
        );
        let s = "Lunch? 🍕👍🏽 café 👨‍👩‍👧";
        assert_eq!(decode(s.as_bytes()), (s.into(), Decoding::Utf8));
        assert!(!Decoding::Utf8.is_fallback());
    }

    #[test]
    fn a_replacement_character_the_phone_sent_is_kept_and_flagged() {
        let (s, how) = decode("\u{FFFD}".as_bytes());
        assert_eq!(s, "\u{FFFD}");
        assert_eq!(how, Decoding::Utf8WithReplacement);
        assert!(how.is_fallback(), "worth a log line, so the next one can be traced");
    }

    #[test]
    fn utf8_with_a_byte_order_mark() {
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice("hi 👋".as_bytes());
        assert_eq!(decode(&b), ("hi 👋".into(), Decoding::Utf8Bom));
    }

    #[test]
    fn utf16_with_and_without_a_byte_order_mark() {
        let text = "Running late, 10 min 🚗";
        let mut le = vec![0xFF, 0xFE];
        le.extend(utf16le(text));
        assert_eq!(decode(&le), (text.into(), Decoding::Utf16Le));
        let mut be = vec![0xFE, 0xFF];
        be.extend(utf16be(text));
        assert_eq!(decode(&be), (text.into(), Decoding::Utf16Be));
        // No mark: the NUL bytes give it away.
        assert_eq!(decode(&utf16le(text)), (text.into(), Decoding::Utf16Le));
        assert_eq!(decode(&utf16be(text)), (text.into(), Decoding::Utf16Be));
    }

    #[test]
    fn a_character_cut_off_at_the_end_is_dropped_not_replaced() {
        let full = "ok see you 😊".as_bytes();
        let cut = &full[..full.len() - 2]; // half of the emoji's four bytes
        assert_eq!(decode(cut), ("ok see you ".into(), Decoding::Utf8CutShort));
        let accent = "👋 café".as_bytes();
        assert_eq!(
            decode(&accent[..accent.len() - 1]),
            ("👋 caf".into(), Decoding::Utf8CutShort)
        );
        // One high byte after plain ASCII is a Windows-1252 letter, not half a character.
        assert_eq!(decode(&[0x63, 0x61, 0x66, 0xE9]), ("café".into(), Decoding::Cp1252));
    }

    #[test]
    fn cesu8_emoji_are_joined_back_together() {
        // 😀 U+1F600 = surrogates D83D DE00, each written as three bytes.
        let mut b = b"hi ".to_vec();
        b.extend_from_slice(&[0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80]);
        b.extend_from_slice(" é".as_bytes());
        assert_eq!(decode(&b), ("hi 😀 é".into(), Decoding::Cesu8));
    }

    #[test]
    fn windows_1252_text_reads_as_words() {
        // "Café – 5€" in Windows-1252.
        let b = [0x43, 0x61, 0x66, 0xE9, 0x20, 0x96, 0x20, 0x35, 0x80];
        assert_eq!(decode(&b), ("Café – 5€".into(), Decoding::Cp1252));
        // "naïve" with ï as one Latin-1 byte.
        assert_eq!(
            decode(&[0x6E, 0x61, 0xEF, 0x76, 0x65]),
            ("naïve".into(), Decoding::Cp1252)
        );
    }

    #[test]
    fn a_stray_byte_inside_utf8_keeps_the_rest() {
        // Proper UTF-8 "👍 " then a lone Windows-1252 "é" mid-word.
        let mut b = "👍 caf".as_bytes().to_vec();
        b.push(0xE9);
        b.extend_from_slice(b" ok");
        assert_eq!(decode(&b), ("👍 café ok".into(), Decoding::Utf8AndCp1252));
    }

    #[test]
    fn a_lone_stray_byte_is_unreadable_not_a_random_symbol() {
        assert_eq!(decode(&[0x9D]), ("\u{FFFD}".into(), Decoding::Unreadable));
        assert_eq!(
            decode(&[0xFF, 0xFE, 0x00]).1,
            Decoding::Cp1252,
            "odd length: not UTF-16"
        );
    }

    #[test]
    fn header_lines_decode_the_same_way() {
        assert_eq!(decode_line("Zoë".as_bytes()), "Zoë");
        assert_eq!(decode_line(&[0x5A, 0x6F, 0xEB]), "Zoë");
    }

    #[test]
    fn byte_pattern_describes_without_content() {
        let p = byte_pattern("hey".as_bytes());
        assert_eq!(p, "3 bytes nul=0 high=0 first=aaa");
        assert_eq!(byte_pattern("\u{FFFD}".as_bytes()), "3 bytes nul=0 high=3 first=ltt");
        let mut le = vec![0xFF, 0xFE];
        le.extend(utf16le("ab"));
        assert_eq!(byte_pattern(&le), "6 bytes bom=utf16le nul=2 high=2 first=xxa0a0");
        assert!(!byte_pattern(b"secret words").contains("secret"));
    }
}
