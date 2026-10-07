//! Bluetooth inventory: a diagnostic of exactly what the iPhone exposes to tug — its GATT
//! services, Device Information, Current Time, Battery, which AMS attributes it answers, ANCS
//! categories seen, which PBAP phonebooks and vCard fields it sends, MAP folders and event types,
//! its Classic SDP records, and the Windows link details.
//!
//! The report is PRIVACY-SAFE by construction: it only ever holds UUIDs, property flags, field
//! names, counts, enums, lengths and harmless values (time-zone offset, model id, firmware
//! strings, volume, queue counts). Strings that could carry anything personal go through
//! `diagnostics::redact` (`safe_text`). It's logged at INFO as a single line starting with
//! [`LOG_MARKER`] once per app run shortly after the first connection (and again 15 minutes later),
//! returned by the `bt_inventory` command, and included in Copy diagnostics.
//!
//! This module is pure (types, decoders, tallies, formatting) and unit-tested; the WinRT probing
//! lives in `ble::actor::inventory` (BLE) and `map::probe` (Classic SDP, PBAP, MAP).

pub mod gatt;
pub mod sdp;
pub mod tally;

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use serde::Serialize;

pub use gatt::{CurrentTime, LocalTimeInfo, PnpId, ReferenceTimeInfo};
pub use sdp::SdpRecord;
pub use tally::{AncsTally, VcardFieldCounts};

/// Greppable prefix of the one-line report in tug.log.
pub const LOG_MARKER: &str = "bt-inventory:";

/// Bumped when the report's shape changes, so old log lines can be told apart.
pub const SCHEMA_VERSION: u32 = 1;

/// One part of the report: what was found, or why it couldn't be. A failure in one part never
/// affects another (or the connection).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Probe<T> {
    Ok(T),
    Unavailable(String),
}

impl<T> Probe<T> {
    pub fn from_result<E: std::fmt::Display>(r: Result<T, E>) -> Self {
        match r {
            Ok(v) => Probe::Ok(v),
            Err(e) => Probe::Unavailable(safe_text(&e.to_string(), 160)),
        }
    }

    pub fn ok(&self) -> Option<&T> {
        match self {
            Probe::Ok(v) => Some(v),
            Probe::Unavailable(_) => None,
        }
    }
}

/// Text made safe for the report: control characters dropped, phone numbers / emails /
/// Bluetooth addresses masked (`diagnostics::redact`), and capped at `max` characters.
pub fn safe_text(s: &str, max: usize) -> String {
    let clean: String = s.chars().filter(|c| !c.is_control()).collect();
    let redacted = crate::diagnostics::redact(clean.trim());
    if redacted.chars().count() > max {
        let mut cut: String = redacted.chars().take(max).collect();
        cut.push('…');
        cut
    } else {
        redacted
    }
}

/// What triggered a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Trigger {
    /// Shortly after "iPhone services ready".
    Connect,
    /// 15 minutes after the connect report, so ANCS categories have accumulated.
    FollowUp,
    /// Asked for (the `bt_inventory` command).
    OnDemand,
}

/// The whole inventory.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BtInventory {
    pub schema: u32,
    pub trigger: Trigger,
    /// Unix ms when the report was put together.
    pub generated_at: i64,
    pub gatt: Probe<Vec<GattService>>,
    pub device_information: Probe<DeviceInformation>,
    pub current_time: Probe<CurrentTimeReport>,
    pub battery: Probe<BatteryReport>,
    pub ams: Probe<AmsReport>,
    pub ancs: AncsTally,
    pub pbap: Probe<PbapReport>,
    pub map: Probe<MapReport>,
    pub classic_sdp: Probe<Vec<SdpRecord>>,
    pub link: Probe<LinkReport>,
    pub audio_playback: Probe<AudioPlaybackReport>,
}

/// One primary GATT service and its characteristics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GattService {
    pub uuid: String,
    pub name: Option<String>,
    pub characteristics: Probe<Vec<GattCharacteristic>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GattCharacteristic {
    pub uuid: String,
    pub name: Option<String>,
    pub properties: Vec<String>,
    /// For readable characteristics: "N bytes", an error, or why it wasn't read. Values are
    /// never reported here (the decoded sections below hold the safe ones).
    pub read: Option<String>,
}

