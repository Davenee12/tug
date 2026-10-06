//! The BLE half of the Bluetooth inventory (`crate::bt_inventory`): one uncached GATT discovery,
//! reads of the readable characteristics (values kept only to decode Device Information, Current
//! Time and Battery; never logged), AMS Entity Attribute reads, the Windows link details, the
//! AudioPlaybackConnection check, then the Classic half (`map::probe`) and the running tallies.
//!
//! It runs as a local task off the connect path, about 30 s after "iPhone services ready" and
//! again 15 minutes later, or when asked for. One GATT operation at a time with a short gap
//! between them, every await bounded, and any failure only marks its own part unavailable: the
//! probe never touches the connection's state.

use std::cell::RefCell;
use std::rc::Rc;

use tokio::sync::oneshot;
use windows::Devices::Bluetooth::GenericAttributeProfile::GattCommunicationStatus;
use windows::Media::Audio::AudioPlaybackConnection;

use super::*;
use crate::bt_inventory::{
    self as inv, gatt, AmsAttribute, AmsReport, AmsValue, AudioPlaybackReport, BatteryReport, BtInventory,
    ConnectionPhy, CurrentTimeReport, LinkReport, Probe, Trigger,
};

/// After "iPhone services ready": long enough for the pre-existing notification replay to settle.
const FIRST_DELAY: Duration = Duration::from_secs(30);
/// The follow-up report, once ANCS categories have had time to accumulate.
const FOLLOW_UP: Duration = Duration::from_secs(15 * 60);
/// Pause between GATT operations, so ANCS and AMS traffic is never starved.
const OP_GAP: Duration = Duration::from_millis(150);
const CLASSIC_TIMEOUT: Duration = Duration::from_secs(30);
/// The inventory's own PBAP probe: seven size questions plus one all-fields pull.
const PBAP_PROBE_TIMEOUT: Duration = Duration::from_secs(150);
/// A failed PBAP probe is tried again at most this often (fav/spd aren't retried in a loop).
const PBAP_RETRY_MS: i64 = 30 * 60 * 1000;

/// AMS Entity Attribute is a select-then-read pair; tug's own truncated-value reads and the
/// inventory's take turns so neither reads the other's selection.
pub(super) static AMS_ATTRIBUTE_TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Default)]
pub(super) struct InventoryState {
    /// When the next automatic report is due, and which one it is.
    due: Option<(Instant, Trigger)>,
    run: Rc<RefCell<Run>>,
}

#[derive(Default)]
struct Run {
    busy: bool,
    /// On-demand callers waiting for the report being built.
    waiters: Vec<oneshot::Sender<BtInventory>>,
}

struct Inputs {
    trigger: Trigger,
    device: Option<BluetoothLEDevice>,
    max_pdu: Option<u16>,
    shared: Arc<Shared>,
}

impl Actor {
    /// A connection just came up: report on it shortly (and again 15 minutes later).
    pub(super) fn schedule_inventory(&mut self) {
        self.inventory.due = Some((Instant::now() + FIRST_DELAY, Trigger::Connect));
    }

    /// From the 1 s tick: start a due report once the link is ready and none is running.
    pub(super) fn inventory_tick(&mut self) {
        let Some((at, trigger)) = self.inventory.due else {
            return;
        };
        let ready = self.link.as_ref().is_some_and(|l| l.connected && l.ancs.is_some());
        if Instant::now() < at || !ready || self.inventory.run.borrow().busy {
            return;
        }
        self.inventory.due = (trigger == Trigger::Connect).then(|| (Instant::now() + FOLLOW_UP, Trigger::FollowUp));
        self.start_inventory(trigger);
    }

    /// The `bt_inventory` command: join the running report, or start one.
    pub(super) fn request_inventory(&mut self, reply: oneshot::Sender<BtInventory>) {
        let busy = {
            let mut run = self.inventory.run.borrow_mut();
            run.waiters.push(reply);
            run.busy
        };
        if !busy {
            self.start_inventory(Trigger::OnDemand);
        }
    }

