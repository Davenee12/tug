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
        assert_eq!(classify("Dave's iPhone", None, None), DeviceKind::Phone);
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
    fn nothing_known_stays_unknown() {
        // A just-connected iPhone often has no name or appearance yet.
        assert_eq!(classify("Unnamed device", None, None), DeviceKind::Unknown);
        assert_eq!(classify("Dave's phone", Some(0), Some(0x1F)), DeviceKind::Unknown);
    }
}
