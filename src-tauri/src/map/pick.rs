//! Choosing which paired Classic device is the iPhone tug reads texts from.
//!
//! A phone can be renamed, and the Classic (texts) and LE (notifications) names don't
//! always agree, so matching by name alone can latch onto the wrong device after a rename.
//! Once a MAP connection succeeds we remember that phone by its stable Bluetooth id and
//! prefer it next time; name matching and the lone-device guess stay as fallbacks for the
//! first run and for when the remembered phone isn't around. Pure and unit-tested; the
//! Windows device lookup lives in `session`.

use std::collections::HashSet;

use crate::state::DeviceKind;

/// A paired Classic device that offers a Message Access Server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapDevice {
    pub id: String,
    pub name: String,
}

/// An unpaired Classic device found while pairing for texts from inside tug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnpairedDevice {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
}

/// A paired Bluetooth LE device, used to spot the one that came with a just-paired Classic iPhone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeCandidate {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
}

/// After pairing an iPhone's Classic side from tug, Windows derives a Bluetooth LE bond for the
/// same phone (cross-transport key derivation). Pick that LE device from the paired ones.
///
/// Pure logic (the WinRT enumeration lives in the actor). `before_ids` are the LE devices already
/// bonded when pairing started, so a device missing from it appeared *because of* this pairing.
/// The Classic and LE sides don't always share a name (the LE side often reports a bare "iPhone"),
/// so a lone newly-bonded phone is the main signal and the name match is a bonus. Never an
/// accessory, and never a guess among several; `None` means fall back to the address mapping.
pub fn choose_le_after_classic<'a>(
    candidates: &'a [LeCandidate],
    classic_name: &str,
    before_ids: &HashSet<String>,
) -> Option<&'a LeCandidate> {
    let is_new = |c: &LeCandidate| !before_ids.contains(&c.id);
    // New since pairing, same name as the Classic phone, not an accessory: the strongest signal.
    if let Some(c) = candidates
        .iter()
        .filter(|c| is_new(c) && c.kind != DeviceKind::Accessory)
        .find(|c| c.name == classic_name)
    {
        return Some(c);
    }
    // Exactly one phone appeared since pairing: it's the LE bond cross-transport derivation made.
    let new_phones: Vec<&LeCandidate> = candidates
        .iter()
        .filter(|c| is_new(c) && c.kind == DeviceKind::Phone)
        .collect();
    if new_phones.len() == 1 {
        return Some(new_phones[0]);
    }
    // Nothing newly bonded resolved; a name match among all non-accessory LE devices is a safe
    // last resort (e.g. the bond existed from a half-finished earlier attempt).
    candidates
        .iter()
        .filter(|c| c.kind != DeviceKind::Accessory)
        .find(|c| c.name == classic_name)
}

/// Choose which unpaired Classic device to pair for texts, given the name of the LE phone already
/// adopted. Pure logic (the WinRT inquiry lives in the actor): prefer the device whose name
/// matches the notifications phone and isn't an accessory, then a lone phone; never guess among
/// several. Keeps tug from pairing a speaker or a second phone by mistake.
pub fn choose_texts_candidate<'a>(devices: &'a [UnpairedDevice], le_name: &str) -> Option<&'a UnpairedDevice> {
    if let Some(d) = devices
        .iter()
        .find(|d| d.name == le_name && d.kind != DeviceKind::Accessory)
    {
        return Some(d);
    }
    let phones: Vec<&UnpairedDevice> = devices.iter().filter(|d| d.kind == DeviceKind::Phone).collect();
    if phones.len() == 1 {
        return Some(phones[0]);
    }
    None
}

