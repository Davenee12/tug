//! Classic Bluetooth SDP records: a data-element parser and a privacy-safe description of each
//! record (service classes, profile versions, protocols, supported features). Text attributes
//! (service names, descriptions) are never kept, only their presence. Pure, unit-tested.

use std::collections::BTreeMap;

use serde::Serialize;

use super::gatt::{format_uuid, sig_short};

/// One SDP data element. Text and URL values keep only their length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Element {
    Nil,
    Uint(u128),
    Int(i128),
    Uuid(u128),
    Text(usize),
    Bool(bool),
    Seq(Vec<Element>),
    Alt(Vec<Element>),
    Url(usize),
}

impl Element {
    fn uint(&self) -> Option<u128> {
        match self {
            Element::Uint(v) => Some(*v),
            _ => None,
        }
    }

    fn items(&self) -> &[Element] {
        match self {
            Element::Seq(v) | Element::Alt(v) => v,
            _ => &[],
        }
    }
}

const SIG_BASE: u128 = 0x0000_0000_0000_1000_8000_0080_5F9B_34FB;

/// Parse one data element from the front of `b`; returns it and the bytes it used.
pub fn parse_element(b: &[u8]) -> Result<(Element, usize), String> {
    parse_at_depth(b, 0)
}

fn parse_at_depth(b: &[u8], depth: usize) -> Result<(Element, usize), String> {
    if depth > 16 {
        return Err("SDP nesting too deep".into());
    }
    let &header = b.first().ok_or("empty SDP element")?;
    let (kind, size_index) = (header >> 3, header & 0x07);
    let (len, head) = match size_index {
        0 if kind == 0 => (0, 1),
        0 => (1, 1),
        1 => (2, 1),
        2 => (4, 1),
        3 => (8, 1),
        4 => (16, 1),
        5 => (*b.get(1).ok_or("truncated SDP length")? as usize, 2),
        6 => (
            u16::from_be_bytes([*b.get(1).ok_or("truncated")?, *b.get(2).ok_or("truncated")?]) as usize,
            3,
        ),
        _ => {
            let n = b.get(1..5).ok_or("truncated SDP length")?;
            (u32::from_be_bytes([n[0], n[1], n[2], n[3]]) as usize, 5)
        }
    };
    let body = b.get(head..head + len).ok_or("truncated SDP element")?;
    let be = |bytes: &[u8]| bytes.iter().fold(0u128, |acc, &x| (acc << 8) | x as u128);
    let el = match kind {
        0 => Element::Nil,
        1 if len <= 16 => Element::Uint(be(body)),
        2 if len <= 16 && len > 0 => {
            let shift = 128 - 8 * len as u32;
            Element::Int(((be(body) << shift) as i128) >> shift)
        }
        3 => match len {
            2 | 4 => Element::Uuid(SIG_BASE | (be(body) << 96)),
            16 => Element::Uuid(be(body)),
            n => return Err(format!("bad SDP UUID size {n}")),
        },
        4 => Element::Text(len),
        5 => Element::Bool(body.first().is_some_and(|&v| v != 0)),
        6 | 7 => {
            let mut items = Vec::new();
            let mut rest = body;
            while !rest.is_empty() {
                let (item, used) = parse_at_depth(rest, depth + 1)?;
                items.push(item);
                rest = &rest[used..];
            }
            if kind == 6 {
                Element::Seq(items)
            } else {
                Element::Alt(items)
            }
        }
        8 => Element::Url(len),
        k => return Err(format!("unknown SDP element type {k}")),
    };
    Ok((el, head + len))
}

/// A whole service record: a sequence of (attribute id, value) pairs.
pub fn parse_record(b: &[u8]) -> Result<Vec<(u16, Element)>, String> {
    let (el, _) = parse_element(b)?;
    let Element::Seq(items) = el else {
        return Err("SDP record isn't a sequence".into());
    };
    let mut out = Vec::new();
    for pair in items.chunks(2) {
        match pair {
            [Element::Uint(id), value] if *id <= u16::MAX as u128 => out.push((*id as u16, value.clone())),
            _ => return Err("SDP record has a malformed attribute pair".into()),
        }
    }
    Ok(out)
}

/// A record described for the inventory.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdpRecord {
    pub service_classes: Vec<String>,
    pub profiles: Vec<String>,
    pub protocols: Vec<String>,
    /// The record's SupportedFeatures (0x0311) or MAP/PBAP supported features (0x0317), in hex.
    pub supported_features: Option<String>,
    pub feature_names: Vec<String>,
    /// Other decoded attributes (PBAP repositories, MAP message types, PnP ids, …).
    pub details: BTreeMap<String, String>,
    /// Every attribute id present, in hex.
    pub attribute_ids: Vec<String>,
}

