//! GATT naming and the decoders for the standard services the inventory reads: Device
//! Information (0x180A), Current Time (0x1805) and Battery (0x180F). Pure, unit-tested.

use serde::Serialize;

use super::safe_text;

/// The Bluetooth SIG base UUID; a 16-bit UUID `xxxx` is `0000xxxx-0000-1000-8000-00805F9B34FB`.
const SIG_BASE: u128 = 0x0000_0000_0000_1000_8000_0080_5F9B_34FB;
const SIG_MASK: u128 = 0xFFFF_FFFF_FFFF_FFFF_FFFF_FFFF;

/// The 16-bit SIG short form of `uuid`, if it is one.
pub fn sig_short(uuid: u128) -> Option<u16> {
    (uuid & SIG_MASK == SIG_BASE && uuid >> 112 == 0).then_some((uuid >> 96) as u16)
}

/// A full UUID from a 16-bit SIG one.
pub const fn sig(short: u16) -> u128 {
    SIG_BASE | ((short as u128) << 96)
}

/// `0x180F` for SIG UUIDs, the hyphenated 128-bit form for everything else.
pub fn format_uuid(uuid: u128) -> String {
    match sig_short(uuid) {
        Some(short) => format!("0x{short:04X}"),
        None => {
            let h = format!("{uuid:032X}");
            format!(
                "{}-{}-{}-{}-{}",
                &h[0..8],
                &h[8..12],
                &h[12..16],
                &h[16..20],
                &h[20..32]
            )
        }
    }
}

pub const ANCS_SERVICE: u128 = crate::ancs::SERVICE;
pub const AMS_SERVICE: u128 = crate::ams::SERVICE;
pub const CONTINUITY_SERVICE: u128 = 0xD0611E78_BBB4_4591_A5F8_487910AE4366;
pub const NEARBY_SERVICE: u128 = 0x9FA480E0_4967_4542_9390_D343DC5D04AE;

pub const DEVICE_INFORMATION: u16 = 0x180A;
pub const CURRENT_TIME_SERVICE: u16 = 0x1805;
pub const BATTERY_SERVICE: u16 = 0x180F;

pub const CURRENT_TIME: u16 = 0x2A2B;
pub const LOCAL_TIME_INFO: u16 = 0x2A0F;
pub const REFERENCE_TIME_INFO: u16 = 0x2A14;
pub const BATTERY_LEVEL: u16 = 0x2A19;

/// A name for a primary service the iPhone exposes, when tug knows it.
pub fn service_name(uuid: u128) -> Option<&'static str> {
    if let Some(short) = sig_short(uuid) {
        return Some(match short {
            0x1800 => "Generic Access",
            0x1801 => "Generic Attribute",
            0x1805 => "Current Time",
            0x1806 => "Reference Time Update",
            0x1807 => "Next DST Change",
            0x180A => "Device Information",
            0x180F => "Battery",
            0x1812 => "Human Interface Device",
            0x184E => "Audio Stream Control",
            0x1850 => "Published Audio Capabilities",
            _ => return None,
        });
    }
    Some(match uuid {
        ANCS_SERVICE => "Apple Notification Center (ANCS)",
        AMS_SERVICE => "Apple Media (AMS)",
        CONTINUITY_SERVICE => "Apple Continuity",
        NEARBY_SERVICE => "Apple Nearby (proprietary)",
        _ => return None,
    })
}

/// A name for a characteristic, when tug knows it.
pub fn characteristic_name(uuid: u128) -> Option<&'static str> {
    if let Some(short) = sig_short(uuid) {
        return Some(match short {
            0x2A00 => "Device Name",
            0x2A01 => "Appearance",
            0x2A04 => "Peripheral Preferred Connection Parameters",
            0x2A05 => "Service Changed",
            0x2AA6 => "Central Address Resolution",
            0x2B29 => "Client Supported Features",
            0x2B2A => "Database Hash",
            0x2B3A => "Server Supported Features",
            0x2A19 => "Battery Level",
            0x2A1A => "Battery Power State",
            0x2BE9 => "Battery Critical Status",
            0x2BEA => "Battery Health Status",
            0x2BEB => "Battery Health Information",
            0x2BEC => "Battery Information",
            0x2BED => "Battery Level Status",
            0x2BEE => "Battery Time Status",
            0x2BEF => "Estimated Service Date",
            0x2BF0 => "Battery Energy Status",
            0x2A23 => "System ID",
            0x2A24 => "Model Number",
            0x2A25 => "Serial Number",
            0x2A26 => "Firmware Revision",
            0x2A27 => "Hardware Revision",
            0x2A28 => "Software Revision",
            0x2A29 => "Manufacturer Name",
            0x2A2A => "Regulatory Certification Data",
            0x2A50 => "PnP ID",
            0x2A2B => "Current Time",
            0x2A0F => "Local Time Information",
            0x2A14 => "Reference Time Information",
            0x2A0D => "DST Offset",
            0x2A0E => "Time Zone",
            0x2A11 => "Time with DST",
            0x2A16 => "Time Update Control Point",
            0x2A17 => "Time Update State",
            _ => return None,
        });
    }
    Some(match uuid {
        crate::ancs::NOTIFICATION_SOURCE => "ANCS Notification Source",
        crate::ancs::CONTROL_POINT => "ANCS Control Point",
        crate::ancs::DATA_SOURCE => "ANCS Data Source",
        crate::ams::REMOTE_COMMAND => "AMS Remote Command",
        crate::ams::ENTITY_UPDATE => "AMS Entity Update",
        crate::ams::ENTITY_ATTRIBUTE => "AMS Entity Attribute",
        0x8667556C_9A37_4C91_84ED_54EE27D90049 => "Apple Continuity",
        0xAF0BADB1_5B99_43CD_917A_A77BC549E3CC => "Apple Nearby",
        _ => return None,
    })
}

