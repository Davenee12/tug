//! The BLE half of the Bluetooth inventory (`crate::bt_inventory`): one uncached GATT discovery,
//! reads of the readable characteristics (values kept only to decode Device Information, Current
//! Time and Battery; never logged), AMS Entity Attribute reads, the Windows link details, the
//! AudioPlaybackConnection check, then the Classic half (`map::probe`) and the running tallies.
//!
//! tug's own traffic comes first. The full report runs once per app run, about 30 s after the
//! first "iPhone services ready" and only once ANCS's pre-existing-notification replay has settled
//! with no request in flight; a follow-up runs 15 minutes later, and one can be asked for. It's a
//! local task off the connect path doing one GATT operation at a time with a short gap, each one
//! bounded. The first sign the link stopped answering (a timeout, Windows reporting the phone
//! unreachable, or Windows closing tug's GATT objects) ends the pass and skips everything after it,
//! so orphaned operations can't pile up behind tug's own ANCS writes; and a link going down aborts
//! the task outright (`cancel_inventory`), dropping the AMS turn it may hold. A part the phone
//! refuses (an ATT error) only marks that part unavailable.

use std::cell::RefCell;
use std::rc::Rc;

use tokio::sync::oneshot;
use tokio::task::AbortHandle;
use windows::Devices::Bluetooth::GenericAttributeProfile::GattCommunicationStatus;
use windows::Media::Audio::AudioPlaybackConnection;

use super::*;
use crate::bt_inventory::{
    self as inv, gatt, AmsAttribute, AmsReport, AmsValue, AudioPlaybackReport, BatteryReport, BtInventory,
    ConnectionParameters, ConnectionPhy, CurrentTimeReport, LinkReport, Probe, Trigger,
};
use crate::map::probe::PbapProbeError;

/// After "iPhone services ready": the earliest the report starts (it also waits for ANCS to settle).
const FIRST_DELAY: Duration = Duration::from_secs(30);
/// The follow-up report, once ANCS categories have had time to accumulate.
const FOLLOW_UP: Duration = Duration::from_secs(15 * 60);
/// Pause between GATT operations, so ANCS and AMS traffic is never starved.
const OP_GAP: Duration = Duration::from_millis(150);
/// Longest the inventory waits on one of its own reads (or AMS select-then-read pairs) before
/// taking it as the link not answering. Shorter than tug's own GATT bound, so the probe gives up
/// first and never holds the AMS turn, which tug's own reads wait on, for long.
const PROBE_OP_TIMEOUT: Duration = Duration::from_secs(5);
/// The Windows link details are synchronous calls, made on a helper thread; this long at most.
const LINK_INFO_TIMEOUT: Duration = Duration::from_secs(3);
const CLASSIC_TIMEOUT: Duration = Duration::from_secs(30);
/// The inventory's phonebook check: up to seven size questions and one reconnect.
const PBAP_PROBE_TIMEOUT: Duration = Duration::from_secs(90);
/// A failed phonebook check is tried again at most this often (fav/spd aren't retried in a loop).
const PBAP_RETRY_MS: i64 = 30 * 60 * 1000;

/// What waiting callers get when the link goes down mid-report.
const DISCONNECTED: &str = "the iPhone disconnected during the report";
/// What every part after a stopped GATT pass says.
const SKIPPED_AFTER_STOP: &str = "skipped: the Bluetooth link stopped answering earlier in this report";

/// AMS Entity Attribute is a select-then-read pair; tug's own truncated-value reads and the
/// inventory's take turns so neither reads the other's selection.
pub(super) static AMS_ATTRIBUTE_TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Default)]
pub(super) struct InventoryState {
    /// When the next automatic report is due, and which one it is.
    due: Option<(Instant, Trigger)>,
    /// The Connect report has started this app run, so later reconnects don't repeat it.
    connect_done: bool,
    run: Rc<RefCell<Run>>,
}

impl InventoryState {
    fn busy(&self) -> bool {
        self.run.borrow().current.is_some()
    }
}

#[derive(Default)]
struct Run {
    /// The report being built: what triggered it, and the handle that stops it.
    current: Option<(Trigger, AbortHandle)>,
    /// On-demand callers waiting for the report being built.
    waiters: Vec<oneshot::Sender<BtInventory>>,
}

struct Inputs {
    trigger: Trigger,
    device: Option<BluetoothLEDevice>,
    max_pdu: Option<u16>,
    shared: Arc<Shared>,
}

