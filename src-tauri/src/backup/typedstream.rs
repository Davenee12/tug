//! The words of a text whose `text` column is empty, from its `attributedBody`.
//!
//! Newer iOS versions often leave `message.text` NULL and keep the words only in
//! `attributedBody`: an `NSAttributedString` archived in Apple's old "typedstream" format. tug
//! needs just the plain string, so rather than a full typedstream decoder this finds the archived
//! `NSString` and reads the one string that follows it:
//!
//! ```text
//! 04 0B "streamtyped" 81 E8 03 84 01 40 84 84 84 "NSAttributedString" ... "NSString" 01 94 84 01
//! 2B <length> <UTF-8 bytes> ...
//! ```
//!
//! `2B` (`+`) introduces the string's bytes. Its length is a typedstream integer: a byte below
//! 0x80 is the length itself, `81` is followed by a 16-bit little-endian length and `82` by a
//! 32-bit one. Everything is bounds-checked: a short, cut-off or unexpected stream gives `None`,
//! never a panic or a garbled string.

/// The class names that come right before the string's bytes (mutable first: it doesn't contain
/// "NSString", so it must be looked for in its own right).
const STRING_CLASSES: [&[u8]; 2] = [b"NSString", b"NSMutableString"];

/// How far past the class name the `+` may sit (the bytes between are the class's version and
/// object markers, a handful in every stream seen).
const MAX_GAP: usize = 24;

/// The typedstream marker that starts a string's bytes.
const STRING_MARKER: u8 = 0x2B;