/// Device Information (0x180A).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInformation {
    pub manufacturer: Option<String>,
    pub model_number: Option<String>,
    /// The serial number identifies the phone: length only.
    pub serial_number_length: Option<usize>,
    pub hardware_revision: Option<String>,
    pub firmware_revision: Option<String>,
    pub software_revision: Option<String>,
    /// System ID identifies the phone: length only.
    pub system_id_length: Option<usize>,
    pub regulatory_data_length: Option<usize>,
    pub pnp_id: Option<PnpId>,
    /// Characteristics present but not readable, with the reason.
    pub unreadable: BTreeMap<String, String>,
}

/// Current Time Service (0x1805).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentTimeReport {
    pub current_time: Probe<CurrentTime>,
    pub current_time_notifies: bool,
    pub local_time_info: Probe<LocalTimeInfo>,
    pub reference_time_info: Probe<ReferenceTimeInfo>,
    /// The phone's offset from UTC (time zone + DST), in minutes.
    pub utc_offset_minutes: Option<i32>,
    /// Phone clock minus PC clock, in seconds (positive: the phone is ahead).
    pub skew_seconds: Option<i64>,
}

/// Battery Service (0x180F).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryReport {
    pub characteristics: Vec<String>,
    pub level: Option<u8>,
    pub level_notifies: bool,
    /// Whether any power-state / charging characteristic exists (iOS isn't expected to have one).
    pub power_state_characteristic: bool,
}

/// What one AMS Entity Attribute read returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AmsValue {
    /// A harmless value (volume, rate, queue index/count, shuffle/repeat, duration).
    Value(String),
    /// Present, with this many bytes (titles, artists, albums, the player's name).
    Present(usize),
    Empty,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmsAttribute {
    pub name: String,
    pub result: AmsValue,
}

/// Apple Media Service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmsReport {
    pub attributes: Vec<AmsAttribute>,
    /// The player's supported remote commands, as last sent by the phone (None: none sent yet).
    pub supported_commands: Option<Vec<String>>,
}

/// One PBAP phonebook asked for its size (MaxListCount = 0: no vCards are transferred).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhonebookAnswer {
    /// The OBEX response code, e.g. "0xA0 success", or the failure (timeout, closed…).
    pub response: String,
    pub size: Option<u16>,
}

/// What `field_counts` covers: the inventory never pulls vCards of its own.
pub const FIELD_COUNTS_SCOPE: &str =
    "only the fields tug's own contacts pulls ask for (name, number, and photo for the photo pass) are counted";

/// Phonebook Access (contacts).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PbapReport {
    /// Which phonebooks answer: pb, ich, och, mch, cch, fav, spd (asked once per run).
    pub phonebooks: BTreeMap<String, PhonebookAnswer>,
    /// Why the phonebook check didn't run (or failed) in this report, when it didn't.
    pub phonebooks_note: Option<String>,
    /// vCard property names per contact, by tug's own pull ("contacts", "contacts+photos").
    pub field_counts: BTreeMap<String, VcardFieldCounts>,
    /// What `field_counts` covers ([`FIELD_COUNTS_SCOPE`]).
    pub field_counts_scope: String,
    /// Unix ms of the inventory's own PBAP probe, if it has run.
    pub probed_at: Option<i64>,
}

/// An OBEX response code, named: `0xC4 not found`.
pub fn obex_response_name(code: u8) -> String {
    let name = match code & 0x7F {
        0x10 => "continue",
        0x20 => "success",
        0x40 => "bad request",
        0x41 => "unauthorized",
        0x43 => "forbidden",
        0x44 => "not found",
        0x46 => "not acceptable",
        0x4F => "unsupported media type",
        0x50 => "internal server error",
        0x51 => "not implemented",
        0x53 => "service unavailable",
        _ => "other",
    };
    format!("0x{code:02X} {name}")
}

/// Message Access (texts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapReport {
    pub folders: Probe<Vec<String>>,
    /// Message types in the latest inbox listing, with counts.
    pub listing_types: BTreeMap<String, u32>,
    /// MNS event types received since tug started.
    pub mns_events: BTreeMap<String, u32>,
    /// The MAS SDP record's supported features, when SDP was read.
    pub sdp_features: Option<Vec<String>>,
    pub sdp_message_types: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionParameters {
    pub interval_ms: f64,
    pub latency: u16,
    pub supervision_timeout_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionPhy {
    pub transmit: String,
    pub receive: String,
}

/// The LE link as Windows sees it (parameters and PHY need Windows 11).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkReport {
    pub parameters: Probe<ConnectionParameters>,
    pub phy: Probe<ConnectionPhy>,
    pub max_pdu_size: Option<u16>,
}