/// The live link, as far as starting an automatic report goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Readiness {
    connected: bool,
    /// ANCS is subscribed, its pre-existing-notification replay has settled (the stale-row sweep
    /// ran), and no Control Point request is waiting for its answer.
    ancs_settled: bool,
}

/// Which automatic report to start now, if any: one is due, none is running, and the link is up
/// with ANCS settled. tug's own replay and requests go first; the report waits for a quiet moment.
fn report_to_start(due: Option<(Instant, Trigger)>, now: Instant, busy: bool, link: Readiness) -> Option<Trigger> {
    let (at, trigger) = due?;
    (now >= at && !busy && link.connected && link.ancs_settled).then_some(trigger)
}

/// What a successful connect does to the schedule. The first one this run schedules the Connect
/// report; later reconnects leave the schedule as it is (a pending follow-up still runs), so a
/// flaky link doesn't get the full pass on every reconnect.
fn schedule_on_connect(
    due: Option<(Instant, Trigger)>,
    connect_done: bool,
    now: Instant,
) -> Option<(Instant, Trigger)> {
    if connect_done {
        due
    } else {
        Some((now + FIRST_DELAY, Trigger::Connect))
    }
}

/// What's due once `trigger` has started: the follow-up after the Connect report, nothing after
/// the others.
fn due_after(trigger: Trigger, now: Instant) -> Option<(Instant, Trigger)> {
    (trigger == Trigger::Connect).then(|| (now + FOLLOW_UP, Trigger::FollowUp))
}

/// Stop the report being built, if any: abort its task (dropping everything it holds, the AMS
/// turn and any PBAP link included) and answer whoever was waiting on it. Whether one was running.
fn cancel_run(run: &RefCell<Run>, reason: &str) -> bool {
    let (trigger, waiters) = {
        let mut r = run.borrow_mut();
        let Some((trigger, handle)) = r.current.take() else {
            return false;
        };
        handle.abort();
        (trigger, std::mem::take(&mut r.waiters))
    };
    let report = BtInventory::unavailable(trigger, now_ms(), reason);
    for w in waiters {
        let _ = w.send(report.clone());
    }
    true
}

impl Actor {
    /// A connection just came up: the first this run gets a report shortly (and a follow-up 15
    /// minutes later); later reconnects don't repeat it.
    pub(super) fn schedule_inventory(&mut self) {
        self.inventory.due = schedule_on_connect(self.inventory.due, self.inventory.connect_done, Instant::now());
    }

    /// From the 1 s tick: start a due report once the link is ready and none is running.
    pub(super) fn inventory_tick(&mut self) {
        let link = self.link.as_ref().map_or_else(Readiness::default, |l| Readiness {
            connected: l.connected,
            ancs_settled: l
                .ancs
                .as_ref()
                .is_some_and(|a| a.sweep_after.is_none() && a.requests.inflight().is_none()),
        });
        let now = Instant::now();
        let Some(trigger) = report_to_start(self.inventory.due, now, self.inventory.busy(), link) else {
            return;
        };
        if trigger == Trigger::Connect {
            self.inventory.connect_done = true;
        }
        self.inventory.due = due_after(trigger, now);
        self.start_inventory(trigger);
    }

    /// The `bt_inventory` command: join the running report, or start one.
    pub(super) fn request_inventory(&mut self, reply: oneshot::Sender<BtInventory>) {
        self.inventory.run.borrow_mut().waiters.push(reply);
        if !self.inventory.busy() {
            self.start_inventory(Trigger::OnDemand);
        }
    }

    /// The link went down (or was dropped): stop the report being built, so nothing more is asked
    /// of a link that's gone and the AMS turn is free for the reconnect's own reads.
    pub(super) fn cancel_inventory(&mut self) {
        if cancel_run(&self.inventory.run, DISCONNECTED) {
            log::info!("Bluetooth inventory: stopped ({DISCONNECTED})");
        }
    }

