//! The Bluetooth actor: advertising, discovery, pairing, and the GATT session
//! with the iPhone (ANCS notifications, AMS media, Battery Service).
//!
//! Connection model. The iPhone never advertises for us, so the PC advertises a
//! connectable tug GATT service and the iPhone connects to it (first time
//! via a BLE app such as LightBlue, afterwards iOS reconnects to the bonded PC
//! by itself). Once linked, the PC acts as GATT *client* of the iPhone's ANCS,
//! AMS and Battery services over that same link. `GattSession::MaintainConnection`
//! asks Windows to keep re-establishing the link whenever the phone is in range.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::sync_channel;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use windows::core::{Interface, GUID, HSTRING};
use windows::Devices::Bluetooth::GenericAttributeProfile::{
    GattCharacteristic, GattCharacteristicProperties, GattDeviceService, GattLocalCharacteristicParameters,
    GattProtectionLevel, GattServiceProvider, GattServiceProviderAdvertisementStatus,
    GattServiceProviderAdvertisementStatusChangedEventArgs, GattServiceProviderAdvertisingParameters, GattSession,
};
use windows::Devices::Bluetooth::{
    BluetoothAdapter, BluetoothCacheMode, BluetoothConnectionStatus, BluetoothDevice, BluetoothError, BluetoothLEDevice,
};
use windows::Devices::Enumeration::{
    DeviceInformation, DeviceInformationCustomPairing, DeviceInformationKind, DeviceInformationUpdate,
    DevicePairingKinds, DevicePairingProtectionLevel, DevicePairingRequestedEventArgs, DevicePairingResultStatus,
    DeviceWatcher,
};
use windows::Devices::Radios::{Radio, RadioKind, RadioState as WinRadioState};
use windows::Foundation::{IReference, TypedEventHandler};
use windows_collections::IIterable;

use super::winrt::{self, BleError, Subscription};
use super::{Command, Reply};
use crate::ams::{self, NowPlaying};
use crate::ancs::{self, Category, EventFlags, EventId, ParseError, Response};
use crate::ancs_queue::{Request, RequestQueue, MAX_ATTEMPTS};
use crate::state::{
    events, keys, AdvertisingState, AppName, ConnectionState, DiscoveredDevice, PairedDevice, PairingRequest,
    RadioState, Services, Shared, Transport,
};
use crate::store::NewNotification;

mod link;
mod media;
mod notifications;
mod pairing;
/// tug's own advertised GATT service, so the iPhone has something to connect to.
mod peripheral;

const TUG_SERVICE: u128 = 0x6E4C3A10_9B2F_4D7A_8C1E_5A0F2B7D9C01;
const TUG_INFO: u128 = 0x6E4C3A11_9B2F_4D7A_8C1E_5A0F2B7D9C01;

const BATTERY_SERVICE: u16 = 0x180F;
const BATTERY_LEVEL: u16 = 0x2A19;

/// Association-endpoint protocol ids for device watchers.
const AEP_PROTOCOL_LE: &str = "{bb7bb05e-5972-42b5-94fc-76eaa7084d49}";
const AEP_PROTOCOL_CLASSIC: &str = "{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}";
const PROP_IS_CONNECTED: &str = "System.Devices.Aep.IsConnected";

const ANCS_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
const RETRY_CONNECTED_SECS: u32 = 2;
const RETRY_IDLE_SECS: u32 = 10;
const ADVERTISE_RETRY_SECS: u32 = 3;
/// Quiet period after the last replayed notification before sweeping stale rows.
// Long enough that a slow replay's gaps aren't mistaken for its end (that swept, then
// restored, notifications still on the phone: a visible flicker).
const REPLAY_SETTLE: Duration = Duration::from_secs(8);
const CCCD_CHECK_SECS: u32 = 15;
const NOT_SHARING: &str = "Your iPhone is connected but isn't sharing notifications with this PC. On the iPhone: Settings › Bluetooth › tap ⓘ next to this PC › turn on Share System Notifications.";
const PIN_CONFIRM_TIMEOUT: Duration = Duration::from_secs(60);

enum Event {
    NotificationSource {
        gen: u64,
        data: Vec<u8>,
    },
    DataSource {
        gen: u64,
        data: Vec<u8>,
    },
    MediaEntity {
        gen: u64,
        data: Vec<u8>,
    },
    MediaCommands {
        gen: u64,
        data: Vec<u8>,
    },
    Battery {
        gen: u64,
        data: Vec<u8>,
    },
    Connection {
        gen: u64,
        connected: bool,
    },
    Advertising(GattServiceProviderAdvertisementStatus),
    Radio(RadioState),
    DeviceAdded(DeviceInformation, Transport),
    DeviceUpdated(DeviceInformationUpdate),
    DeviceRemoved(String),
    PairingDone {
        id: String,
        result: Result<(), String>,
        reply: Reply,
    },
}