const ATTR_SERVICE_CLASS_ID_LIST: u16 = 0x0001;
const ATTR_PROTOCOL_DESCRIPTOR_LIST: u16 = 0x0004;
const ATTR_PROFILE_DESCRIPTOR_LIST: u16 = 0x0009;
const ATTR_SUPPORTED_FEATURES: u16 = 0x0311;
const ATTR_PBAP_REPOSITORIES: u16 = 0x0314;
const ATTR_MAS_INSTANCE_ID: u16 = 0x0315;
const ATTR_MAP_MESSAGE_TYPES: u16 = 0x0316;
const ATTR_MAP_PBAP_FEATURES: u16 = 0x0317;
const ATTR_GOEP_L2CAP_PSM: u16 = 0x0200;
const ATTR_HFP_NETWORK: u16 = 0x0301;

pub const CLASS_MAS: u16 = 0x1132;
pub const CLASS_PSE: u16 = 0x112F;
const CLASS_AVRCP_TARGET: u16 = 0x110C;
const CLASS_A2DP_SOURCE: u16 = 0x110A;
const CLASS_HFP_AG: u16 = 0x111F;
const CLASS_PNP: u16 = 0x1200;

/// Classic service class / profile / protocol UUID names.
pub fn classic_uuid_name(uuid: u128) -> String {
    let named = sig_short(uuid).and_then(|s| {
        Some(match s {
            0x0001 => "SDP",
            0x0003 => "RFCOMM",
            0x0008 => "OBEX",
            0x000F => "BNEP",
            0x0017 => "AVCTP",
            0x0019 => "AVDTP",
            0x0100 => "L2CAP",
            0x1000 => "Service Discovery Server",
            0x1101 => "Serial Port",
            0x1105 => "OBEX Object Push",
            0x1106 => "OBEX File Transfer",
            0x1108 => "Headset",
            0x110A => "A2DP Audio Source",
            0x110B => "A2DP Audio Sink",
            0x110C => "AVRCP Target",
            0x110D => "A2DP",
            0x110E => "AVRCP",
            0x110F => "AVRCP Controller",
            0x1112 => "Headset Audio Gateway",
            0x1115 => "PAN User",
            0x1116 => "PAN Network Access Point",
            0x1117 => "PAN Group Network",
            0x111E => "Hands-Free",
            0x111F => "Hands-Free Audio Gateway",
            0x1124 => "HID",
            0x112D => "SIM Access",
            0x112E => "PBAP Client",
            0x112F => "PBAP Server",
            0x1130 => "PBAP",
            0x1132 => "MAP Message Access Server",
            0x1133 => "MAP Message Notification Server",
            0x1134 => "MAP",
            0x1200 => "PnP Information",
            0x1203 => "Generic Audio",
            0x1204 => "Generic Telephony",
            _ => return None,
        })
    });
    match named {
        Some(n) => n.to_string(),
        None if uuid == 0x0000_0000_DECA_FADE_DECA_DEAF_DECA_CAFF => "Apple iAP2".to_string(),
        None => format_uuid(uuid),
    }
}

fn version(v: u128) -> String {
    format!("{}.{}", (v >> 8) & 0xFF, v & 0xFF)
}

fn bit_names(bits: u128, names: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = names
        .iter()
        .enumerate()
        .filter(|(i, _)| bits & (1 << i) != 0)
        .map(|(_, n)| n.to_string())
        .collect();
    let unknown = bits >> names.len();
    if unknown != 0 {
        out.push(format!("other bits 0x{:X}", unknown << names.len()));
    }
    out
}