    /// tug forgot the phone or adopted a different one: stop any report, clear the schedule (the
    /// next phone gets its own Connect report) and forget what the inventory knew about this one.
    pub(super) fn reset_inventory(&mut self) {
        self.cancel_inventory();
        self.inventory.due = None;
        self.inventory.connect_done = false;
        inv::reset_device_state();
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
        let task = tokio::task::spawn_local(async move {
            let report = build(inputs).await;
            log::info!("{}", inv::log_line(&report));
            inv::set_last_report(&report);
            let waiters = {
                let mut r = run.borrow_mut();
                r.current = None;
                std::mem::take(&mut r.waiters)
            };
            for w in waiters {
                let _ = w.send(report.clone());
            }
        });
        // A local task doesn't run until this returns, so the handle is in place before it can end.
        self.inventory.run.borrow_mut().current = Some((trigger, task.abort_handle()));
    }
}

async fn build(i: Inputs) -> BtInventory {
    let now = now_ms();
    let mut r = BtInventory::unavailable(i.trigger, now, "the iPhone isn't connected");
    let mut stopped: Option<String> = None;
    if let Some(device) = &i.device {
        match gatt_pass(device).await {
            Ok(pass) => {
                r.device_information = device_information(&pass);
                r.current_time = current_time(&pass);
                r.battery = battery(&pass);
                r.ams = match &pass.stopped {
                    Some(_) => Probe::Unavailable(SKIPPED_AFTER_STOP.into()),
                    None => {
                        let (ams, stop) = ams(&pass).await;
                        stopped = stop;
                        ams
                    }
                };
                stopped = pass.stopped.or(stopped);
                r.gatt = Probe::Ok(pass.services);
            }
            Err(e) => {
                let why = match e {
                    StepError::LinkGone(detail) => {
                        let why = stop_text(&detail);
                        stopped = Some(why.clone());
                        why
                    }
                    StepError::Part(detail) => inv::safe_text(&format!("GATT discovery failed: {detail}"), 160),
                };
                r.device_information = Probe::Unavailable(why.clone());
                r.current_time = Probe::Unavailable(why.clone());
                r.battery = Probe::Unavailable(why.clone());
                r.ams = Probe::Unavailable(why.clone());
                r.gatt = Probe::Unavailable(why);
            }
        }
        if stopped.is_none() {
            r.link = Probe::Ok(link_report(device, i.max_pdu).await);
        }
    }
    match &stopped {
        Some(why) => {
            log::info!("Bluetooth inventory: {why}");
            r.skip_after_link_loss(SKIPPED_AFTER_STOP);
        }
        None => {
            let status = i.shared.status();
            let names: Vec<String> = status
                .device
                .iter()
                .map(|d| d.name.clone())
                .chain(status.texts_device.clone())
                .collect();
            r.audio_playback = audio_playback(i.device.as_ref(), &names).await;
            classic(&mut r, &i.shared, i.trigger, now).await;
        }
    }
    r.ancs = inv::ancs_tally();
    r
}

/// A failed inventory step: the link stopped answering (the whole pass ends), or only this part
/// failed (the phone answered with an error).
#[derive(Debug, PartialEq)]
enum StepError {
    LinkGone(String),
    Part(String),
}

/// Whether a failed step means the link itself stopped answering — a timeout (`TimedOut`), Windows
/// reporting the phone unreachable (`Unreachable`), or Windows closing tug's GATT objects — so the pass
/// ends rather than queue more operations behind tug's own. An ATT error or an access refusal is
/// the phone answering: only that part is marked.
fn ends_pass(e: &BleError) -> bool {
    matches!(e, BleError::Unreachable) || e.is_timeout() || e.is_closed()
}

impl From<BleError> for StepError {
    fn from(e: BleError) -> Self {
        if ends_pass(&e) {
            StepError::LinkGone(describe(&e))
        } else {
            StepError::Part(describe(&e))
        }
    }
}

impl From<windows::core::Error> for StepError {
    fn from(e: windows::core::Error) -> Self {
        BleError::Win(e).into()
    }
}

/// A discovery result's status that isn't Success.
fn status_error(status: GattCommunicationStatus) -> StepError {
    if status == GattCommunicationStatus::Unreachable {
        StepError::LinkGone("Windows reported the iPhone unreachable".into())
    } else {
        StepError::Part(format!("Windows reported status {}", status.0))
    }
}

/// The reason a stopped pass gives, in the part where it stopped.
fn stop_text(detail: &str) -> String {
    inv::safe_text(
        &format!("stopped: the Bluetooth link stopped answering ({detail}), so the pass ended here"),
        200,
    )
}

type Key = (u128, u128);