/// Whether this PC could play the phone's audio (A2DP sink). Only availability and enumeration
/// are checked: no connection is opened and no audio is routed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioPlaybackReport {
    pub candidates: usize,
    pub iphone_listed: bool,
}

impl BtInventory {
    /// A report where every probed part is unavailable for the same reason (no Bluetooth backend,
    /// or nothing to probe); the running ANCS tally is still included.
    pub fn unavailable(trigger: Trigger, generated_at: i64, reason: &str) -> Self {
        let why = || reason.to_string();
        BtInventory {
            schema: SCHEMA_VERSION,
            trigger,
            generated_at,
            gatt: Probe::Unavailable(why()),
            device_information: Probe::Unavailable(why()),
            current_time: Probe::Unavailable(why()),
            battery: Probe::Unavailable(why()),
            ams: Probe::Unavailable(why()),
            ancs: ancs_tally(),
            pbap: Probe::Unavailable(why()),
            map: Probe::Unavailable(why()),
            classic_sdp: Probe::Unavailable(why()),
            link: Probe::Unavailable(why()),
            audio_playback: Probe::Unavailable(why()),
        }
    }

    /// The Bluetooth LE link stopped answering during the GATT pass: everything after it (the
    /// Windows link details, audio, the Classic half) is skipped rather than asked of a radio that
    /// isn't answering, and says why. What the pass already found, and the ANCS tally, are kept.
    pub fn skip_after_link_loss(&mut self, why: &str) {
        let why = || why.to_string();
        self.link = Probe::Unavailable(why());
        self.audio_playback = Probe::Unavailable(why());
        self.classic_sdp = Probe::Unavailable(why());
        self.pbap = Probe::Unavailable(why());
        self.map = Probe::Unavailable(why());
    }
}

/// The phonebook size questions stop after this many fail (time out, error, or keep streaming):
/// each failure costs a fresh PBAP connection, and a phone that failed twice won't do better.
pub const MAX_FAILED_PHONEBOOKS: usize = 2;

/// Whether to ask the next phonebook for its size after `failed` failures.
pub fn keep_asking_phonebooks(failed: usize) -> bool {
    failed < MAX_FAILED_PHONEBOOKS
}

/// Why the inventory's own PBAP probe won't run in this report, if it won't. It needs Sync Contacts
/// on (tug's own pull has seen people), isn't run with the Connect report (tug's contacts sync is
/// busy right after connecting), and runs once per run, a failure retried slowly.
pub fn pbap_probe_skip(trigger: Trigger, contacts_shared: bool, due: bool) -> Option<&'static str> {
    if !contacts_shared {
        Some("not asked: contacts aren't shared with this PC (Sync Contacts is off, or tug hasn't pulled them yet)")
    } else if trigger == Trigger::Connect {
        Some("not asked yet: the inventory's phonebook check runs with the 15-minute report")
    } else if !due {
        Some("not asked again: the phonebook check runs once per run (a failure is retried after 30 minutes)")
    } else {
        None
    }
}

/// Connection parameters from WinRT's units (interval 1.25 ms, timeout 10 ms).
pub fn connection_parameters(interval: u16, latency: u16, timeout: u16) -> ConnectionParameters {
    ConnectionParameters {
        interval_ms: interval as f64 * 1.25,
        latency,
        supervision_timeout_ms: timeout as u32 * 10,
    }
}

/// A PHY from WinRT's flags.
pub fn phy_name(uncoded_1m: bool, uncoded_2m: bool, coded: bool) -> String {
    match (uncoded_1m, uncoded_2m, coded) {
        (_, true, _) => "2M",
        (_, _, true) => "Coded",
        (true, _, _) => "1M",
        _ => "unknown",
    }
    .to_string()
}

/// The AMS attributes the inventory asks for, with whether their value is safe to log.
/// (entity, attribute, name, value_safe)
pub const AMS_ATTRIBUTES: [(u8, u8, &str, bool); 11] = [
    (0, 0, "player/name", false),
    (0, 1, "player/playbackInfo", true),
    (0, 2, "player/volume", true),
    (1, 0, "queue/index", true),
    (1, 1, "queue/count", true),
    (1, 2, "queue/shuffleMode", true),
    (1, 3, "queue/repeatMode", true),
    (2, 0, "track/artist", false),
    (2, 1, "track/album", false),
    (2, 2, "track/title", false),
    (2, 3, "track/duration", true),
];