    fn start_inventory(&mut self, trigger: Trigger) {
        let link = self.link.as_ref().filter(|l| l.connected);
        let inputs = Inputs {
            trigger,
            device: link.map(|l| l.device.clone()),
            max_pdu: link
                .and_then(|l| l._gatt_session.as_ref())
                .and_then(|s| s.MaxPduSize().ok()),
            shared: self.shared.clone(),
        };
        let run = self.inventory.run.clone();
        run.borrow_mut().busy = true;
        tokio::task::spawn_local(async move {
            let report = build(inputs).await;
            log::info!("{}", inv::log_line(&report));
            inv::set_last_report(&report);
            let waiters = {
                let mut r = run.borrow_mut();
                r.busy = false;
                std::mem::take(&mut r.waiters)
            };
            for w in waiters {
                let _ = w.send(report.clone());
            }
        });
    }
}

async fn build(i: Inputs) -> BtInventory {
    let now = now_ms();
    let mut r = BtInventory::unavailable(i.trigger, now, "the iPhone isn't connected");
    if let Some(device) = &i.device {
        match gatt_pass(device).await {
            Ok(pass) => {
                r.device_information = device_information(&pass);
                r.current_time = current_time(&pass);
                r.battery = battery(&pass);
                r.ams = ams(&pass).await;
                r.gatt = Probe::Ok(pass.services);
            }
            Err(e) => {
                let why = inv::safe_text(&format!("GATT discovery failed: {e}"), 160);
                r.device_information = Probe::Unavailable(why.clone());
                r.current_time = Probe::Unavailable(why.clone());
                r.battery = Probe::Unavailable(why.clone());
                r.ams = Probe::Unavailable(why.clone());
                r.gatt = Probe::Unavailable(why);
            }
        }
        r.link = Probe::Ok(link_report(device, i.max_pdu));
    }
    let status = i.shared.status();
    let names: Vec<String> = status
        .device
        .iter()
        .map(|d| d.name.clone())
        .chain(status.texts_device.clone())
        .collect();
    r.audio_playback = audio_playback(i.device.as_ref(), &names).await;
    classic(&mut r, &i.shared, i.trigger, now).await;
    r.ancs = inv::ancs_tally();
    r
}

type Key = (u128, u128);

/// What the discovery pass found. Read values stay in memory only long enough to decode the
/// standard services; they're never put in the report.
#[derive(Default)]
struct Pass {
    services: Vec<inv::GattService>,
    service_uuids: Vec<u128>,
    props: HashMap<Key, u32>,
    values: HashMap<Key, Result<Vec<u8>, String>>,
    read_at_ms: HashMap<Key, i64>,
    entity_attribute: Option<GattCharacteristic>,
}

impl Pass {
    fn has(&self, service: u128) -> bool {
        self.service_uuids.contains(&service)
    }
}

