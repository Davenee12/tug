//! Choosing which paired Classic device is the iPhone tug reads texts from.
//!
//! A phone can be renamed, and the Classic (texts) and LE (notifications) names don't
//! always agree, so matching by name alone can latch onto the wrong device after a rename.
//! Once a MAP connection succeeds we remember that phone by its stable Bluetooth id and
//! prefer it next time; name matching and the lone-device guess stay as fallbacks for the
//! first run and for when the remembered phone isn't around. Pure and unit-tested; the
//! Windows device lookup lives in `session`.

/// A paired Classic device that offers a Message Access Server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapDevice {
    pub id: String,
    pub name: String,
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
}