/// Pick the iPhone from the paired Classic devices, preferring the remembered one.
///
/// Order: the device whose id was remembered from a previous connection (if it's still
/// present), then one whose name matches the notifications phone, then a lone device
/// (never one of several, which would be a guess). `None` means nothing safe to pick.
pub fn choose_device<'a>(
    devices: &'a [MapDevice],
    stored_id: Option<&str>,
    wanted_name: Option<&str>,
) -> Option<&'a MapDevice> {
    if let Some(id) = stored_id {
        if let Some(d) = devices.iter().find(|d| d.id == id) {
            return Some(d);
        }
    }
    if let Some(name) = wanted_name {
        if let Some(d) = devices.iter().find(|d| d.name == name) {
            return Some(d);
        }
    }
    if devices.len() == 1 {
        return devices.first();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str, name: &str) -> MapDevice {
        MapDevice {
            id: id.into(),
            name: name.into(),
        }
    }

    #[test]
    fn prefers_the_remembered_id_over_a_name_match() {
        // The phone was renamed: its id still identifies it, its old name now sits on
        // a different device. The id must win so texts don't follow the name.
        let devices = vec![dev("id-phone", "new name"), dev("id-other", "old name")];
        let pick = choose_device(&devices, Some("id-phone"), Some("old name"));
        assert_eq!(pick, Some(&devices[0]));
    }

    #[test]
    fn falls_back_to_name_when_the_stored_id_is_absent() {
        let devices = vec![dev("id-a", "Dave's iPhone"), dev("id-b", "Watch")];
        let pick = choose_device(&devices, Some("id-gone"), Some("Dave's iPhone"));
        assert_eq!(pick, Some(&devices[0]));
    }

    #[test]
    fn falls_back_to_name_with_no_stored_id() {
        let devices = vec![dev("id-a", "iPhone"), dev("id-b", "Speaker")];
        let pick = choose_device(&devices, None, Some("iPhone"));
        assert_eq!(pick, Some(&devices[0]));
    }

    #[test]
    fn picks_a_lone_device_when_nothing_else_matches() {
        let devices = vec![dev("id-only", "Mystery")];
        let pick = choose_device(&devices, Some("id-gone"), Some("not here"));
        assert_eq!(pick, Some(&devices[0]));
    }

    #[test]
    fn never_guesses_among_several() {
        let devices = vec![dev("id-a", "A"), dev("id-b", "B")];
        assert_eq!(choose_device(&devices, Some("id-gone"), Some("missing")), None);
        assert_eq!(choose_device(&devices, None, None), None);
    }

    #[test]
    fn nothing_to_pick_from_is_none() {
        assert_eq!(choose_device(&[], Some("id"), Some("name")), None);
    }

    #[test]
    fn a_lone_device_is_used_even_without_hints() {
        let devices = vec![dev("id-only", "iPhone")];
        assert_eq!(choose_device(&devices, None, None), Some(&devices[0]));
    }

    fn un(id: &str, name: &str, kind: DeviceKind) -> UnpairedDevice {
        UnpairedDevice {
            id: id.into(),
            name: name.into(),
            kind,
        }
    }

    #[test]
    fn texts_pick_prefers_the_name_match() {
        let devices = vec![
            un("spk", "Kitchen Speaker", DeviceKind::Accessory),
            un("phone", "Dave's iPhone", DeviceKind::Phone),
        ];
        assert_eq!(choose_texts_candidate(&devices, "Dave's iPhone"), Some(&devices[1]));
    }

    #[test]
    fn texts_pick_never_takes_an_accessory_even_on_a_name_match() {
        // A renamed accessory that happens to match the phone's name must not be paired for texts.
        let devices = vec![un("x", "Dave's iPhone", DeviceKind::Accessory)];
        assert_eq!(choose_texts_candidate(&devices, "Dave's iPhone"), None);
    }

    #[test]
    fn texts_pick_falls_back_to_a_lone_phone() {
        let devices = vec![
            un("phone", "iPhone", DeviceKind::Phone),
            un("kbd", "Keychron", DeviceKind::Accessory),
        ];
        assert_eq!(choose_texts_candidate(&devices, "Dave's iPhone"), Some(&devices[0]));
    }

    #[test]
    fn texts_pick_never_guesses_among_several_phones() {
        let devices = vec![
            un("a", "A iPhone", DeviceKind::Phone),
            un("b", "B iPhone", DeviceKind::Phone),
        ];
        assert_eq!(choose_texts_candidate(&devices, "C iPhone"), None);
    }

    #[test]
    fn texts_pick_is_none_when_nothing_fits() {
        assert_eq!(choose_texts_candidate(&[], "iPhone"), None);
        let devices = vec![un("u", "Mystery", DeviceKind::Unknown)];
        assert_eq!(choose_texts_candidate(&devices, "iPhone"), None);
    }

    fn le(id: &str, name: &str, kind: DeviceKind) -> LeCandidate {
        LeCandidate {
            id: id.into(),
            name: name.into(),
            kind,
        }
    }

    fn before(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn le_after_classic_prefers_a_new_name_match() {
        // A phone bonded before (same name on an old entry) must not win over the fresh one.
        let devices = vec![
            le("old", "Dave's iPhone", DeviceKind::Phone),
            le("new", "Dave's iPhone", DeviceKind::Phone),
        ];
        let pick = choose_le_after_classic(&devices, "Dave's iPhone", &before(&["old"]));
        assert_eq!(pick, Some(&devices[1]));
    }

    #[test]
    fn le_after_classic_takes_a_lone_new_phone_when_names_differ() {
        // The LE side reports the bare "iPhone" while the Classic side carried "Dave's iPhone".
        let devices = vec![
            le("kbd", "Keychron", DeviceKind::Accessory),
            le("phone", "iPhone", DeviceKind::Phone),
        ];
        let pick = choose_le_after_classic(&devices, "Dave's iPhone", &before(&["kbd"]));
        assert_eq!(pick, Some(&devices[1]));
    }

    #[test]
    fn le_after_classic_never_takes_an_accessory() {
        // A renamed accessory matching the phone's name must never be adopted for notifications.
        let devices = vec![le("x", "Dave's iPhone", DeviceKind::Accessory)];
        assert_eq!(
            choose_le_after_classic(&devices, "Dave's iPhone", &HashSet::new()),
            None
        );
    }

    #[test]
    fn le_after_classic_never_guesses_among_several_new_phones() {
        let devices = vec![
            le("a", "A iPhone", DeviceKind::Phone),
            le("b", "B iPhone", DeviceKind::Phone),
        ];
        assert_eq!(choose_le_after_classic(&devices, "C iPhone", &HashSet::new()), None);
    }

    #[test]
    fn le_after_classic_falls_back_to_a_name_match_even_if_not_new() {
        // Nothing appeared since pairing (an earlier attempt already made the bond), but a
        // non-accessory name match is still safe to adopt.
        let devices = vec![le("phone", "Dave's iPhone", DeviceKind::Phone)];
        let pick = choose_le_after_classic(&devices, "Dave's iPhone", &before(&["phone"]));
        assert_eq!(pick, Some(&devices[0]));
    }

    #[test]
    fn le_after_classic_is_none_when_nothing_fits() {
        assert_eq!(choose_le_after_classic(&[], "iPhone", &HashSet::new()), None);
        // A lone phone that was already bonded and whose name doesn't match isn't a safe guess.
        let devices = vec![le("old", "Work iPhone", DeviceKind::Phone)];
        assert_eq!(
            choose_le_after_classic(&devices, "Dave's iPhone", &before(&["old"])),
            None
        );
    }
}