fn s(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn describe(e: &BleError) -> String {
    match e.att_code() {
        Some(code) => match ams::error_name(code) {
            Some(name) => format!("ATT 0x{code:02X} {name}"),
            None => format!("ATT 0x{code:02X}"),
        },
        None => inv::safe_text(&e.to_string(), 120),
    }
}

async fn gatt_pass(device: &BluetoothLEDevice) -> Result<Pass, String> {
    let res = winrt::bounded_for(
        winrt::DISCOVERY_TIMEOUT,
        device
            .GetGattServicesWithCacheModeAsync(BluetoothCacheMode::Uncached)
            .map_err(s)?,
    )
    .await
    .map_err(s)?;
    let status = res.Status().map_err(s)?;
    if status != GattCommunicationStatus::Success {
        return Err(format!("Windows reported status {}", status.0));
    }
    let mut pass = Pass::default();
    for svc in res.Services().map_err(s)? {
        let uuid = svc.Uuid().map_err(s)?.to_u128();
        pass.service_uuids.push(uuid);
        let characteristics = Probe::from_result(service_characteristics(&svc, uuid, &mut pass).await);
        pass.services.push(inv::GattService {
            uuid: gatt::format_uuid(uuid),
            name: gatt::service_name(uuid).map(str::to_string),
            characteristics,
        });
    }
    Ok(pass)
}

async fn service_characteristics(
    svc: &GattDeviceService,
    service: u128,
    pass: &mut Pass,
) -> Result<Vec<inv::GattCharacteristic>, String> {
    tokio::time::sleep(OP_GAP).await;
    let res = winrt::bounded_for(
        winrt::DISCOVERY_TIMEOUT,
        svc.GetCharacteristicsWithCacheModeAsync(BluetoothCacheMode::Uncached)
            .map_err(s)?,
    )
    .await
    .map_err(s)?;
    let status = res.Status().map_err(s)?;
    if status != GattCommunicationStatus::Success {
        return Err(format!("Windows reported status {}", status.0));
    }
    // ANCS and AMS are tug's live services: nothing there is read by the inventory.
    let apple_live = service == gatt::ANCS_SERVICE || service == gatt::AMS_SERVICE;
    let mut out = Vec::new();
    for ch in res.Characteristics().map_err(s)? {
        let uuid = ch.Uuid().map_err(s)?.to_u128();
        let bits = ch.CharacteristicProperties().map_err(s)?.0;
        let key = (service, uuid);
        pass.props.insert(key, bits);
        if service == gatt::AMS_SERVICE && uuid == ams::ENTITY_ATTRIBUTE {
            pass.entity_attribute = Some(ch.clone());
        }
        let read = if bits & gatt::PROP_READ == 0 {
            None
        } else if apple_live {
            Some("not read (live tug service)".to_string())
        } else {
            tokio::time::sleep(OP_GAP).await;
            match winrt::read(&ch).await {
                Ok(bytes) => {
                    let outcome = format!("{} bytes", bytes.len());
                    pass.values.insert(key, Ok(bytes));
                    pass.read_at_ms.insert(key, now_ms());
                    Some(outcome)
                }
                Err(e) => {
                    let why = describe(&e);
                    pass.values.insert(key, Err(why.clone()));
                    Some(format!("error: {why}"))
                }
            }
        };
        out.push(inv::GattCharacteristic {
            uuid: gatt::format_uuid(uuid),
            name: gatt::characteristic_name(uuid).map(str::to_string),
            properties: gatt::property_names(bits).into_iter().map(str::to_string).collect(),
            read,
        });
    }
    Ok(out)
}

const NOT_PRESENT: &str = "not present on the iPhone";

fn device_information(pass: &Pass) -> Probe<inv::DeviceInformation> {
    let svc = gatt::sig(gatt::DEVICE_INFORMATION);
    if !pass.has(svc) {
        return Probe::Unavailable(NOT_PRESENT.into());
    }
    let mut d = inv::DeviceInformation::default();
    for (&(service, ch), value) in &pass.values {
        let Some(short) = gatt::sig_short(ch).filter(|_| service == svc) else {
            continue;
        };
        let name = gatt::characteristic_name(ch).unwrap_or("unknown").to_string();
        let bytes = match value {
            Ok(b) => b,
            Err(e) => {
                d.unreadable.insert(name, e.clone());
                continue;
            }
        };
        match short {
            0x2A29 => d.manufacturer = Some(gatt::decode_dis_string(bytes)),
            0x2A24 => d.model_number = Some(gatt::decode_dis_string(bytes)),
            0x2A25 => d.serial_number_length = Some(bytes.len()),
            0x2A27 => d.hardware_revision = Some(gatt::decode_dis_string(bytes)),
            0x2A26 => d.firmware_revision = Some(gatt::decode_dis_string(bytes)),
            0x2A28 => d.software_revision = Some(gatt::decode_dis_string(bytes)),
            0x2A23 => d.system_id_length = Some(bytes.len()),
            0x2A2A => d.regulatory_data_length = Some(bytes.len()),
            0x2A50 => match gatt::decode_pnp_id(bytes) {
                Ok(p) => d.pnp_id = Some(p),
                Err(e) => {
                    d.unreadable.insert(name, e);
                }
            },
            _ => {}
        }
    }
    Probe::Ok(d)
}

/// One characteristic of `service`, decoded, or why not.
fn decoded<T>(pass: &Pass, service: u128, short: u16, f: impl Fn(&[u8]) -> Result<T, String>) -> Probe<T> {
    let key = (service, gatt::sig(short));
    match pass.values.get(&key) {
        Some(Ok(b)) => Probe::from_result(f(b)),
        Some(Err(e)) => Probe::Unavailable(e.clone()),
        None if pass.props.contains_key(&key) => Probe::Unavailable("not readable".into()),
        None => Probe::Unavailable(NOT_PRESENT.into()),
    }
}

fn current_time(pass: &Pass) -> Probe<CurrentTimeReport> {
    let svc = gatt::sig(gatt::CURRENT_TIME_SERVICE);
    if !pass.has(svc) {
        return Probe::Unavailable(NOT_PRESENT.into());
    }
    let current = decoded(pass, svc, gatt::CURRENT_TIME, gatt::decode_current_time);
    let local = decoded(pass, svc, gatt::LOCAL_TIME_INFO, gatt::decode_local_time_info);
    let reference = decoded(pass, svc, gatt::REFERENCE_TIME_INFO, gatt::decode_reference_time_info);
    let utc_offset_minutes = local.ok().and_then(|l| l.utc_offset_minutes());
    let read_at = pass.read_at_ms.get(&(svc, gatt::sig(gatt::CURRENT_TIME)));
    let skew_seconds = match (current.ok(), utc_offset_minutes, read_at) {
        (Some(t), Some(offset), Some(&at)) => Some(gatt::clock_skew_seconds(t, offset, at.div_euclid(1000))),
        _ => None,
    };
    let props = pass
        .props
        .get(&(svc, gatt::sig(gatt::CURRENT_TIME)))
        .copied()
        .unwrap_or(0);
    Probe::Ok(CurrentTimeReport {
        current_time: current,
        current_time_notifies: props & (gatt::PROP_NOTIFY | gatt::PROP_INDICATE) != 0,
        local_time_info: local,
        reference_time_info: reference,
        utc_offset_minutes,
        skew_seconds,
    })
}

fn battery(pass: &Pass) -> Probe<BatteryReport> {
    let svc = gatt::sig(gatt::BATTERY_SERVICE);
    if !pass.has(svc) {
        return Probe::Unavailable(NOT_PRESENT.into());
    }
    let mut shorts: Vec<(u128, u32)> = pass
        .props
        .iter()
        .filter(|((service, _), _)| *service == svc)
        .map(|((_, ch), bits)| (*ch, *bits))
        .collect();
    shorts.sort();
    let level_key = (svc, gatt::sig(gatt::BATTERY_LEVEL));
    Probe::Ok(BatteryReport {
        characteristics: shorts
            .iter()
            .map(|(ch, _)| match gatt::characteristic_name(*ch) {
                Some(n) => format!("{} {n}", gatt::format_uuid(*ch)),
                None => gatt::format_uuid(*ch),
            })
            .collect(),
        level: match pass.values.get(&level_key) {
            Some(Ok(b)) => b.first().map(|&l| l.min(100)),
            _ => None,
        },
        level_notifies: pass.props.get(&level_key).is_some_and(|b| b & gatt::PROP_NOTIFY != 0),
        power_state_characteristic: shorts
            .iter()
            .any(|(ch, _)| gatt::sig_short(*ch).is_some_and(gatt::is_power_state_characteristic)),
    })
}

async fn ams(pass: &Pass) -> Probe<AmsReport> {
    let Some(attr) = &pass.entity_attribute else {
        return Probe::Unavailable(if pass.has(gatt::AMS_SERVICE) {
            "AMS has no Entity Attribute characteristic".into()
        } else {
            NOT_PRESENT.into()
        });
    };
    let mut attributes = Vec::new();
    for (entity, attribute, name, value_safe) in inv::AMS_ATTRIBUTES {
        tokio::time::sleep(OP_GAP).await;
        let result = match super::media::read_attribute(attr, entity, attribute).await {
            Ok(value) => inv::ams_value(&value, value_safe),
            Err(e) => AmsValue::Error(describe(&e)),
        };
        attributes.push(AmsAttribute {
            name: name.to_string(),
            result,
        });
    }
    Probe::Ok(AmsReport {
        attributes,
        supported_commands: inv::ams_commands(),
    })
}

fn link_report(device: &BluetoothLEDevice, max_pdu_size: Option<u16>) -> LinkReport {
    let win11 = |e: windows::core::Error| format!("Windows didn't provide it (needs Windows 11): {}", e.message());
    let parameters = device
        .GetConnectionParameters()
        .and_then(|p| {
            Ok(inv::connection_parameters(
                p.ConnectionInterval()?,
                p.ConnectionLatency()?,
                p.LinkTimeout()?,
            ))
        })
        .map_err(win11);
    let phy = device
        .GetConnectionPhy()
        .and_then(|p| {
            let name = |i: windows::Devices::Bluetooth::BluetoothLEConnectionPhyInfo| -> windows::core::Result<String> {
                Ok(inv::phy_name(i.IsUncoded1MPhy()?, i.IsUncoded2MPhy()?, i.IsCodedPhy()?))
            };
            Ok(ConnectionPhy {
                transmit: name(p.TransmitInfo()?)?,
                receive: name(p.ReceiveInfo()?)?,
            })
        })
        .map_err(win11);
    LinkReport {
        parameters: Probe::from_result(parameters),
        phy: Probe::from_result(phy),
        max_pdu_size,
    }
}

/// Whether Windows could play this phone's audio (A2DP sink). Enumeration only: nothing is
/// opened and no audio is routed.
async fn audio_playback(device: Option<&BluetoothLEDevice>, names: &[String]) -> Probe<AudioPlaybackReport> {
    let selector = match AudioPlaybackConnection::GetDeviceSelector() {
        Ok(sel) => sel,
        Err(e) => {
            return Probe::Unavailable(inv::safe_text(
                &format!("not available on this PC: {}", e.message()),
                160,
            ))
        }
    };
    let found = match DeviceInformation::FindAllAsyncAqsFilter(&selector) {
        Ok(op) => winrt::bounded_for(CLASSIC_TIMEOUT, op).await.map_err(s),
        Err(e) => Err(e.message()),
    };
    let infos = match found {
        Ok(i) => i,
        Err(e) => return Probe::Unavailable(inv::safe_text(&format!("enumeration failed: {e}"), 160)),
    };
    let address = device
        .and_then(|d| d.BluetoothAddress().ok())
        .map(|a| format!("{a:012x}"));
    let mut candidates = 0;
    let mut iphone_listed = false;
    for info in infos {
        candidates += 1;
        let name = info.Name().map(|n| n.to_string()).unwrap_or_default();
        let id = info.Id().map(|n| n.to_string().to_lowercase()).unwrap_or_default();
        let by_name = names
            .iter()
            .any(|n| !n.trim().is_empty() && n.trim().eq_ignore_ascii_case(name.trim()));
        let by_address = address.as_ref().is_some_and(|a| id.contains(a.as_str()));
        iphone_listed |= by_name || by_address;
    }
    Probe::Ok(AudioPlaybackReport {
        candidates,
        iphone_listed,
    })
}

/// The Classic half: SDP records, PBAP (the inventory's own probe runs only with the follow-up
/// or an on-demand report, once per run, so it never competes with the contacts sync right after
/// connecting) and MAP.
async fn classic(r: &mut BtInventory, shared: &Arc<Shared>, trigger: Trigger, now: i64) {
    let Some(device_id) = shared.store.setting(keys::TEXTS_DEVICE_ID).ok().flatten() else {
        let why = "the texts side (Classic Bluetooth) hasn't connected yet";
        r.classic_sdp = Probe::Unavailable(why.into());
        r.map = Probe::Unavailable(why.into());
        r.pbap = Probe::Unavailable(why.into());
        return;
    };
    let sdp = Probe::from_result(
        tokio::time::timeout(CLASSIC_TIMEOUT, crate::map::probe::sdp_records(&device_id))
            .await
            .unwrap_or_else(|_| Err("timeout".into())),
    );
    let mut pbap_error = None;
    if trigger != Trigger::Connect && inv::pbap_probe_due(now, PBAP_RETRY_MS) {
        let result = tokio::time::timeout(PBAP_PROBE_TIMEOUT, crate::map::probe::pbap_probe(&device_id, now))
            .await
            .unwrap_or_else(|_| Err("timeout".into()));
        match result {
            Ok(p) => inv::set_pbap_probe(Some(p), now),
            Err(e) => {
                let e = inv::safe_text(&e, 160);
                log::info!("Bluetooth inventory: the PBAP probe failed: {e}");
                inv::set_pbap_probe(None, now);
                pbap_error = Some(e);
            }
        }
    }
    r.pbap = match inv::pbap_report() {
        Some(p) => Probe::Ok(p),
        None => Probe::Unavailable(
            pbap_error
                .unwrap_or_else(|| "no phonebook pull yet (the inventory's own runs with the 15-minute report)".into()),
        ),
    };
    let folders = match shared.map.get() {
        Some(map) => tokio::time::timeout(CLASSIC_TIMEOUT, map.inventory_folders())
            .await
            .unwrap_or_else(|_| Probe::Unavailable("the message service was busy".into())),
        None => Probe::Unavailable("the message service isn't running".into()),
    };
    r.map = Probe::Ok(inv::map_report(folders, sdp.ok().map(Vec::as_slice)));
    r.classic_sdp = sdp;
}