const AVRCP_TG_FEATURES: &[&str] = &[
    "category 1 (player/recorder)",
    "category 2 (monitor/amplifier)",
    "category 3 (tuner)",
    "category 4 (menu)",
    "player application settings",
    "group navigation",
    "browsing",
    "multiple media player applications",
    "cover art",
];
const A2DP_SOURCE_FEATURES: &[&str] = &["player", "microphone", "tuner", "mixer"];
const HFP_AG_FEATURES: &[&str] = &[
    "three-way calling",
    "echo cancelling/noise reduction",
    "voice recognition",
    "in-band ring tone",
    "attach number to voice tag",
    "wide band speech",
    "enhanced voice recognition status",
    "voice recognition text",
    "super wide band speech",
];
const MAP_FEATURES: &[&str] = &[
    "notification registration",
    "notification",
    "browsing",
    "uploading",
    "delete",
    "instance information",
    "extended event report 1.1",
    "event report 1.2",
    "message format 1.1",
    "messages-listing format 1.1",
    "persistent message handles",
    "database identifier",
    "folder version counter",
    "conversation version counters",
    "participant presence change notification",
    "participant chat state change notification",
    "PBAP contact cross reference",
    "notification filtering",
    "UTC offset timestamp format",
    "supported features in connect request",
    "conversation listing",
    "owner status",
    "message forwarding",
];
const PBAP_FEATURES: &[&str] = &[
    "download",
    "browsing",
    "database identifier",
    "folder version counters",
    "vCard selecting",
    "enhanced missed calls",
    "X-BT-UCI vCard property",
    "X-BT-UID vCard property",
    "contact referencing",
    "default contact image format",
];
const MAP_MESSAGE_TYPES: &[&str] = &["EMAIL", "SMS_GSM", "SMS_CDMA", "MMS", "IM"];
const PBAP_REPOSITORIES: &[&str] = &["local phonebook", "SIM card", "speed dial", "favorites"];

