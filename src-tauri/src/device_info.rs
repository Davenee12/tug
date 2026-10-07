//! The iPhone's Device Information Service (Bluetooth SIG 0x180A): its Model Number String
//! (0x2A24) is Apple's model identifier, e.g. "iPhone16,2" for an iPhone 15 Pro Max. tug reads it
//! once per connection so the sidebar can picture the exact phone. Pure, so the parse is tested
//! here; the read itself lives in `ble/actor` and is optional (a missing service changes nothing).
//! Turning the identifier into a marketing name and a picture happens in `src/lib/phoneModel.ts`.

/// Device Information Service.
pub const DEVICE_INFORMATION_SERVICE: u16 = 0x180A;
/// Model Number String characteristic.
pub const MODEL_NUMBER_STRING: u16 = 0x2A24;

/// Manufacturer Name String characteristic ("Apple Inc." on an iPhone).
pub const MANUFACTURER_NAME_STRING: u16 = 0x2A29;

/// GATT services only Apple devices offer, which an iPhone shows even while ANCS is withheld
/// (locked after a restart): Continuity and Nearby Interaction.
pub const APPLE_ONLY_SERVICES: &[u128] = &[
    0xD0611E78_BBB4_4591_A5F8_487910AE4366,
    0x9FA480E0_4967_4542_9390_D343DC5D04AE,
];

/// A phone with no ANCS: is it plainly not an iPhone (so "unlock your iPhone" would never help)?
/// Conservative: any sign of Apple (a model identifier tug read before, an Apple-only service, an
/// Apple manufacturer name) means it's an iPhone that's locked. Only a non-Apple manufacturer, or
/// no manufacturer and no Apple service at all, says it isn't.
pub fn not_an_iphone(known_model: Option<&str>, manufacturer: Option<&str>, apple_service: bool) -> bool {
    if apple_service || known_model.is_some_and(|m| m.starts_with("iPhone") || m.starts_with("iPad")) {
        return false;
    }
    match manufacturer.map(|m| m.trim().to_lowercase()) {
        Some(m) if !m.is_empty() => !m.contains("apple"),
        _ => true,
    }
}

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
    fn tells_a_locked_iphone_from_a_phone_that_isnt_one() {
        // Any sign of Apple: a locked iPhone.
        assert!(!not_an_iphone(None, None, true));
        assert!(!not_an_iphone(Some("iPhone16,2"), None, false));
        assert!(!not_an_iphone(None, Some("Apple Inc."), false));
        // Another maker, or nothing Apple at all: not an iPhone.
        assert!(not_an_iphone(None, Some("samsung"), false));
        assert!(not_an_iphone(None, Some("Google"), false));
        assert!(not_an_iphone(None, None, false));
        assert!(not_an_iphone(None, Some("  "), false));
    }

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