struct Ancs {
    // The service handle must outlive its characteristics: if Windows closes the
    // service, their ValueChanged notifications stop without any error.
    _service: GattDeviceService,
    control_point: GattCharacteristic,
    // Their notification handlers live exactly as long as these do.
    notification_source: Subscription,
    data_source: Subscription,
    /// Control Point requests, one in flight at a time (see ancs_queue).
    requests: RequestQueue,
    reassembler: ancs::Reassembler,
    /// While a late reply to a timed-out request is being read, the request to
    /// listen for again once it's done.
    resume: Option<Request>,
    meta: HashMap<u32, (EventFlags, Category)>,
    rows: HashMap<u32, i64>,
    asked_apps: HashSet<String>,
    /// Set until the post-subscribe replay has settled and stale rows are swept.
    sweep_after: Option<Instant>,
}

impl Ancs {
    /// A request was abandoned after its retries. For an app's name, forget that we
    /// asked, so the app's next notification tries again instead of tug showing the raw
    /// bundle id until it restarts.
    fn gave_up_on(&mut self, r: &Request) {
        if let Request::App(app_id) = r {
            self.asked_apps.remove(app_id);
        }
    }
}

struct Media {
    _service: GattDeviceService,
    remote_command: GattCharacteristic,
    _remote_updates: Subscription,
    _entity_update: Subscription,
}

struct Battery {
    _service: GattDeviceService,
    /// None when the phone only allows reading the level, not notifications.
    _level: Option<Subscription>,
}

/// One opened iPhone. Survives disconnects; services are rebuilt on reconnect.
struct Link {
    /// Stamp for this link's connection events.
    gen: u64,
    /// Stamp for the current GATT subscription; changes on every (re)subscribe so
    /// events still queued from before a disconnect are ignored.
    sub_gen: u64,
    device: BluetoothLEDevice,
    _gatt_session: Option<GattSession>,
    connected: bool,
    /// Identifies this ANCS subscription; iOS notification UIDs are only valid within it.
    session_id: Option<String>,
    ancs: Option<Ancs>,
    media: Option<Media>,
    _battery: Option<Battery>,
}

struct Discovered {
    info: DeviceInformation,
    transport: Transport,
}

pub(super) async fn run(shared: Arc<Shared>, mut commands: UnboundedReceiver<Command>) {
    let (tx, mut events) = unbounded_channel();
    let mut actor = Actor {
        shared,
        tx,
        gen: 0,
        provider: None,
        _radio: None,
        watchers: Vec::new(),
        discovered: HashMap::new(),
        discovered_dirty: false,
        device_id: None,
        link: None,
        retry_in: 0,
        advertise_retry_in: None,
        carried_name: None,
        cccd_check_in: CCCD_CHECK_SECS,
    };
    actor.init().await;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            Some(cmd) = commands.recv() => actor.command(cmd).await,
            Some(ev) = events.recv() => actor.event(ev).await,
            _ = tick.tick() => actor.tick().await,
            else => break,
        }
    }
}

struct Actor {
    shared: Arc<Shared>,
    tx: UnboundedSender<Event>,
    gen: u64,
    provider: Option<GattServiceProvider>,
    _radio: Option<Radio>,
    watchers: Vec<DeviceWatcher>,
    discovered: HashMap<String, Discovered>,
    discovered_dirty: bool,
    device_id: Option<String>,
    link: Option<Link>,
    /// Seconds until the next connection attempt.
    retry_in: u32,
    /// Seconds until advertising is retried after Windows aborted it.
    advertise_retry_in: Option<u32>,
    /// Name of a Classic-paired iPhone whose LE side is being paired on its behalf.
    carried_name: Option<String>,
    /// Seconds until the ANCS subscription is verified on the iPhone again.
    cccd_check_in: u32,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn guid(u: u128) -> GUID {
    GUID::from_u128(u)
}

impl Actor {
    async fn init(&mut self) {
        if let Err(e) = self.watch_radio().await {
            log::warn!("radio state unavailable: {e}");
        }
        match self.peripheral_supported().await {
            Ok(supported) => {
                log::info!("Bluetooth adapter found; peripheral role supported: {supported}");
                self.shared.update_status(|s| s.peripheral_supported = Some(supported));
            }
            Err(e) => log::warn!("no Bluetooth adapter: {e}"),
        }
        let advertise = self.shared.store.setting(keys::ADVERTISE).ok().flatten();
        if advertise.as_deref() != Some("false") {
            self.start_advertising().await;
        }
        let store = &self.shared.store;
        if let (Ok(Some(id)), Ok(name)) = (store.setting(keys::DEVICE_ID), store.setting(keys::DEVICE_NAME)) {
            let name = name.unwrap_or_else(|| "iPhone".into());
            self.shared.update_status(|s| {
                s.device = Some(PairedDevice { id: id.clone(), name });
                s.connection = ConnectionState::Disconnected;
            });
            self.device_id = Some(id);
            self.retry_in = 0;
        }
    }