/// Battery Service characteristics that would tell tug about charging or power state.
pub fn is_power_state_characteristic(short: u16) -> bool {
    matches!(short, 0x2A1A | 0x2BE9 | 0x2BED | 0x2BEE | 0x2BF0)
}

/// WinRT `GattCharacteristicProperties` bits, named.
pub fn property_names(bits: u32) -> Vec<&'static str> {
    const NAMES: [(u32, &str); 10] = [
        (0x001, "broadcast"),
        (0x002, "read"),
        (0x004, "writeWithoutResponse"),
        (0x008, "write"),
        (0x010, "notify"),
        (0x020, "indicate"),
        (0x040, "authenticatedSignedWrites"),
        (0x080, "extendedProperties"),
        (0x100, "reliableWrites"),
        (0x200, "writableAuxiliaries"),
    ];
    NAMES.iter().filter(|(b, _)| bits & b != 0).map(|(_, n)| *n).collect()
}

pub const PROP_READ: u32 = 0x002;
pub const PROP_NOTIFY: u32 = 0x010;
pub const PROP_INDICATE: u32 = 0x020;

/// PnP ID (0x2A50).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PnpId {
    pub vendor_id_source: String,
    pub vendor_id: String,
    pub vendor: Option<String>,
    pub product_id: String,
    pub product_version: String,
}

pub fn decode_pnp_id(b: &[u8]) -> Result<PnpId, String> {
    if b.len() < 7 {
        return Err(format!("PnP ID too short ({} bytes)", b.len()));
    }
    let word = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let (source, vendor) = (b[0], word(1));
    let vendor_id_source = match source {
        1 => "bluetoothSig".to_string(),
        2 => "usbIf".to_string(),
        n => format!("unknown({n})"),
    };
    let vendor_name = match (source, vendor) {
        (1, 0x004C) | (2, 0x05AC) => Some("Apple".to_string()),
        _ => None,
    };
    Ok(PnpId {
        vendor_id_source,
        vendor_id: format!("0x{vendor:04X}"),
        vendor: vendor_name,
        product_id: format!("0x{:04X}", word(3)),
        product_version: format!("0x{:04X}", word(5)),
    })
}

/// A Device Information string characteristic (manufacturer, model, revisions), made safe for
/// the log: control characters dropped, redacted, capped.
pub fn decode_dis_string(b: &[u8]) -> String {
    let text = String::from_utf8_lossy(b);
    safe_text(text.trim_end_matches('\0'), 64)
}

/// Current Time (0x2A2B): Exact Time 256 plus the adjust reason. The phone's *local* time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentTime {
    /// `YYYY-MM-DDTHH:MM:SS`, the phone's local wall-clock time.
    pub local: String,
    pub day_of_week: Option<String>,
    /// Fractions of a second in 1/256 s.
    pub fractions256: u8,
    pub adjust_reasons: Vec<String>,
    #[serde(skip)]
    pub fields: (i32, u32, u32, u32, u32, u32),
}

pub fn decode_current_time(b: &[u8]) -> Result<CurrentTime, String> {
    if b.len() < 9 {
        return Err(format!("Current Time too short ({} bytes)", b.len()));
    }
    let year = u16::from_le_bytes([b[0], b[1]]) as i32;
    let (month, day, hour, minute, second) = (b[2] as u32, b[3] as u32, b[4] as u32, b[5] as u32, b[6] as u32);
    if year < 1582 || !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 59
    {
        return Err("Current Time has an unknown or invalid date".into());
    }
    let day_of_week = match b[7] {
        1 => Some("monday"),
        2 => Some("tuesday"),
        3 => Some("wednesday"),
        4 => Some("thursday"),
        5 => Some("friday"),
        6 => Some("saturday"),
        7 => Some("sunday"),
        _ => None,
    }
    .map(str::to_string);
    let reason = b.get(9).copied().unwrap_or(0);
    let adjust_reasons = [
        (0x01, "manualTimeUpdate"),
        (0x02, "externalReferenceTimeUpdate"),
        (0x04, "timeZoneChange"),
        (0x08, "dstChange"),
    ]
    .iter()
    .filter(|(bit, _)| reason & bit != 0)
    .map(|(_, n)| n.to_string())
    .collect();
    Ok(CurrentTime {
        local: format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}"),
        day_of_week,
        fractions256: b[8],
        adjust_reasons,
        fields: (year, month, day, hour, minute, second),
    })
}

