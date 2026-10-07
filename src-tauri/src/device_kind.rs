//! Telling a phone from the keyboards, mice and headphones Windows also lists: what the device
//! says about itself (BLE Appearance, Classic Class of Device), then its name as a fallback.

use crate::state::DeviceKind;

/// BLE Appearance category (the value's top 10 bits) for phones.
const APPEARANCE_PHONE: u16 = 1;
/// Classic Class of Device major classes: phone, and the two that say nothing.
const COD_MAJOR_PHONE: u32 = 2;
const COD_MAJOR_MISC: u32 = 0;
const COD_MAJOR_UNCATEGORIZED: u32 = 0x1F;

/// Lowercase name fragments that only accessories use. "le-" is how headphones name their
/// Bluetooth LE side ("LE-Bose Flex", "LE-WH-1000XM5").
const ACCESSORY_WORDS: &[&str] = &[
    "keyboard",
    "keychron",
    "mouse",
    "trackpad",
    "trackball",
    "mx master",
    "mx keys",
    "logi",
    "headphone",
    "headset",
    "earbud",
    "buds",
    "airpods",
    "beats",
    "bose",
    "jbl",
    "sony wh",
    "wh-1000",
    "speaker",
    "soundbar",
    "watch",
    "controller",
    "gamepad",
    "xbox",
    "dualsense",
    "pencil",
    "remote",
    "tile",
    "airtag",
];

/// Whether a name from a Bluetooth NameChanged event is worth adopting. iOS briefly reports
/// junk mid-rename (a test phone flashed up as "4" once), and a one- or two-character name is
/// never a real iPhone name; keep the last good one instead of following it.
pub fn plausible_device_name(name: &str) -> bool {
    name.trim().chars().count() >= 3
}

/// Whether `candidate` is a better phone name to show than `current`. The LE side often reports
/// the bare generic "iPhone", while the Classic side carries the real "Jordan's iPhone"; prefer a
/// specific, plausible candidate over a generic or empty current name, but never overwrite an
/// already-specific name.
pub fn more_specific_name(current: &str, candidate: &str) -> bool {
    let candidate = candidate.trim();
    if !plausible_device_name(candidate) || is_generic_name(candidate) {
        return false;
    }
    is_generic_name(current.trim())
}

/// A placeholder/default name that any real one should replace.
fn is_generic_name(name: &str) -> bool {
    name.is_empty() || name.eq_ignore_ascii_case("iphone") || name.eq_ignore_ascii_case("unnamed device")
}

pub fn classify(name: &str, appearance: Option<u16>, cod_major: Option<u32>) -> DeviceKind {
    let name = name.to_lowercase();
    let category = appearance.map(|a| a >> 6).filter(|&c| c != 0);
    let major = cod_major.filter(|&m| m != COD_MAJOR_MISC && m != COD_MAJOR_UNCATEGORIZED);
    if category == Some(APPEARANCE_PHONE) || major == Some(COD_MAJOR_PHONE) || name.contains("iphone") {
        return DeviceKind::Phone;
    }
    if category.is_some()
        || major.is_some()
        || name.starts_with("le-")
        || ACCESSORY_WORDS.iter().any(|w| name.contains(w))
    {
        return DeviceKind::Accessory;
    }
    DeviceKind::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keyboard_is_never_the_phone() {
        // 0x03C1: HID keyboard.
        assert_eq!(classify("Keychron K3", Some(0x03C1), None), DeviceKind::Accessory);
        assert_eq!(classify("Keychron K3", None, None), DeviceKind::Accessory);
        assert_eq!(classify("K3", Some(0x03C1), None), DeviceKind::Accessory);
    }

    #[test]
    fn phones_by_appearance_class_or_name() {
        assert_eq!(classify("Unnamed device", Some(0x0040), None), DeviceKind::Phone);
        assert_eq!(classify("Pocket", None, Some(2)), DeviceKind::Phone);
        assert_eq!(classify("Jordan's iPhone", None, None), DeviceKind::Phone);
        // A phone signal wins over a misleading word in a renamed phone.
        assert_eq!(classify("Watch this iPhone", None, None), DeviceKind::Phone);
    }

    #[test]
    fn headphones_and_other_classes_are_accessories() {
        assert_eq!(classify("LE-Bose Flex", None, None), DeviceKind::Accessory);
        // Class 4: audio/video. 0x00C0: watch.
        assert_eq!(classify("Thing", None, Some(4)), DeviceKind::Accessory);
        assert_eq!(classify("Thing", Some(0x00C0), None), DeviceKind::Accessory);
    }

    #[test]
    fn implausible_short_names_are_rejected() {
        // iOS flashed "4" mid-rename; one- and two-character names are never a real iPhone.
        assert!(!plausible_device_name("4"));
        assert!(!plausible_device_name("ab"));
        assert!(!plausible_device_name("  x "));
        assert!(!plausible_device_name(""));
        // Real names (and a trimmed three-plus) are kept.
        assert!(plausible_device_name("Jordan's iPhone"));
        assert!(plausible_device_name("iPhone"));
        assert!(plausible_device_name(" Pro "));
    }

    #[test]
    fn prefers_a_specific_name_over_the_generic_one() {
        // The LE side gives "iPhone"; the Classic side has the real name.
        assert!(more_specific_name("iPhone", "Jordan's iPhone"));
        assert!(more_specific_name("", "Jordan's iPhone"));
        assert!(more_specific_name("Unnamed device", "Jordan's iPhone"));
        // Don't downgrade a real name, don't swap one real name for another, don't take junk.
        assert!(!more_specific_name("Jordan's iPhone", "iPhone"));
        assert!(!more_specific_name("Jordan's iPhone", "Work iPhone"));
        assert!(!more_specific_name("iPhone", "iPhone"));
        assert!(!more_specific_name("iPhone", "4"));
    }

    #[test]
    fn nothing_known_stays_unknown() {
        // A just-connected iPhone often has no name or appearance yet.
        assert_eq!(classify("Unnamed device", None, None), DeviceKind::Unknown);
        assert_eq!(classify("Jordan's phone", Some(0), Some(0x1F)), DeviceKind::Unknown);
    }
}