    async fn command(&mut self, cmd: Command) {
        match cmd {
            Command::PerformAction { id, positive, reply } => {
                let _ = reply.send(self.perform_action(id, positive).await);
            }
            Command::Media { command, reply } => {
                let res = match self.link.as_ref().and_then(|l| l.media.as_ref()) {
                    Some(m) => {
                        let ch = m.remote_command.clone();
                        winrt::write(&ch, &[command.id()]).await.map_err(|e| e.to_string())
                    }
                    None => Err("Media controls aren't available right now".into()),
                };
                let _ = reply.send(res);
            }
            Command::StartDiscovery => {
                if let Err(e) = self.start_discovery() {
                    self.set_error(format!("Couldn't scan for devices: {}", e.message()));
                }
            }
            Command::StopDiscovery => self.stop_discovery(),
            Command::Pair { id, reply } => self.pair(id, reply),
            Command::UseDevice { id, reply } => match self.use_device(&id).await {
                Err(e) => match self.le_side_of_classic(&id) {
                    // Classic-only pairing (calls/audio): pair the same phone's LE
                    // side, which LightBlue has connected, and keep the Classic name.
                    Some((le_id, name)) => {
                        log::info!("{name} has no LE record; pairing its connected LE side {le_id}");
                        self.carried_name = Some(name);
                        self.pair(le_id, reply);
                    }
                    None => {
                        let _ = reply.send(Err(e));
                    }
                },
                ok => {
                    let _ = reply.send(ok);
                }
            },
            Command::Forget { reply } => {
                let _ = reply.send(self.forget().await);
            }
            Command::SetAdvertising { enabled, reply } => {
                let _ = self
                    .shared
                    .store
                    .set_setting(keys::ADVERTISE, if enabled { "true" } else { "false" });
                if enabled {
                    self.start_advertising().await;
                } else {
                    self.stop_advertising();
                }
                let _ = reply.send(Ok(()));
            }
        }
    }