/// Local Time Information (0x2A0F): the phone's time zone and daylight-saving offset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalTimeInfo {
    /// Standard offset from UTC in minutes (15-minute steps); None if the phone says unknown.
    pub time_zone_minutes: Option<i32>,
    /// Daylight-saving offset in minutes; None if unknown.
    pub dst_offset_minutes: Option<i32>,
}

impl LocalTimeInfo {
    /// Total offset from UTC, when both parts are known.
    pub fn utc_offset_minutes(&self) -> Option<i32> {
        Some(self.time_zone_minutes? + self.dst_offset_minutes?)
    }
}

pub fn decode_local_time_info(b: &[u8]) -> Result<LocalTimeInfo, String> {
    if b.len() < 2 {
        return Err(format!("Local Time Information too short ({} bytes)", b.len()));
    }
    let tz = b[0] as i8;
    let time_zone_minutes = (tz != -128 && (-48..=56).contains(&tz)).then(|| tz as i32 * 15);
    let dst_offset_minutes = match b[1] {
        0 => Some(0),
        2 => Some(30),
        4 => Some(60),
        8 => Some(120),
        _ => None,
    };
    Ok(LocalTimeInfo {
        time_zone_minutes,
        dst_offset_minutes,
    })
}

/// Reference Time Information (0x2A14): where the phone's clock came from and how fresh it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceTimeInfo {
    pub source: String,
    /// Accuracy in 1/8 s; None when out of range or unknown (see `accuracy`).
    pub accuracy_eighths: Option<u8>,
    pub accuracy: String,
    /// 255 means "255 or more".
    pub days_since_update: u8,
    pub hours_since_update: u8,
}