/// What the discovery pass found. Read values stay in memory only long enough to decode the
/// standard services; they're never put in the report.
#[derive(Default)]
struct Pass {
    services: Vec<inv::GattService>,
    service_uuids: Vec<u128>,
    /// Services whose characteristics were all listed (and read where readable).
    completed: HashSet<u128>,
    props: HashMap<Key, u32>,
    values: HashMap<Key, Result<Vec<u8>, String>>,
    read_at_ms: HashMap<Key, i64>,
    entity_attribute: Option<GattCharacteristic>,
    /// Set when the link stopped answering: nothing more is asked of it.
    stopped: Option<String>,
}

impl Pass {
    fn has(&self, service: u128) -> bool {
        self.service_uuids.contains(&service)
    }

    /// Whether `service` was walked in full, so its section can be decoded; or why not.
    fn walked(&self, service: u128) -> Result<(), String> {
        if !self.has(service) {
            Err(NOT_PRESENT.into())
        } else if !self.completed.contains(&service) {
            Err(self
                .stopped
                .clone()
                .unwrap_or_else(|| "its characteristics couldn't be listed".into()))
        } else {
            Ok(())
        }
    }
}

fn describe(e: &BleError) -> String {
    match e {
        BleError::Unreachable => "Windows reported the iPhone unreachable".into(),
        BleError::TimedOut => "no answer in time".into(),
        _ => match e.att_code() {
            Some(code) => match ams::error_name(code) {
                Some(name) => format!("ATT 0x{code:02X} {name}"),
                None => format!("ATT 0x{code:02X}"),
            },
            None => inv::safe_text(&e.to_string(), 120),
        },
    }
}

/// One read by the inventory, given up on sooner than tug's own (as the link not answering).
async fn probe_read(ch: &GattCharacteristic) -> Result<Vec<u8>, BleError> {
    tokio::time::timeout(PROBE_OP_TIMEOUT, winrt::read(ch))
        .await
        .map_err(|_| BleError::TimedOut)?
}