/// How an AMS value is reported: safe attributes show their value only if it's plainly numeric
/// (digits, `.`, `,`, `-`); everything else shows presence and length only.
pub fn ams_value(raw: &[u8], value_safe: bool) -> AmsValue {
    if raw.is_empty() {
        return AmsValue::Empty;
    }
    let text = String::from_utf8_lossy(raw);
    let numeric = text.len() <= 32 && text.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-'));
    if value_safe && numeric {
        AmsValue::Value(text.into_owned())
    } else {
        AmsValue::Present(raw.len())
    }
}

/// The one-line log entry: the marker, then the report as compact JSON.
pub fn log_line(report: &BtInventory) -> String {
    match serde_json::to_string(report) {
        Ok(json) => format!("{LOG_MARKER} {json}"),
        Err(e) => format!("{LOG_MARKER} {{\"error\":\"couldn't serialize: {e}\"}}"),
    }
}

/// The Copy diagnostics section: the latest report as pretty JSON, or a note that none ran yet.
pub fn diagnostics_section(report: Option<&BtInventory>) -> String {
    let mut out = String::from("bluetooth inventory\n-------------------\n");
    match report.map(serde_json::to_string_pretty) {
        Some(Ok(json)) => out.push_str(&json),
        Some(Err(e)) => out.push_str(&format!("(couldn't serialize: {e})")),
        None => out.push_str("(not collected yet: it runs about 30 seconds after the iPhone connects)"),
    }
    out.push('\n');
    out
}

// ---- Process-wide state: tallies fed by small hooks in ANCS/AMS/MAP code, the last report,
// ---- and the once-per-run PBAP probe result.

/// The tallies and caches, shared by the BLE actor, the MAP worker and the commands.
#[derive(Debug, PartialEq)]
pub struct State {
    pub ancs: Option<AncsTally>,
    pub ams_commands: Option<Vec<u8>>,
    pub listing_types: BTreeMap<String, u32>,
    pub mns_events: BTreeMap<String, u32>,
    pub vcard_fields: BTreeMap<String, VcardFieldCounts>,
    pub pbap: Option<PbapReport>,
    /// Unix ms of the last PBAP probe attempt (successful or not), so it isn't retried often.
    pub pbap_attempted_at: Option<i64>,
    pub last_report: Option<BtInventory>,
}

impl State {
    const fn new() -> Self {
        State {
            ancs: None,
            ams_commands: None,
            listing_types: BTreeMap::new(),
            mns_events: BTreeMap::new(),
            vcard_fields: BTreeMap::new(),
            pbap: None,
            pbap_attempted_at: None,
            last_report: None,
        }
    }