    async fn event(&mut self, ev: Event) {
        let link_gen = self.link.as_ref().map(|l| l.gen);
        let current = self.link.as_ref().map(|l| l.sub_gen);
        match &ev {
            Event::NotificationSource { gen, data } => {
                log::debug!("ANCS event {data:02X?} (gen {gen}, current {current:?})")
            }
            Event::DataSource { gen, data } => log::debug!("ANCS data {} bytes (gen {gen})", data.len()),
            Event::MediaEntity { data, .. } => log::debug!("AMS update {} bytes", data.len()),
            _ => {}
        }
        match ev {
            Event::NotificationSource { gen, data } if Some(gen) == current => self.on_notification_source(&data).await,
            Event::DataSource { gen, data } if Some(gen) == current => self.on_data_source(&data).await,
            Event::MediaEntity { gen, data } if Some(gen) == current => {
                self.shared
                    .update_now_playing(|np| np.apply_entity_update(&data, now_ms()));
            }
            Event::MediaCommands { gen, data } if Some(gen) == current => {
                self.shared.update_now_playing(|np| np.apply_available_commands(&data));
            }
            Event::Battery { gen, data } if Some(gen) == current => {
                if let Some(&level) = data.first() {
                    self.shared.update_status(|s| s.battery = Some(level.min(100)));
                }
            }
            Event::Connection { gen, connected } if Some(gen) == link_gen => self.on_connection(connected),
            Event::NotificationSource { .. }
            | Event::DataSource { .. }
            | Event::MediaEntity { .. }
            | Event::MediaCommands { .. }
            | Event::Battery { .. }
            | Event::Connection { .. } => log::debug!("ignored event from a replaced link"),
            Event::Advertising(status) => {
                let name = match status {
                    GattServiceProviderAdvertisementStatus::Created => "created",
                    GattServiceProviderAdvertisementStatus::Stopped => "stopped",
                    GattServiceProviderAdvertisementStatus::Started => "started",
                    GattServiceProviderAdvertisementStatus::Aborted => "aborted",
                    GattServiceProviderAdvertisementStatus::StartedWithoutAllAdvertisementData => {
                        "started without all advertisement data"
                    }
                    _ => "unknown",
                };
                log::info!("advertising status: {name}");
                let state = match status {
                    GattServiceProviderAdvertisementStatus::Started
                    | GattServiceProviderAdvertisementStatus::StartedWithoutAllAdvertisementData => {
                        AdvertisingState::On
                    }
                    GattServiceProviderAdvertisementStatus::Aborted => AdvertisingState::Error,
                    _ => AdvertisingState::Off,
                };
                self.shared.update_status(|s| s.advertising = state);
                // Windows reports Aborted briefly on every start, and for real when the
                // radio blips or another app held the service. Retry unless it recovers.
                match status {
                    GattServiceProviderAdvertisementStatus::Aborted => {
                        self.advertise_retry_in.get_or_insert(ADVERTISE_RETRY_SECS);
                    }
                    GattServiceProviderAdvertisementStatus::Started
                    | GattServiceProviderAdvertisementStatus::StartedWithoutAllAdvertisementData => {
                        self.advertise_retry_in = None;
                    }
                    _ => {}
                }
            }
            Event::Radio(state) => {
                self.shared.update_status(|s| s.radio = state);
                if state == RadioState::On {
                    self.retry_in = 0;
                }
            }
            Event::DeviceAdded(info, transport) => {
                if let Ok(id) = info.Id() {
                    self.discovered.insert(id.to_string(), Discovered { info, transport });
                    self.discovered_dirty = true;
                }
            }
            Event::DeviceUpdated(update) => {
                if let Ok(id) = update.Id() {
                    if let Some(d) = self.discovered.get(&id.to_string()) {
                        let _ = d.info.Update(&update);
                        self.discovered_dirty = true;
                    }
                }
            }
            Event::DeviceRemoved(id) => {
                self.discovered.remove(&id);
                self.discovered_dirty = true;
            }
            Event::PairingDone { id, result, reply } => {
                log::info!("pairing finished: {result:?}");
                let result = match result {
                    Ok(()) => self.use_device(&id).await,
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
        }
    }

    async fn tick(&mut self) {
        if let Some(secs) = self.advertise_retry_in {
            if secs > 0 {
                self.advertise_retry_in = Some(secs - 1);
            } else {
                self.advertise_retry_in = None;
                log::info!("advertising aborted; restarting");
                self.stop_advertising();
                self.start_advertising().await;
            }
        }

        if self.cccd_check_in > 0 {
            self.cccd_check_in -= 1;
        } else {
            self.cccd_check_in = CCCD_CHECK_SECS;
            self.verify_ancs_subscription().await;
        }

        if self.discovered_dirty {
            self.discovered_dirty = false;
            self.emit_discovered();
        }

        // A lost Data Source response would otherwise stall the queue forever.
        let stalled = self
            .link
            .as_ref()
            .and_then(|l| l.ancs.as_ref())
            .is_some_and(|a| a.requests.timed_out(Instant::now(), ANCS_RESPONSE_TIMEOUT));
        if stalled {
            if let Some(a) = self.link.as_mut().and_then(|l| l.ancs.as_mut()) {
                let req = a.requests.inflight().cloned();
                a.reassembler.reset();
                a.resume = None;
                match a.requests.fail_inflight() {
                    Some(gave_up) => {
                        log::warn!("ANCS request {gave_up:?} timed out {MAX_ATTEMPTS} times; giving up");
                        a.gave_up_on(&gave_up);
                    }
                    None => log::info!("ANCS request {req:?} timed out; retrying"),
                }
            }
            self.pump().await;
        }

        self.sweep_if_settled();

        let ready = self.link.as_ref().is_some_and(|l| l.connected && l.ancs.is_some());
        if self.device_id.is_none() || ready || self.shared.status().radio == RadioState::Off {
            return;
        }
        if self.retry_in > 0 {
            self.retry_in -= 1;
            return;
        }
        self.connect().await;
    }

    fn set_error(&self, message: String) {
        self.shared.update_status(|s| s.last_error = Some(message));
    }
}