async fn gatt_pass(device: &BluetoothLEDevice) -> Result<Pass, StepError> {
    let res = winrt::bounded_for(
        winrt::DISCOVERY_TIMEOUT,
        device.GetGattServicesWithCacheModeAsync(BluetoothCacheMode::Uncached)?,
    )
    .await?;
    let status = res.Status()?;
    if status != GattCommunicationStatus::Success {
        return Err(status_error(status));
    }
    let mut services = Vec::new();
    for svc in res.Services()? {
        let uuid = svc.Uuid()?.to_u128();
        services.push((svc, uuid));
    }
    let mut pass = Pass {
        service_uuids: services.iter().map(|(_, uuid)| *uuid).collect(),
        ..Default::default()
    };
    for (svc, uuid) in services {
        let characteristics = if pass.stopped.is_some() {
            Probe::Unavailable(SKIPPED_AFTER_STOP.into())
        } else {
            match service_characteristics(&svc, uuid, &mut pass).await {
                Ok(chars) => {
                    if pass.stopped.is_none() {
                        pass.completed.insert(uuid);
                    }
                    Probe::Ok(chars)
                }
                Err(StepError::LinkGone(detail)) => {
                    let why = stop_text(&detail);
                    pass.stopped = Some(why.clone());
                    Probe::Unavailable(why)
                }
                Err(StepError::Part(detail)) => Probe::Unavailable(inv::safe_text(&detail, 160)),
            }
        };
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
) -> Result<Vec<inv::GattCharacteristic>, StepError> {
    tokio::time::sleep(OP_GAP).await;
    let res = winrt::bounded_for(
        winrt::DISCOVERY_TIMEOUT,
        svc.GetCharacteristicsWithCacheModeAsync(BluetoothCacheMode::Uncached)?,
    )
    .await?;
    let status = res.Status()?;
    if status != GattCommunicationStatus::Success {
        return Err(status_error(status));
    }
    // ANCS and AMS are tug's live services: none of their characteristics is read here. (The only
    // AMS traffic is `ams()`'s Entity Attribute pairs, which take turns with tug's own reads.)
    let apple_live = service == gatt::ANCS_SERVICE || service == gatt::AMS_SERVICE;
    let mut out = Vec::new();
    for ch in res.Characteristics()? {
        let uuid = ch.Uuid()?.to_u128();
        let bits = ch.CharacteristicProperties()?.0;
        let key = (service, uuid);
        pass.props.insert(key, bits);
        if service == gatt::AMS_SERVICE && uuid == ams::ENTITY_ATTRIBUTE {
            pass.entity_attribute = Some(ch.clone());
        }
        let read = if bits & gatt::PROP_READ == 0 {
            None
        } else if apple_live {
            Some("not read (live tug service)".to_string())
        } else if pass.stopped.is_some() {
            Some("not read: the pass stopped".to_string())
        } else {
            tokio::time::sleep(OP_GAP).await;
            match probe_read(&ch).await {
                Ok(bytes) => {
                    let outcome = format!("{} bytes", bytes.len());
                    pass.values.insert(key, Ok(bytes));
                    pass.read_at_ms.insert(key, now_ms());
                    Some(outcome)
                }
                Err(e) => {
                    let why = describe(&e);
                    if ends_pass(&e) {
                        pass.stopped = Some(stop_text(&why));
                    }
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
    if let Err(why) = pass.walked(svc) {
        return Probe::Unavailable(why);
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
    if let Err(why) = pass.walked(svc) {
        return Probe::Unavailable(why);
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
    if let Err(why) = pass.walked(svc) {
        return Probe::Unavailable(why);
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

/// Which AMS attributes answer, and why the pass stopped if the link stopped answering.
async fn ams(pass: &Pass) -> (Probe<AmsReport>, Option<String>) {
    let Some(attr) = &pass.entity_attribute else {
        let why = if pass.has(gatt::AMS_SERVICE) {
            "AMS has no Entity Attribute characteristic"
        } else {
            NOT_PRESENT
        };
        return (Probe::Unavailable(why.into()), None);
    };
    let mut attributes = Vec::new();
    let mut stopped = None;
    for (entity, attribute, name, value_safe) in inv::AMS_ATTRIBUTES {
        let result = if stopped.is_some() {
            AmsValue::Error("not asked: the pass stopped".into())
        } else {
            tokio::time::sleep(OP_GAP).await;
            match ams_read(attr, entity, attribute).await {
                Ok(value) => inv::ams_value(&value, value_safe),
                Err(e) => {
                    let why = describe(&e);
                    if ends_pass(&e) {
                        stopped = Some(stop_text(&why));
                    }
                    AmsValue::Error(why)
                }
            }
        };
        attributes.push(AmsAttribute {
            name: name.to_string(),
            result,
        });
    }
    let report = AmsReport {
        attributes,
        supported_commands: inv::ams_commands(),
    };
    (Probe::Ok(report), stopped)
}

/// One AMS Entity Attribute select-then-read pair for the inventory. It takes the same turn as
/// tug's own reads (so neither reads the other's selection), but gives up after `PROBE_OP_TIMEOUT`
/// and releases the turn, so tug's own reads never wait long behind it.
async fn ams_read(attr: &GattCharacteristic, entity: u8, attribute: u8) -> Result<Vec<u8>, BleError> {
    let _turn = AMS_ATTRIBUTE_TURN.lock().await;
    tokio::time::timeout(PROBE_OP_TIMEOUT, async {
        winrt::write(attr, &[entity, attribute]).await?;
        winrt::read(attr).await
    })
    .await
    .map_err(|_| BleError::TimedOut)?
}

type LinkDetails = (Result<ConnectionParameters, String>, Result<ConnectionPhy, String>);

/// The Windows link details. `GetConnectionParameters`/`GetConnectionPhy` are synchronous and can
/// block while the adapter is busy, so they run on a short-lived helper thread and the BLE thread
/// waits at most `LINK_INFO_TIMEOUT` (a stuck call leaves only that helper thread waiting).
async fn link_report(device: &BluetoothLEDevice, max_pdu_size: Option<u16>) -> LinkReport {
    let (tx, rx) = oneshot::channel();
    let device = device.clone();
    let spawned = std::thread::Builder::new()
        .name("tug-bt-link-info".into())
        .spawn(move || {
            let _ = tx.send(link_details(&device));
        });
    let (parameters, phy): LinkDetails = match spawned {
        Err(e) => {
            let why = format!("couldn't ask Windows: {e}");
            (Err(why.clone()), Err(why))
        }
        Ok(_) => match tokio::time::timeout(LINK_INFO_TIMEOUT, rx).await {
            Ok(Ok(details)) => details,
            Ok(Err(_)) => {
                let why = "Windows didn't answer".to_string();
                (Err(why.clone()), Err(why))
            }
            Err(_) => {
                let why = "Windows didn't answer in time".to_string();
                (Err(why.clone()), Err(why))
            }
        },
    };
    LinkReport {
        parameters: Probe::from_result(parameters),
        phy: Probe::from_result(phy),
        max_pdu_size,
    }
}

/// The synchronous Windows 11 link calls (run on `link_report`'s helper thread).
fn link_details(device: &BluetoothLEDevice) -> LinkDetails {
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
    (parameters, phy)
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
        Ok(op) => winrt::bounded_for(CLASSIC_TIMEOUT, op).await.map_err(|e| e.to_string()),
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

/// The Classic half: SDP records, PBAP and MAP. The inventory's own phonebook check (sizes only)
/// needs Sync Contacts on, skips the Connect report and runs once per run (`pbap_probe_skip`); it
/// takes the PBAP turn with `try_lock`, so it skips rather than open a second PBAP connection
/// while tug's own contacts, calls or photo pull has one, and those pulls defer while it runs.
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
    let contacts_shared = shared.status().contacts_shared;
    let mut note =
        inv::pbap_probe_skip(trigger, contacts_shared, inv::pbap_probe_due(now, PBAP_RETRY_MS)).map(str::to_string);
    if note.is_none() {
        let result = tokio::time::timeout(PBAP_PROBE_TIMEOUT, crate::map::probe::pbap_probe(&device_id, now))
            .await
            .unwrap_or_else(|_| Err(PbapProbeError::Failed("timeout".into())));
        match result {
            Ok(p) => inv::set_pbap_probe(Some(p), now),
            // Not an attempt: the next report asks again.
            Err(PbapProbeError::Busy) => {
                note = Some(
                    "skipped: tug's own contacts, calls or photo pull had the phonebook connection (asked again next report)"
                        .into(),
                )
            }
            Err(PbapProbeError::Failed(e)) => {
                let e = inv::safe_text(&e, 160);
                log::info!("Bluetooth inventory: the PBAP probe failed: {e}");
                inv::set_pbap_probe(None, now);
                note = Some(format!("the phonebook check failed: {e}"));
            }
        }
    }
    r.pbap = match inv::pbap_report() {
        Some(mut p) => {
            p.phonebooks_note = note;
            Probe::Ok(p)
        }
        None => Probe::Unavailable(note.unwrap_or_else(|| "no phonebook pull yet".into())),
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

#[cfg(test)]
mod tests {
    use super::*;

    const READY: Readiness = Readiness {
        connected: true,
        ancs_settled: true,
    };

    #[test]
    fn a_report_waits_until_ancs_has_settled() {
        let now = Instant::now();
        let due = Some((now, Trigger::Connect));
        assert_eq!(report_to_start(due, now, false, READY), Some(Trigger::Connect));
        let replaying = Readiness {
            ancs_settled: false,
            ..READY
        };
        assert_eq!(
            report_to_start(due, now, false, replaying),
            None,
            "replay still settling, or a request in flight: wait"
        );
        let down = Readiness {
            connected: false,
            ..READY
        };
        assert_eq!(report_to_start(due, now, false, down), None, "link down: wait");
        assert_eq!(report_to_start(due, now, true, READY), None, "one already running");
        assert_eq!(
            report_to_start(
                Some((now + Duration::from_secs(1), Trigger::Connect)),
                now,
                false,
                READY
            ),
            None,
            "not due yet"
        );
        assert_eq!(report_to_start(None, now, false, READY), None, "nothing due");
        assert_eq!(
            report_to_start(Some((now, Trigger::FollowUp)), now + FOLLOW_UP, false, READY),
            Some(Trigger::FollowUp)
        );
    }

    #[test]
    fn the_connect_report_runs_once_per_app_run() {
        let now = Instant::now();
        assert_eq!(
            schedule_on_connect(None, false, now),
            Some((now + FIRST_DELAY, Trigger::Connect)),
            "the first connection this run"
        );
        let pending = Some((now, Trigger::Connect));
        let later = now + Duration::from_secs(5);
        assert_eq!(
            schedule_on_connect(pending, false, later),
            Some((later + FIRST_DELAY, Trigger::Connect)),
            "a reconnect before it started pushes it back"
        );
        // Started: reconnects leave the schedule alone, so the follow-up still runs and nothing repeats.
        let follow_up = due_after(Trigger::Connect, now);
        assert_eq!(follow_up, Some((now + FOLLOW_UP, Trigger::FollowUp)));
        assert_eq!(schedule_on_connect(follow_up, true, later), follow_up);
        assert_eq!(schedule_on_connect(None, true, later), None, "no second Connect report");
        assert_eq!(due_after(Trigger::FollowUp, now), None, "the follow-up is the last");
        assert_eq!(due_after(Trigger::OnDemand, now), None);
    }

    #[test]
    fn only_a_link_that_stopped_answering_ends_the_pass() {
        assert!(ends_pass(&BleError::Unreachable), "an unreachable phone");
        assert!(ends_pass(&BleError::TimedOut), "a timeout");
        let closed = windows::core::Error::from(windows::core::HRESULT(0x8000_0013_u32 as i32));
        assert!(ends_pass(&BleError::Win(closed)), "Windows closed tug's GATT objects");
        assert!(
            !ends_pass(&BleError::Protocol(Some(0x0E))),
            "the phone answered with an ATT error"
        );
        assert!(!ends_pass(&BleError::Protocol(None)));
        assert!(!ends_pass(&BleError::AccessDenied));
        assert!(!ends_pass(&BleError::NotFound("x")));
        let other = windows::core::Error::from(windows::core::HRESULT(0x8007_0005_u32 as i32));
        assert!(!ends_pass(&BleError::Win(other)));
        assert!(matches!(StepError::from(BleError::Unreachable), StepError::LinkGone(_)));
        assert!(matches!(
            StepError::from(BleError::Protocol(Some(2))),
            StepError::Part(_)
        ));
        assert!(matches!(
            status_error(GattCommunicationStatus::Unreachable),
            StepError::LinkGone(_)
        ));
        assert!(matches!(
            status_error(GattCommunicationStatus::ProtocolError),
            StepError::Part(_)
        ));
        assert!(matches!(
            status_error(GattCommunicationStatus::AccessDenied),
            StepError::Part(_)
        ));
    }

    #[test]
    fn a_stopped_pass_never_decodes_a_service_it_didnt_finish() {
        let dis = gatt::sig(gatt::DEVICE_INFORMATION);
        let battery_svc = gatt::sig(gatt::BATTERY_SERVICE);
        let mut pass = Pass {
            service_uuids: vec![dis, battery_svc],
            ..Default::default()
        };
        pass.completed.insert(dis);
        pass.stopped = Some(stop_text("timeout"));
        assert!(pass.walked(dis).is_ok(), "finished before the stop");
        assert!(
            pass.walked(battery_svc).unwrap_err().starts_with("stopped:"),
            "says the pass stopped, not that the service is missing"
        );
        assert_eq!(battery(&pass), Probe::Unavailable(pass.stopped.clone().unwrap()));
        assert_eq!(
            pass.walked(gatt::sig(gatt::CURRENT_TIME_SERVICE)),
            Err(NOT_PRESENT.to_string())
        );
        pass.stopped = None;
        assert_eq!(
            pass.walked(battery_svc),
            Err("its characteristics couldn't be listed".to_string())
        );
    }

    #[test]
    fn cancelling_aborts_the_report_answers_waiters_and_frees_the_turn() {
        static TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, async {
            let run = Rc::new(RefCell::new(Run::default()));
            let (tx, rx) = oneshot::channel();
            run.borrow_mut().waiters.push(tx);
            // A report stuck mid-pass while holding the turn (like an AMS pair on a stale link).
            let task = tokio::task::spawn_local(async {
                let _turn = TURN.lock().await;
                std::future::pending::<()>().await;
            });
            run.borrow_mut().current = Some((Trigger::Connect, task.abort_handle()));
            tokio::task::yield_now().await;
            assert!(TURN.try_lock().is_err(), "the report holds the turn");

            assert!(cancel_run(&run, DISCONNECTED));
            assert!(run.borrow().current.is_none(), "no longer busy");
            assert!(run.borrow().waiters.is_empty());
            let report = rx.await.expect("the waiter is answered");
            assert_eq!(report.trigger, Trigger::Connect);
            assert_eq!(report.gatt, Probe::Unavailable(DISCONNECTED.into()));
            assert!(task.await.unwrap_err().is_cancelled());
            assert!(TURN.try_lock().is_ok(), "aborting dropped the turn");
            assert!(!cancel_run(&run, DISCONNECTED), "nothing left to cancel");
        });
    }
}