    /// Everything here describes one phone: forget it all (the tallies, the PBAP probe result and
    /// its once-per-run mark, vCard counts, the last report) when tug forgets the phone or adopts
    /// another.
    fn reset_device(&mut self) {
        *self = State::new();
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

/// The shared state; a poisoned lock is recovered (it only ever holds plain data).
pub fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// tug forgot the phone or adopted a different one: drop what the inventory knew about the old one.
pub fn reset_device_state() {
    state().reset_device();
}

/// Hook: one ANCS Notification Source packet.
pub fn record_ancs_event(ev: &crate::ancs::NotificationEvent) {
    state()
        .ancs
        .get_or_insert_with(AncsTally::with_all_categories)
        .event(ev);
}

/// Hook: one ANCS notification's details came back.
pub fn record_ancs_details(attrs: &crate::ancs::NotificationAttributes) {
    state()
        .ancs
        .get_or_insert_with(AncsTally::with_all_categories)
        .details(attrs);
}

/// Hook: the AMS player's supported commands (raw RemoteCommandIDs).
pub fn record_ams_commands(data: &[u8]) {
    state().ams_commands = Some(data.to_vec());
}

/// Hook: the message types of the latest MAP inbox listing.
pub fn record_listing_types<'a>(types: impl Iterator<Item = &'a str>) {
    let mut counts = BTreeMap::new();
    for t in types {
        let t = tally::clean_token(t).unwrap_or_else(|| "(blank)".into());
        *counts.entry(t).or_default() += 1;
    }
    state().listing_types = counts;
}

/// Hook: one MNS event report arrived.
pub fn record_mns_event(kind: crate::map::mns_event::EventType) {
    *state().mns_events.entry(format!("{kind:?}")).or_default() += 1;
}

/// Hook: a PBAP phonebook pull came back (`label` says which kind). Only names are counted.
pub fn record_vcard_fields(label: &str, raw: &str) {
    let counts = tally::count_vcard_fields(raw);
    state().vcard_fields.insert(label.to_string(), counts);
}

/// The AMS supported-commands list, named.
pub fn ams_commands() -> Option<Vec<String>> {
    state()
        .ams_commands
        .as_ref()
        .map(|ids| ids.iter().map(|&id| crate::ams::command_label(id)).collect())
}

/// The ANCS tally so far (all zeros before the first notification).
pub fn ancs_tally() -> AncsTally {
    state().ancs.clone().unwrap_or_else(AncsTally::with_all_categories)
}

/// Remember (and return a copy of) the newest report, for Copy diagnostics.
pub fn set_last_report(report: &BtInventory) {
    state().last_report = Some(report.clone());
}

pub fn last_report() -> Option<BtInventory> {
    state().last_report.clone()
}

/// Whether the inventory's own PBAP probe should run now: never yet, or the last attempt found
/// nothing and was at least `retry_ms` ago (so fav/spd aren't retried over and over).
pub fn pbap_probe_due(now_ms: i64, retry_ms: i64) -> bool {
    let s = state();
    probe_due(s.pbap.is_some(), s.pbap_attempted_at, now_ms, retry_ms)
}

fn probe_due(have_result: bool, attempted_at: Option<i64>, now_ms: i64, retry_ms: i64) -> bool {
    match (have_result, attempted_at) {
        (true, _) => false,
        (false, None) => true,
        (false, Some(at)) => now_ms - at >= retry_ms,
    }
}

/// Store the inventory's own PBAP probe result (None: the attempt failed, try again later).
pub fn set_pbap_probe(result: Option<PbapReport>, now_ms: i64) {
    let mut s = state();
    s.pbap_attempted_at = Some(now_ms);
    if result.is_some() {
        s.pbap = result;
    }
}

/// The PBAP part of the report: the cached probe plus every pull's field counts.
pub fn pbap_report() -> Option<PbapReport> {
    let s = state();
    if s.pbap.is_none() && s.vcard_fields.is_empty() {
        return None;
    }
    let mut r = s.pbap.clone().unwrap_or_default();
    r.field_counts = s.vcard_fields.clone();
    r.field_counts_scope = FIELD_COUNTS_SCOPE.to_string();
    Some(r)
}

/// The MAP part from the tallies, with the folders the worker listed and the MAS SDP record.
pub fn map_report(folders: Probe<Vec<String>>, sdp: Option<&[SdpRecord]>) -> MapReport {
    let s = state();
    let mas = sdp
        .unwrap_or(&[])
        .iter()
        .find(|r| r.service_classes.iter().any(|c| c == "MAP Message Access Server"));
    MapReport {
        folders,
        listing_types: s.listing_types.clone(),
        mns_events: s.mns_events.clone(),
        sdp_features: mas.map(|r| r.feature_names.clone()),
        sdp_message_types: mas.and_then(|r| r.details.get("mapMessageTypes").cloned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> BtInventory {
        BtInventory {
            schema: SCHEMA_VERSION,
            trigger: Trigger::Connect,
            generated_at: 1,
            gatt: Probe::Ok(vec![GattService {
                uuid: gatt::format_uuid(gatt::sig(0x180F)),
                name: Some("Battery".into()),
                characteristics: Probe::Ok(vec![GattCharacteristic {
                    uuid: "0x2A19".into(),
                    name: Some("Battery Level".into()),
                    properties: vec!["read".into(), "notify".into()],
                    read: Some("1 bytes".into()),
                }]),
            }]),
            device_information: Probe::Unavailable("not present".into()),
            current_time: Probe::Unavailable("not present".into()),
            battery: Probe::Ok(BatteryReport {
                characteristics: vec!["0x2A19 Battery Level".into()],
                level: Some(80),
                level_notifies: true,
                power_state_characteristic: false,
            }),
            ams: Probe::Ok(AmsReport {
                attributes: vec![AmsAttribute {
                    name: "track/title".into(),
                    result: ams_value(b"Secret Song", false),
                }],
                supported_commands: None,
            }),
            ancs: AncsTally::with_all_categories(),
            pbap: Probe::Unavailable("texts pairing not set up".into()),
            map: Probe::Unavailable("texts pairing not set up".into()),
            classic_sdp: Probe::Unavailable("texts pairing not set up".into()),
            link: Probe::Ok(LinkReport {
                parameters: Probe::Ok(connection_parameters(24, 0, 72)),
                phy: Probe::Unavailable("needs Windows 11".into()),
                max_pdu_size: Some(185),
            }),
            audio_playback: Probe::Ok(AudioPlaybackReport {
                candidates: 1,
                iphone_listed: true,
            }),
        }
    }

    #[test]
    fn log_line_is_one_greppable_json_line() {
        let line = log_line(&sample());
        assert!(line.starts_with("bt-inventory: {"), "{line}");
        assert!(!line.contains('\n'));
        let json: serde_json::Value = serde_json::from_str(line.strip_prefix("bt-inventory: ").unwrap()).unwrap();
        assert_eq!(json["schema"], 1);
        assert_eq!(json["trigger"], "connect");
        assert_eq!(json["battery"]["ok"]["level"], 80);
        assert_eq!(json["deviceInformation"]["unavailable"], "not present");
        assert_eq!(json["link"]["ok"]["parameters"]["ok"]["intervalMs"], 30.0);
        assert_eq!(json["link"]["ok"]["parameters"]["ok"]["supervisionTimeoutMs"], 720);
        assert_eq!(json["ams"]["ok"]["attributes"][0]["result"]["present"], 11);
        assert!(!line.contains("Secret Song"), "titles are length-only");
    }

    #[test]
    fn diagnostics_section_has_the_report_or_says_why_not() {
        let with = diagnostics_section(Some(&sample()));
        assert!(with.starts_with("bluetooth inventory\n"));
        assert!(with.contains("\"trigger\": \"connect\""));
        assert!(diagnostics_section(None).contains("not collected yet"));
    }

    #[test]
    fn ams_values_are_numbers_or_lengths() {
        assert_eq!(ams_value(b"0.5625", true), AmsValue::Value("0.5625".into()));
        assert_eq!(ams_value(b"1,1.0,42.317", true), AmsValue::Value("1,1.0,42.317".into()));
        assert_eq!(
            ams_value(b"Music", true),
            AmsValue::Present(5),
            "non-numeric is length-only"
        );
        assert_eq!(
            ams_value(b"3", false),
            AmsValue::Present(1),
            "unsafe attributes never show values"
        );
        assert_eq!(ams_value(b"", true), AmsValue::Empty);
    }

    #[test]
    fn safe_text_redacts_strips_and_caps() {
        assert_eq!(safe_text("  iOS 26.0\n", 64), "iOS 26.0");
        assert_eq!(safe_text("dave@example.com", 64), "[email]");
        assert_eq!(safe_text("abcdef", 3), "abc…");
    }

    #[test]
    fn link_units_and_phy_names() {
        let p = connection_parameters(12, 4, 500);
        assert_eq!(p.interval_ms, 15.0);
        assert_eq!(p.latency, 4);
        assert_eq!(p.supervision_timeout_ms, 5000);
        assert_eq!(phy_name(true, false, false), "1M");
        assert_eq!(phy_name(false, true, false), "2M");
        assert_eq!(phy_name(false, false, true), "Coded");
        assert_eq!(phy_name(false, false, false), "unknown");
    }

    #[test]
    fn probe_results_carry_redacted_reasons() {
        let p: Probe<u8> = Probe::from_result(Err::<u8, _>("failed for 3025550142"));
        assert_eq!(p, Probe::Unavailable("failed for [number]".into()));
        assert_eq!(Probe::from_result(Ok::<_, String>(3)).ok(), Some(&3));
    }

    #[test]
    fn the_pbap_probe_runs_once_and_retries_failures_slowly() {
        assert!(probe_due(false, None, 0, 100), "never tried");
        assert!(!probe_due(true, Some(0), 1_000, 100), "a result is kept for the run");
        assert!(
            !probe_due(false, Some(950), 1_000, 100),
            "a failure isn't retried at once"
        );
        assert!(probe_due(false, Some(900), 1_000, 100));
    }

    #[test]
    fn a_link_loss_skips_the_rest_but_keeps_what_was_found() {
        let mut r = sample();
        r.skip_after_link_loss("skipped: the link stopped answering");
        assert_eq!(r.link, Probe::Unavailable("skipped: the link stopped answering".into()));
        assert_eq!(
            r.audio_playback,
            Probe::Unavailable("skipped: the link stopped answering".into())
        );
        assert_eq!(
            r.classic_sdp,
            Probe::Unavailable("skipped: the link stopped answering".into())
        );
        assert_eq!(r.pbap, Probe::Unavailable("skipped: the link stopped answering".into()));
        assert_eq!(r.map, Probe::Unavailable("skipped: the link stopped answering".into()));
        assert_eq!(r.gatt, sample().gatt, "the GATT pass's findings stay");
        assert_eq!(r.battery, sample().battery);
        assert_eq!(r.ancs, sample().ancs);
    }

    #[test]
    fn the_phonebook_loop_stops_after_two_failures() {
        assert!(keep_asking_phonebooks(0));
        assert!(keep_asking_phonebooks(1), "one failure: reconnect and go on");
        assert!(!keep_asking_phonebooks(2), "two failures: stop asking");
        assert!(!keep_asking_phonebooks(5));
    }

    #[test]
    fn the_pbap_probe_needs_shared_contacts_and_runs_once() {
        assert!(
            pbap_probe_skip(Trigger::FollowUp, false, true).is_some_and(|w| w.contains("aren't shared")),
            "Sync Contacts off: never asked, whatever the trigger"
        );
        assert!(pbap_probe_skip(Trigger::OnDemand, false, true).is_some());
        assert!(
            pbap_probe_skip(Trigger::Connect, true, true).is_some(),
            "not with the connect report"
        );
        assert!(
            pbap_probe_skip(Trigger::FollowUp, true, false).is_some(),
            "already done this run"
        );
        assert_eq!(pbap_probe_skip(Trigger::FollowUp, true, true), None);
        assert_eq!(pbap_probe_skip(Trigger::OnDemand, true, true), None);
    }

    #[test]
    fn forgetting_the_phone_clears_everything_the_inventory_knew() {
        let mut s = State::new();
        s.ancs = Some(AncsTally::with_all_categories());
        s.ams_commands = Some(vec![0, 1]);
        s.listing_types.insert("SMS_GSM".into(), 3);
        s.mns_events.insert("NewMessage".into(), 1);
        s.vcard_fields.insert("contacts".into(), VcardFieldCounts::default());
        s.pbap = Some(PbapReport::default());
        s.pbap_attempted_at = Some(5);
        s.last_report = Some(sample());
        assert!(!probe_due(s.pbap.is_some(), s.pbap_attempted_at, 10, 100));
        s.reset_device();
        assert_eq!(s, State::new());
        assert!(
            probe_due(s.pbap.is_some(), s.pbap_attempted_at, 10, 100),
            "a new phone gets its own PBAP probe"
        );
    }

    #[test]
    fn names_obex_responses() {
        assert_eq!(obex_response_name(0xA0), "0xA0 success");
        assert_eq!(obex_response_name(0xC4), "0xC4 not found");
        assert_eq!(obex_response_name(0xD3), "0xD3 service unavailable");
        assert_eq!(obex_response_name(0xEE), "0xEE other");
    }

    #[test]
    fn an_unavailable_report_says_why_everywhere() {
        let r = BtInventory::unavailable(Trigger::OnDemand, 5, "Bluetooth is only supported on Windows");
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["trigger"], "onDemand");
        assert_eq!(json["gatt"]["unavailable"], "Bluetooth is only supported on Windows");
        assert_eq!(json["ancs"]["categories"]["schedule"], 0);
    }

    #[test]
    fn map_report_picks_the_mas_record() {
        let mas = SdpRecord {
            service_classes: vec!["MAP Message Access Server".into()],
            feature_names: vec!["notification".into()],
            details: [("mapMessageTypes".to_string(), "SMS_GSM".to_string())].into(),
            ..Default::default()
        };
        let r = map_report(Probe::Ok(vec!["inbox".into()]), Some(&[mas]));
        assert_eq!(r.sdp_features, Some(vec!["notification".to_string()]));
        assert_eq!(r.sdp_message_types.as_deref(), Some("SMS_GSM"));
        assert_eq!(map_report(Probe::Unavailable("x".into()), None).sdp_features, None);
    }
}