/// The plain text archived in an `attributedBody`, or `None` when there's none to find.
pub fn extract_text(stream: &[u8]) -> Option<String> {
    STRING_CLASSES.iter().find_map(|class| {
        let at = find(stream, class)?;
        read_string_after(stream, at + class.len())
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn read_string_after(stream: &[u8], start: usize) -> Option<String> {
    let window = stream.get(start..)?;
    let gap = window.iter().take(MAX_GAP).position(|&b| b == STRING_MARKER)?;
    let mut at = start + gap + 1;
    let (len, used) = read_length(stream.get(at..)?)?;
    at += used;
    let bytes = stream.get(at..at.checked_add(len)?)?;
    let text = String::from_utf8(bytes.to_vec()).ok()?;
    Some(text)
}

/// A typedstream length: `(value, bytes used)`. Negative or oversized lengths are refused.
fn read_length(b: &[u8]) -> Option<(usize, usize)> {
    match *b.first()? {
        0x81 => {
            let v = i16::from_le_bytes([*b.get(1)?, *b.get(2)?]);
            Some((usize::try_from(v).ok()?, 3))
        }
        0x82 => {
            let v = i32::from_le_bytes([*b.get(1)?, *b.get(2)?, *b.get(3)?, *b.get(4)?]);
            Some((usize::try_from(v).ok()?, 5))
        }
        n if n < 0x80 => Some((n as usize, 1)),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A synthetic attributedBody as iOS writes one: header, class chain, the string, and a tail
    /// of attribute runs (which must be ignored).
    pub(crate) fn stream_for(text: &str) -> Vec<u8> {
        let mut s = vec![0x04, 0x0B];
        s.extend_from_slice(b"streamtyped");
        s.extend_from_slice(&[0x81, 0xE8, 0x03, 0x84, 0x01, 0x40, 0x84, 0x84, 0x84, 0x12]);
        s.extend_from_slice(b"NSAttributedString");
        s.extend_from_slice(&[0x00, 0x84, 0x84, 0x08]);
        s.extend_from_slice(b"NSObject");
        s.extend_from_slice(&[0x00, 0x85, 0x92, 0x84, 0x84, 0x84, 0x08]);
        s.extend_from_slice(b"NSString");
        s.extend_from_slice(&[0x01, 0x94, 0x84, 0x01, STRING_MARKER]);
        let bytes = text.as_bytes();
        match bytes.len() {
            n if n < 0x80 => s.push(n as u8),
            n if n <= i16::MAX as usize => {
                s.push(0x81);
                s.extend_from_slice(&(n as i16).to_le_bytes());
            }
            n => {
                s.push(0x82);
                s.extend_from_slice(&(n as i32).to_le_bytes());
            }
        }
        s.extend_from_slice(bytes);
        s.extend_from_slice(&[0x86, 0x84, 0x02, 0x69, 0x49, 0x01, 0x07, 0x92, 0x84, 0x84, 0x84, 0x0C]);
        s.extend_from_slice(b"NSDictionary");
        s.extend_from_slice(&[0x00, 0x94, 0x84, 0x01, 0x69, 0x01, 0x92, 0x84, 0x96, 0x96, 0x1D]);
        s.extend_from_slice(b"__kIMMessagePartAttributeName");
        s.extend_from_slice(&[0x86, 0x92, 0x84, 0x84, 0x84, 0x08]);
        s.extend_from_slice(b"NSNumber");
        s
    }

    #[test]
    fn reads_a_short_text() {
        assert_eq!(
            extract_text(&stream_for("see you at 5")).as_deref(),
            Some("see you at 5")
        );
    }

    #[test]
    fn reads_a_long_text_with_a_two_byte_length() {
        let long = "on my way, traffic is awful ".repeat(20);
        assert!(long.len() > 0x80);
        assert_eq!(extract_text(&stream_for(&long)).as_deref(), Some(long.as_str()));
    }

    #[test]
    fn reads_a_very_long_text_with_a_four_byte_length() {
        let huge = "x".repeat(40_000);
        assert_eq!(extract_text(&stream_for(&huge)).map(|t| t.len()), Some(40_000));
    }

    #[test]
    fn keeps_emoji_and_other_scripts() {
        let text = "dinner? 🍝 très bien, 明天见";
        assert_eq!(extract_text(&stream_for(text)).as_deref(), Some(text));
    }

    #[test]
    fn an_empty_string_is_empty() {
        assert_eq!(extract_text(&stream_for("")).as_deref(), Some(""));
    }

    #[test]
    fn a_mutable_string_is_found_too() {
        let mut s = stream_for("hello");
        let at = find(&s, b"NSString").unwrap();
        s.splice(at..at + 8, b"NSMutableString".iter().copied());
        assert_eq!(extract_text(&s).as_deref(), Some("hello"));
    }

    #[test]
    fn cut_off_or_garbled_streams_give_nothing() {
        let text = "this text is cut off";
        let full = stream_for(text);
        let end = find(&full, text.as_bytes()).unwrap() + text.len();
        // Every truncation either still holds the whole string or gives nothing: never a panic,
        // never half a string.
        for cut in 0..full.len() {
            let got = extract_text(&full[..cut]);
            if cut < end {
                assert_eq!(got, None, "cut at {cut}");
            } else {
                assert_eq!(got.as_deref(), Some(text), "cut at {cut}");
            }
        }
        assert_eq!(extract_text(b""), None);
        assert_eq!(extract_text(b"NSString"), None);
        assert_eq!(extract_text(b"no class here at all"), None);
        // A length that runs past the end.
        let mut long = b"NSString\x01\x94\x84\x01\x2B\x81\xFF\x7F".to_vec();
        long.extend_from_slice(b"short");
        assert_eq!(extract_text(&long), None);
        // A negative 16-bit length.
        assert_eq!(extract_text(b"NSString\x01\x2B\x81\x00\x80abc"), None);
        // An unknown length tag.
        assert_eq!(extract_text(b"NSString\x01\x2B\x90abc"), None);
        // Bytes that aren't UTF-8.
        assert_eq!(extract_text(b"NSString\x01\x2B\x02\xFF\xFE"), None);
    }

    #[test]
    fn a_marker_too_far_from_the_class_is_not_trusted() {
        let mut s = b"NSString".to_vec();
        s.extend(std::iter::repeat_n(0x84, MAX_GAP + 4));
        s.extend_from_slice(b"\x2B\x02hi");
        assert_eq!(extract_text(&s), None);
    }
}
