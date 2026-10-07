//! The iPhone's Device Information Service (Bluetooth SIG 0x180A): its Model Number String
//! (0x2A24) is Apple's model identifier, e.g. "iPhone16,2" for an iPhone 15 Pro Max. tug reads it
//! once per connection so the sidebar can picture the exact phone. Pure, so the parse is tested
//! here; the read itself lives in `ble/actor` and is optional (a missing service changes nothing).
//! Turning the identifier into a marketing name and a picture happens in `src/lib/phoneModel.ts`.

/// Device Information Service.
pub const DEVICE_INFORMATION_SERVICE: u16 = 0x180A;
/// Model Number String characteristic.
pub const MODEL_NUMBER_STRING: u16 = 0x2A24;

/// Longest identifier tug keeps. Apple's are ~10 characters ("iPhone18,4"); anything far longer
/// isn't an identifier and shouldn't end up in settings or the UI.
const MAX_LEN: usize = 32;

/// The model identifier from a raw Model Number String value, or None when it isn't one.
///
/// The characteristic is a UTF-8 string with no length prefix; some stacks pad it with NULs or
/// spaces, so those are trimmed. Only short ASCII identifiers (letters, digits and `,._-`) are
/// accepted: anything else (garbage, an empty value) is ignored rather than shown to the user.
pub fn parse_model_number(raw: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(raw).ok()?;
    let text = text.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    let valid = !text.is_empty()
        && text.len() <= MAX_LEN
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ',' | '.' | '_' | '-'));
    valid.then(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_an_iphone_identifier() {
        assert_eq!(parse_model_number(b"iPhone16,2").as_deref(), Some("iPhone16,2"));
        assert_eq!(parse_model_number(b"iPhone18,4").as_deref(), Some("iPhone18,4"));
    }

    #[test]
    fn trims_nul_and_space_padding() {
        assert_eq!(parse_model_number(b"iPhone10,3\0\0").as_deref(), Some("iPhone10,3"));
        assert_eq!(parse_model_number(b" iPhone17,5 \n").as_deref(), Some("iPhone17,5"));
    }

    #[test]
    fn keeps_identifiers_tug_does_not_know_yet() {
        // A future phone: the UI falls back to a generic picture, but the identifier is kept.
        assert_eq!(parse_model_number(b"iPhone19,1").as_deref(), Some("iPhone19,1"));
    }

    #[test]
    fn rejects_empty_and_garbage_values() {
        assert_eq!(parse_model_number(b""), None);
        assert_eq!(parse_model_number(b"\0\0\0"), None);
        assert_eq!(parse_model_number(b"   "), None);
        assert_eq!(parse_model_number(&[0xFF, 0xFE, 0x41]), None);
        assert_eq!(parse_model_number(b"<script>"), None);
        assert_eq!(parse_model_number(b"My iPhone"), None);
        assert_eq!(parse_model_number(&[b'A'; 64]), None);
    }
}