pub fn decode_reference_time_info(b: &[u8]) -> Result<ReferenceTimeInfo, String> {
    if b.len() < 4 {
        return Err(format!("Reference Time Information too short ({} bytes)", b.len()));
    }
    let source = match b[0] {
        0 => "unknown".to_string(),
        1 => "networkTimeProtocol".to_string(),
        2 => "gps".to_string(),
        3 => "radioTimeSignal".to_string(),
        4 => "manual".to_string(),
        5 => "atomicClock".to_string(),
        6 => "cellularNetwork".to_string(),
        7 => "notSynchronized".to_string(),
        n => format!("reserved({n})"),
    };
    let (accuracy_eighths, accuracy) = match b[1] {
        254 => (None, "outOfRange".to_string()),
        255 => (None, "unknown".to_string()),
        n => (Some(n), format!("{:.3} s", n as f64 / 8.0)),
    };
    Ok(ReferenceTimeInfo {
        source,
        accuracy_eighths,
        accuracy,
        days_since_update: b[2],
        hours_since_update: b[3],
    })
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = month as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The phone's clock as Unix seconds (UTC), from its local time and its UTC offset.
pub fn phone_unix_seconds(t: &CurrentTime, utc_offset_minutes: i32) -> i64 {
    let (y, mo, d, h, mi, s) = t.fields;
    let local = days_from_civil(y, mo, d) * 86_400 + (h * 3600 + mi * 60 + s) as i64;
    local - utc_offset_minutes as i64 * 60
}

/// Phone clock minus PC clock, in seconds (positive: the phone is ahead).
pub fn clock_skew_seconds(t: &CurrentTime, utc_offset_minutes: i32, pc_unix_seconds: i64) -> i64 {
    phone_unix_seconds(t, utc_offset_minutes) - pc_unix_seconds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sig_and_vendor_uuids() {
        assert_eq!(format_uuid(sig(0x180F)), "0x180F");
        assert_eq!(sig_short(sig(0x2A2B)), Some(0x2A2B));
        assert_eq!(format_uuid(ANCS_SERVICE), "7905F431-B5CE-4E99-A40F-4B1E122D00D0");
        assert_eq!(sig_short(ANCS_SERVICE), None);
        assert_eq!(service_name(sig(0x1805)), Some("Current Time"));
        assert_eq!(service_name(AMS_SERVICE), Some("Apple Media (AMS)"));
        assert_eq!(characteristic_name(crate::ancs::DATA_SOURCE), Some("ANCS Data Source"));
        assert_eq!(service_name(0x1234_5678_0000_0000_0000_0000_0000_0000), None);
    }

    #[test]
    fn names_property_bits() {
        assert_eq!(property_names(0x12), vec!["read", "notify"]);
        assert_eq!(property_names(0x28), vec!["write", "indicate"]);
        assert!(property_names(0).is_empty());
    }

    #[test]
    fn decodes_an_apple_pnp_id() {
        // Bluetooth SIG source, vendor 0x004C (Apple), product 0x7A11, version 0x0100.
        let p = decode_pnp_id(&[0x01, 0x4C, 0x00, 0x11, 0x7A, 0x00, 0x01]).unwrap();
        assert_eq!(p.vendor_id_source, "bluetoothSig");
        assert_eq!(p.vendor.as_deref(), Some("Apple"));
        assert_eq!(p.vendor_id, "0x004C");
        assert_eq!(p.product_id, "0x7A11");
        assert_eq!(p.product_version, "0x0100");
        assert!(decode_pnp_id(&[1, 2, 3]).is_err());
    }

    #[test]
    fn dis_strings_are_trimmed_and_redacted() {
        assert_eq!(decode_dis_string(b"iPhone16,2"), "iPhone16,2");
        assert_eq!(decode_dis_string(b"Apple Inc.\0"), "Apple Inc.");
        assert_eq!(decode_dis_string(b"call 3025550142"), "call [number]");
    }

    #[test]
    fn decodes_current_time_and_its_adjust_reasons() {
        // 2026-10-06 14:05:09, Tuesday, 128/256 s, reason: external reference + time zone change.
        let b = [0xEA, 0x07, 10, 6, 14, 5, 9, 2, 128, 0x06];
        let t = decode_current_time(&b).unwrap();
        assert_eq!(t.local, "2026-10-06T14:05:09");
        assert_eq!(t.day_of_week.as_deref(), Some("tuesday"));
        assert_eq!(t.fractions256, 128);
        assert_eq!(t.adjust_reasons, vec!["externalReferenceTimeUpdate", "timeZoneChange"]);
        assert!(
            decode_current_time(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_err(),
            "unknown date"
        );
        assert!(decode_current_time(&[1, 2]).is_err());
    }

    #[test]
    fn decodes_local_time_info() {
        // UTC-5 (−20 quarter hours) plus daylight time (+1 h): New York in summer.
        let lt = decode_local_time_info(&[(-20i8) as u8, 4]).unwrap();
        assert_eq!(lt.time_zone_minutes, Some(-300));
        assert_eq!(lt.dst_offset_minutes, Some(60));
        assert_eq!(lt.utc_offset_minutes(), Some(-240));
        // India: +5:30 (22 quarter hours), no DST.
        assert_eq!(
            decode_local_time_info(&[22, 0]).unwrap().utc_offset_minutes(),
            Some(330)
        );
        let unknown = decode_local_time_info(&[0x80, 255]).unwrap();
        assert_eq!(unknown.time_zone_minutes, None);
        assert_eq!(unknown.utc_offset_minutes(), None);
    }

    #[test]
    fn decodes_reference_time_info() {
        let r = decode_reference_time_info(&[6, 8, 0, 3]).unwrap();
        assert_eq!(r.source, "cellularNetwork");
        assert_eq!(r.accuracy_eighths, Some(8));
        assert_eq!(r.accuracy, "1.000 s");
        assert_eq!(r.hours_since_update, 3);
        assert_eq!(decode_reference_time_info(&[1, 255, 0, 0]).unwrap().accuracy, "unknown");
        assert!(decode_reference_time_info(&[1]).is_err());
    }

    #[test]
    fn computes_clock_skew_from_local_time_and_offset() {
        // 2026-10-06T10:00:00 local at UTC-4 is 14:00:00 UTC = 1791295200.
        let t = decode_current_time(&[0xEA, 0x07, 10, 6, 10, 0, 0, 2, 0, 0]).unwrap();
        assert_eq!(phone_unix_seconds(&t, -240), 1_791_295_200);
        assert_eq!(clock_skew_seconds(&t, -240, 1_791_295_200 - 3), 3, "phone 3 s ahead");
        assert_eq!(clock_skew_seconds(&t, -240, 1_791_295_200 + 2), -2, "phone 2 s behind");
        // The epoch itself.
        let epoch = decode_current_time(&[0xB2, 0x07, 1, 1, 0, 0, 0, 4, 0, 0]).unwrap();
        assert_eq!(phone_unix_seconds(&epoch, 0), 0);
    }

    #[test]
    fn spots_power_state_characteristics() {
        assert!(is_power_state_characteristic(0x2A1A));
        assert!(is_power_state_characteristic(0x2BED));
        assert!(
            !is_power_state_characteristic(0x2A19),
            "the level alone says nothing about charging"
        );
    }
}