/// Describe one parsed record. Only UUIDs, versions, numbers and bitfields are kept.
pub fn describe_record(attrs: &[(u16, Element)]) -> SdpRecord {
    let mut r = SdpRecord {
        attribute_ids: attrs.iter().map(|(id, _)| format!("0x{id:04X}")).collect(),
        ..Default::default()
    };
    let get = |id: u16| attrs.iter().find(|(a, _)| *a == id).map(|(_, v)| v);
    let mut classes: Vec<u16> = Vec::new();
    if let Some(list) = get(ATTR_SERVICE_CLASS_ID_LIST) {
        for el in list.items() {
            if let Element::Uuid(u) = el {
                r.service_classes.push(classic_uuid_name(*u));
                classes.extend(sig_short(*u));
            }
        }
    }
    if let Some(list) = get(ATTR_PROFILE_DESCRIPTOR_LIST) {
        for profile in list.items() {
            if let [Element::Uuid(u), rest @ ..] = profile.items() {
                let v = rest.first().and_then(Element::uint).map(version);
                r.profiles.push(match v {
                    Some(v) => format!("{} {v}", classic_uuid_name(*u)),
                    None => classic_uuid_name(*u),
                });
            }
        }
    }
    if let Some(list) = get(ATTR_PROTOCOL_DESCRIPTOR_LIST) {
        for proto in list.items() {
            if let [Element::Uuid(u), params @ ..] = proto.items() {
                let name = classic_uuid_name(*u);
                let param = params.first().and_then(Element::uint);
                r.protocols.push(match (sig_short(*u), param) {
                    (Some(0x0100), Some(psm)) => format!("{name} psm 0x{psm:04X}"),
                    (Some(0x0003), Some(ch)) => format!("{name} channel {ch}"),
                    (Some(0x0017 | 0x0019 | 0x000F), Some(v)) => format!("{name} {}", version(v)),
                    _ => name,
                });
            }
        }
    }
    let has = |c: u16| classes.contains(&c);
    let features = get(ATTR_MAP_PBAP_FEATURES)
        .filter(|_| has(CLASS_MAS) || has(CLASS_PSE))
        .or_else(|| get(ATTR_SUPPORTED_FEATURES))
        .and_then(Element::uint);
    if let Some(bits) = features {
        r.supported_features = Some(format!("0x{bits:04X}"));
        let table: &[&str] = if has(CLASS_MAS) {
            MAP_FEATURES
        } else if has(CLASS_PSE) {
            PBAP_FEATURES
        } else if has(CLASS_AVRCP_TARGET) {
            AVRCP_TG_FEATURES
        } else if has(CLASS_HFP_AG) {
            HFP_AG_FEATURES
        } else if has(CLASS_A2DP_SOURCE) {
            A2DP_SOURCE_FEATURES
        } else {
            &[]
        };
        if !table.is_empty() {
            r.feature_names = bit_names(bits, table);
        }
    }
    let mut detail = |key: &str, value: String| {
        r.details.insert(key.to_string(), value);
    };
    if has(CLASS_PSE) {
        if let Some(v) = get(ATTR_PBAP_REPOSITORIES).and_then(Element::uint) {
            detail("pbapRepositories", bit_names(v, PBAP_REPOSITORIES).join(", "));
        }
    }
    if has(CLASS_MAS) {
        if let Some(v) = get(ATTR_MAP_MESSAGE_TYPES).and_then(Element::uint) {
            detail("mapMessageTypes", bit_names(v, MAP_MESSAGE_TYPES).join(", "));
        }
        if let Some(v) = get(ATTR_MAS_INSTANCE_ID).and_then(Element::uint) {
            detail("masInstanceId", v.to_string());
        }
    }
    if has(CLASS_HFP_AG) {
        if let Some(v) = get(ATTR_HFP_NETWORK).and_then(Element::uint) {
            detail(
                "hfpNetwork",
                if v == 1 {
                    "can reject calls"
                } else {
                    "can't reject calls"
                }
                .into(),
            );
        }
    }
    if has(CLASS_PNP) {
        for (id, key) in [
            (0x0201, "vendorId"),
            (0x0202, "productId"),
            (0x0203, "version"),
            (0x0205, "vendorIdSource"),
        ] {
            if let Some(v) = get(id).and_then(Element::uint) {
                detail(key, format!("0x{v:04X}"));
            }
        }
    } else if let Some(v) = get(ATTR_GOEP_L2CAP_PSM).and_then(Element::uint) {
        detail("goepL2capPsm", format!("0x{v:04X}"));
    }
    if get(0x0100).is_some() {
        detail("serviceName", "present".into());
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode helpers for building records in tests.
    fn uint16(v: u16) -> Vec<u8> {
        let mut out = vec![0x09];
        out.extend_from_slice(&v.to_be_bytes());
        out
    }
    fn uint32(v: u32) -> Vec<u8> {
        let mut out = vec![0x0A];
        out.extend_from_slice(&v.to_be_bytes());
        out
    }
    fn uint8(v: u8) -> Vec<u8> {
        vec![0x08, v]
    }
    fn uuid16(v: u16) -> Vec<u8> {
        let mut out = vec![0x19];
        out.extend_from_slice(&v.to_be_bytes());
        out
    }
    fn seq(items: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = items.concat();
        let mut out = vec![0x35, body.len() as u8];
        out.extend(body);
        out
    }
    fn text(s: &str) -> Vec<u8> {
        let mut out = vec![0x25, s.len() as u8];
        out.extend_from_slice(s.as_bytes());
        out
    }

    #[test]
    fn parses_elements_of_every_size() {
        assert_eq!(parse_element(&[0x00]).unwrap(), (Element::Nil, 1));
        assert_eq!(parse_element(&uint16(0x0104)).unwrap(), (Element::Uint(0x0104), 3));
        assert_eq!(parse_element(&[0x10, 0xFF]).unwrap(), (Element::Int(-1), 2));
        assert_eq!(parse_element(&[0x28, 0x01]).unwrap(), (Element::Bool(true), 2));
        let (u, _) = parse_element(&uuid16(0x1132)).unwrap();
        assert_eq!(u, Element::Uuid(super::super::gatt::sig(0x1132)));
        // Text keeps only its length.
        assert_eq!(parse_element(&text("Dave's MAP")).unwrap(), (Element::Text(10), 12));
        // 16-bit length sequence.
        let mut long = vec![0x36, 0x00, 0x03];
        long.extend(uint16(7));
        assert_eq!(parse_element(&long).unwrap().0, Element::Seq(vec![Element::Uint(7)]));
        // A 128-bit UUID.
        let mut u128b = vec![0x1C];
        u128b.extend_from_slice(&0x0000_0000_DECA_FADE_DECA_DEAF_DECA_CAFFu128.to_be_bytes());
        let (el, used) = parse_element(&u128b).unwrap();
        assert_eq!(used, 17);
        assert_eq!(el, Element::Uuid(0x0000_0000_DECA_FADE_DECA_DEAF_DECA_CAFF));
    }

    #[test]
    fn rejects_truncated_and_malformed_data() {
        assert!(parse_element(&[]).is_err());
        assert!(parse_element(&[0x09, 0x01]).is_err(), "uint16 missing a byte");
        assert!(
            parse_element(&[0x35, 0x05, 0x09]).is_err(),
            "sequence longer than the data"
        );
        assert!(parse_record(&uint16(3)).is_err(), "a record must be a sequence");
        assert!(parse_record(&seq(&[uint16(1)])).is_err(), "an id without a value");
    }

    #[test]
    fn describes_an_ios_style_mas_record() {
        let record = seq(&[
            uint16(0x0001),
            seq(&[uuid16(0x1132)]),
            uint16(0x0004),
            seq(&[
                seq(&[uuid16(0x0100)]),
                seq(&[uuid16(0x0003), uint8(2)]),
                seq(&[uuid16(0x0008)]),
            ]),
            uint16(0x0009),
            seq(&[seq(&[uuid16(0x1134), uint16(0x0104)])]),
            uint16(0x0100),
            text("MAP MAS-iOS"),
            uint16(0x0315),
            uint8(0),
            uint16(0x0316),
            uint8(0x0A), // SMS_GSM + MMS
            uint16(0x0317),
            uint32(0x0000_007F),
        ]);
        let attrs = parse_record(&record).unwrap();
        let r = describe_record(&attrs);
        assert_eq!(r.service_classes, vec!["MAP Message Access Server"]);
        assert_eq!(r.profiles, vec!["MAP 1.4"]);
        assert_eq!(r.protocols, vec!["L2CAP", "RFCOMM channel 2", "OBEX"]);
        assert_eq!(r.supported_features.as_deref(), Some("0x007F"));
        assert_eq!(r.feature_names.len(), 7);
        assert_eq!(r.feature_names[0], "notification registration");
        assert_eq!(r.details["mapMessageTypes"], "SMS_GSM, MMS");
        assert_eq!(r.details["masInstanceId"], "0");
        assert_eq!(r.details["serviceName"], "present");
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("iOS"), "service name text is never kept: {json}");
    }

    #[test]
    fn describes_avrcp_target_with_version_and_features() {
        let record = seq(&[
            uint16(0x0001),
            seq(&[uuid16(0x110C)]),
            uint16(0x0004),
            seq(&[
                seq(&[uuid16(0x0100), uint16(0x0017)]),
                seq(&[uuid16(0x0017), uint16(0x0104)]),
            ]),
            uint16(0x0009),
            seq(&[seq(&[uuid16(0x110E), uint16(0x0106)])]),
            uint16(0x0311),
            uint16(0x0043), // cat 1, cat 2, browsing
        ]);
        let r = describe_record(&parse_record(&record).unwrap());
        assert_eq!(r.service_classes, vec!["AVRCP Target"]);
        assert_eq!(r.profiles, vec!["AVRCP 1.6"]);
        assert_eq!(r.protocols, vec!["L2CAP psm 0x0017", "AVCTP 1.4"]);
        assert_eq!(
            r.feature_names,
            vec![
                "category 1 (player/recorder)",
                "category 2 (monitor/amplifier)",
                "browsing"
            ]
        );
    }

    #[test]
    fn describes_pbap_hfp_and_pnp_records() {
        let pse = seq(&[
            uint16(0x0001),
            seq(&[uuid16(0x112F)]),
            uint16(0x0314),
            uint8(0x09), // local phonebook + favorites
            uint16(0x0317),
            uint32(0x0000_0201),
        ]);
        let r = describe_record(&parse_record(&pse).unwrap());
        assert_eq!(r.details["pbapRepositories"], "local phonebook, favorites");
        assert_eq!(r.feature_names, vec!["download", "default contact image format"]);

        let ag = seq(&[
            uint16(0x0001),
            seq(&[uuid16(0x111F), uuid16(0x1203)]),
            uint16(0x0301),
            uint8(1),
            uint16(0x0311),
            uint16(0x0400), // a bit beyond the named ones
        ]);
        let r = describe_record(&parse_record(&ag).unwrap());
        assert_eq!(r.service_classes, vec!["Hands-Free Audio Gateway", "Generic Audio"]);
        assert_eq!(r.details["hfpNetwork"], "can reject calls");
        assert_eq!(r.feature_names, vec!["other bits 0x400"]);

        let pnp = seq(&[
            uint16(0x0001),
            seq(&[uuid16(0x1200)]),
            uint16(0x0201),
            uint16(0x004C),
            uint16(0x0205),
            uint16(0x0001),
        ]);
        let r = describe_record(&parse_record(&pnp).unwrap());
        assert_eq!(r.details["vendorId"], "0x004C");
        assert_eq!(r.details["vendorIdSource"], "0x0001");
    }

    #[test]
    fn unknown_uuids_print_in_full() {
        assert_eq!(classic_uuid_name(super::super::gatt::sig(0x110A)), "A2DP Audio Source");
        assert_eq!(
            classic_uuid_name(0x0000_0000_DECA_FADE_DECA_DEAF_DECA_CAFF),
            "Apple iAP2"
        );
        assert_eq!(
            classic_uuid_name(0x1234_5678_9ABC_DEF0_1234_5678_9ABC_DEF0),
            "12345678-9ABC-DEF0-1234-56789ABCDEF0"
        );
    }
}
