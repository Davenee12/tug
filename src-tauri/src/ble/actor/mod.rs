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
use crate::device_info;
use crate::link_policy;
use crate::state::{
    events, keys, AdvertisingState, AppName, ConnectionState, DiscoveredDevice, PairedDevice, PairingRequest,
    RadioState, Services, Shared, Transport,
};
use crate::store::NewNotification;
use crate::wake::WakeCause;
use crate::wedge::{WedgeWatch, Wedged};

/// The wake watch: tells the actor when the PC resumed, from a thread of its own.
mod heartbeat;
/// The Bluetooth inventory probe (`crate::bt_inventory`), run off the connect path.
mod inventory;
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
/// Longest wait between connect attempts once they keep failing.
const MAX_RETRY_SECS: u32 = 30;
/// The iPhone is connected but keeps not offering ANCS — locked after a restart, or mid-update
/// before its first unlock. Those attempts can't succeed until it's unlocked, so back off far
/// further than an ordinary failure (up to 2 min) instead of hammering it every 30 s all night,
/// while still retrying so it reconnects on its own within 2 min of the morning unlock.
const UNLOCK_RETRY_SECS: u32 = 30;
const MAX_UNLOCK_RETRY_SECS: u32 = 120;
/// After this many failures in a row, treat link-up blips as flapping.
const FLAPPING_AFTER: u32 = 3;
const FLAP_SETTLE_SECS: u32 = 5;
/// A link-down shorter than this is a blip: the GATT subscription (and the pre-existing
/// notifications iOS is replaying on it) survives, so keep ANCS instead of tearing it down and
/// losing the replay. Only a down that outlasts this is treated as a real disconnect.
const LINK_BLIP_GRACE: Duration = Duration::from_millis(1500);
const ADVERTISE_RETRY_SECS: u32 = 3;
/// Longest wait between attempts to start advertising while the adapter keeps timing out.
const MAX_ADVERTISE_RETRY_SECS: u32 = 60;
/// Quiet period after the last replayed notification before sweeping stale rows.
// Long enough that a slow replay's gaps aren't mistaken for its end (that swept, then
// restored, notifications still on the phone: a visible flicker).
const REPLAY_SETTLE: Duration = Duration::from_secs(8);
const CCCD_CHECK_SECS: u32 = 15;
/// While the user watches the iPhone's switches: Share System Notifications shows up in ~2 s.
const CCCD_CHECK_WATCHING_SECS: u32 = 2;
const NOT_SHARING: &str = "Your iPhone is connected but isn't sharing notifications with this PC. On the iPhone: Settings › Bluetooth › tap ⓘ next to this PC › turn on Share System Notifications.";
const PIN_CONFIRM_TIMEOUT: Duration = Duration::from_secs(60);
/// What an action or media command says while tug is rebuilding the link on its own.
const RECONNECTING: &str = "Reconnecting to your iPhone. Try again in a moment.";

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
    /// The phone's Model Number String (Device Information Service), read once per connection.
    Model {
        gen: u64,
        data: Vec<u8>,
    },
    Connection {
        gen: u64,
        connected: bool,
        /// When Windows reported the edge (stamped in its handler), so the blip debounce measures
        /// the real outage, not however long the actor took to get to the event.
        at: Instant,
    },
    /// The phone's Bluetooth name changed (renamed in Settings › General › About).
    Name {
        gen: u64,
        name: String,
    },
    /// The wake watch saw the PC resume from sleep (see `crate::wake`). Carries roughly how long it
    /// was away and which signal said so, for the log.
    Woke {
        slept: Duration,
        cause: WakeCause,
    },
    Advertising(GattServiceProviderAdvertisementStatus),
    Radio(RadioState),
    DeviceAdded(DeviceInformation, Transport),
    DeviceUpdated(DeviceInformationUpdate),
    DeviceRemoved(String),
    PairingDone {
        id: String,
        result: Result<(), String>,
        /// Set when the device paired was an unpaired Classic iPhone, so success adopts the LE
        /// bond cross-transport derivation creates rather than the Classic id directly. The LE bond
        /// is resolved off the actor loop (in the pairing task) and carried here, so adoption never
        /// blocks the loop waiting for it.
        classic: Option<ClassicPairing>,
        reply: Reply,
    },
}

/// A just-paired Classic iPhone ready to adopt: the phone's name, and the LE bond cross-transport
/// derivation created (resolved in the pairing task, so the actor loop isn't blocked waiting). None
/// when no LE bond appeared within the wait — adoption then falls back or asks the user to retry.
struct ClassicPairing {
    name: String,
    resolved_le: Option<String>,
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
    /// Reads full values of truncated updates; None if the phone didn't expose it.
    entity_attribute: Option<GattCharacteristic>,
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
    // On its own thread, not this one: a Windows call blocking the Bluetooth thread must neither be
    // mistaken for a wake nor delay a real one.
    heartbeat::spawn(tx.clone());
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
        connect_failures: 0,
        advertise_retry_in: None,
        advertise_failures: 0,
        carried_name: None,
        cccd_check_in: CCCD_CHECK_SECS,
        optional_retry_at: None,
        link_down_at: None,
        wedge: WedgeWatch::default(),
        wedge_relink_at: None,
        inventory: inventory::InventoryState::default(),
        media_gate: ams::CommandGate::default(),
        away_since: None,
        last_poke: None,
        radio_recheck_in: link_policy::RADIO_RECHECK_SECS,
        connected_since_adopt: true,
        adopt_timeouts: 0,
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
    watchers: Vec<(Transport, DeviceWatcher)>,
    discovered: HashMap<String, Discovered>,
    discovered_dirty: bool,
    device_id: Option<String>,
    link: Option<Link>,
    /// Seconds until the next connection attempt.
    retry_in: u32,
    /// Connect attempts that failed in a row; drives the backoff.
    connect_failures: u32,
    /// Seconds until advertising is retried after Windows aborted it.
    advertise_retry_in: Option<u32>,
    /// Attempts to start advertising that timed out in a row; drives that retry's backoff.
    advertise_failures: u32,
    /// Name of a Classic-paired iPhone whose LE side is being paired on its behalf.
    carried_name: Option<String>,
    /// Seconds until the ANCS subscription is verified on the iPhone again.
    cccd_check_in: u32,
    /// Last retry of media/battery discovery: full GATT discovery, so it keeps the slow
    /// cadence even while the user watches the switches (and the probe runs every 2 s).
    optional_retry_at: Option<Instant>,
    /// When the link last went down (as stamped by Windows' event) and is waiting out the blip
    /// grace before a real teardown. Set on a disconnect, cleared by the next link-up or by
    /// `finish_link_down`.
    link_down_at: Option<Instant>,
    /// Watches GATT requests on the live link for a run of timeouts: an adapter that stopped answering.
    wedge: WedgeWatch,
    /// Set while recovering from such a stall: requests are paused until then, when the link is
    /// rebuilt. Cleared by any link teardown.
    wedge_relink_at: Option<Instant>,
    /// When the next Bluetooth inventory report is due, and the one running now.
    inventory: inventory::InventoryState,
    /// One press, one media command (drops presses that queued up behind a stalled write).
    media_gate: ams::CommandGate,
    /// Since when the phone has been away (down past the blip grace, or unreachable). Sticky:
    /// cleared only by a successful connect, so failed retries don't flip the status.
    away_since: Option<Instant>,
    /// When a connect last tried a link Windows reports down (see `link_policy::wait_for_link_up`).
    last_poke: Option<Instant>,
    /// Seconds until the radio is read again while it isn't On.
    radio_recheck_in: u32,
    /// Whether the adopted phone has connected since it was adopted (true for a remembered one).
    connected_since_adopt: bool,
    /// Timed-out connects since adopting the phone, while it hasn't connected yet.
    adopt_timeouts: u32,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Seconds before the next connect attempt: the base delay, doubling with each failure in
/// a row (a phone at the edge of range fails every few seconds, all night), capped.
fn retry_delay(base: u32, failures: u32, cap: u32) -> u32 {
    let doublings = failures.saturating_sub(1).min(5);
    base.saturating_mul(1 << doublings).min(cap)
}

/// Whether a deferred link-down has lasted long enough to be a real disconnect rather than a
/// sub-second blip. Pure, so the blip debounce is unit-tested.
fn link_down_is_real(down_for: Duration) -> bool {
    down_for >= LINK_BLIP_GRACE
}

/// What a link-up means for the ANCS session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkUp {
    /// No pending link-down: an ordinary (re)connect.
    Fresh,
    /// Down then up inside the blip grace: the GATT subscription survived, keep ANCS.
    Blip,
    /// Down past the grace, even if the teardown hasn't run yet: a real outage.
    AfterOutage,
}

/// Classify a link-up from the event-source stamps of the pending down (if any) and the up.
/// Pure, so the debounce is unit-tested; judged on Windows' timestamps, never on when the actor
/// happened to handle the events (it can be stuck in a bounded await for tens of seconds).
fn classify_link_up(down_at: Option<Instant>, up_at: Instant) -> LinkUp {
    match down_at {
        None => LinkUp::Fresh,
        Some(down) if link_down_is_real(up_at.saturating_duration_since(down)) => LinkUp::AfterOutage,
        Some(_) => LinkUp::Blip,
    }
}

/// Whether a GATT/ANCS event still belongs to the live subscription and should be processed. The
/// decision is purely the subscription generation: a momentary link-down flag doesn't invalidate
/// events that carry the current generation, because the GATT subscription (and the pre-existing
/// notifications iOS replays on it) outlives a sub-second connection blip.
fn event_is_current(event_gen: u64, current_sub_gen: Option<u64>) -> bool {
    current_sub_gen == Some(event_gen)
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
            // The last model the phone reported, so the sidebar pictures it before it reconnects.
            let model = store.setting(keys::DEVICE_MODEL).ok().flatten();
            self.shared.update_status(|s| {
                s.device = Some(PairedDevice {
                    id: id.clone(),
                    name,
                    model,
                });
                s.connection = ConnectionState::Disconnected;
            });
            self.device_id = Some(id);
            self.retry_in = 0;
        }
    }

    async fn command(&mut self, cmd: Command) {
        match cmd {
            Command::PerformAction { id, positive, reply } => {
                let result = self.perform_action(id, positive).await;
                if let Err(e) = &result {
                    log::info!("notification action on row {id} not done: {e}");
                }
                let _ = reply.send(result);
            }
            Command::Media {
                command,
                requested_at,
                report_repeat,
                reply,
            } => {
                let res = self.send_media_command(command, requested_at, report_repeat).await;
                let _ = reply.send(res);
            }
            Command::StartDiscovery => {
                if let Err(e) = self.start_discovery() {
                    self.set_error(format!("Couldn't scan for devices: {}", e.message()));
                }
            }
            Command::StopDiscovery => self.stop_discovery(),
            Command::RescanDiscovery => {
                if let Err(e) = self.rescan_classic() {
                    log::debug!("rescan for the iPhone failed: {}", e.message());
                }
            }
            Command::Pair { id, reply } => self.pair(id, reply),
            Command::RemovePairing { id, reply } => {
                let _ = reply.send(self.remove_pairing(id).await);
            }
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
            Command::PairTexts { reply } => {
                let _ = reply.send(self.pair_texts().await);
            }
            Command::Forget { reply } => {
                let _ = reply.send(self.forget().await);
            }
            Command::SetAdvertising { enabled, reply } => {
                let _ = self
                    .shared
                    .store
                    .set_setting(keys::ADVERTISE, if enabled { "true" } else { "false" });
                if enabled {
                    // The user asked: try now, with a fresh backoff.
                    self.advertise_failures = 0;
                    self.start_advertising().await;
                } else {
                    self.stop_advertising();
                }
                let _ = reply.send(Ok(()));
            }
            Command::Inventory { reply } => self.request_inventory(reply),
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
            _ => {}
        }
        match ev {
            Event::NotificationSource { gen, data } if event_is_current(gen, current) => {
                self.on_notification_source(&data).await
            }
            Event::DataSource { gen, data } if event_is_current(gen, current) => self.on_data_source(&data).await,
            Event::MediaEntity { gen, data } if event_is_current(gen, current) => self.on_media_entity(&data).await,
            Event::MediaCommands { gen, data } if event_is_current(gen, current) => self.on_media_commands(&data),
            Event::Battery { gen, data } if event_is_current(gen, current) => {
                if let Some(&level) = data.first() {
                    self.shared.update_status(|s| s.battery = Some(level.min(100)));
                }
            }
            Event::Model { gen, data } if event_is_current(gen, current) => self.on_model_number(&data),
            Event::Connection { gen, connected, at } if Some(gen) == link_gen => {
                self.on_connection(connected, at).await
            }
            Event::Name { gen, name } if Some(gen) == link_gen => self.set_device_name(&name),
            Event::Woke { slept, cause } => {
                let how = match cause {
                    WakeCause::Slept => "the PC slept",
                    WakeCause::Resumed => "Windows resumed",
                    WakeCause::Frozen => "tug was suspended",
                };
                log::info!(
                    "woke after ~{}s away ({how}); retrying the iPhone link and messages now",
                    slept.as_secs()
                );
                // A real change: drop the backoff and retry at once instead of waiting it out.
                self.connect_failures = 0;
                self.link_down_at = None;
                self.retry_in = 0;
                self.last_poke = None;
                self.shared.update_status(|s| s.awaiting_unlock = false);
                // After resume the old GATT handles can be stale and Windows may never fire a
                // reconnect for them. Read the subscription back (bounded): a link that answers is
                // kept, so a short sleep doesn't cost a full reconnect and its notification replay.
                if self.link.is_some() {
                    let check = self.wake_check().await;
                    let (linked, subscribed) = self
                        .link
                        .as_ref()
                        .map_or((false, false), |l| (l.connected, l.ancs.is_some()));
                    if link_policy::keep_link_after_wake(linked, subscribed, check) {
                        log::info!("the iPhone link survived the sleep ({check:?}); keeping it");
                        self.cccd_check_in = 0;
                    } else {
                        self.relink(&format!("woke ({how}; link check: {check:?})"));
                    }
                }
                // The radio may have changed while asleep without an event reaching tug.
                if self.shared.status().radio != RadioState::On {
                    self.recheck_radio().await;
                }
                // Nudge the texts/contacts/calls worker to rebuild its MAP session too.
                if let Some(map) = self.shared.map.get() {
                    map.refresh();
                }
            }
            Event::NotificationSource { .. }
            | Event::DataSource { .. }
            | Event::MediaEntity { .. }
            | Event::MediaCommands { .. }
            | Event::Battery { .. }
            | Event::Model { .. }
            | Event::Connection { .. }
            | Event::Name { .. } => log::debug!("ignored event from a replaced link"),
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
                        self.advertise_failures = 0;
                    }
                    _ => {}
                }
            }
            Event::Radio(state) => {
                log::info!("Bluetooth radio: {state:?}");
                self.shared.update_status(|s| {
                    s.radio = state;
                    // With Bluetooth off there's nothing to reconnect: the UI says it's off instead.
                    if matches!(state, RadioState::Off | RadioState::Unavailable) {
                        s.reconnecting = false;
                        // `tick` doesn't connect with the radio off, so a relink that had set
                        // "connecting" would otherwise say "Connecting…" until Bluetooth came back.
                        if s.connection == ConnectionState::Connecting {
                            s.connection = ConnectionState::Disconnected;
                        }
                    }
                });
                if state == RadioState::On {
                    self.retry_in = 0;
                    self.connect_failures = 0;
                }
            }
            Event::DeviceAdded(info, transport) => {
                if let Ok(id) = info.Id() {
                    // Discovery was effectively silent in the logs; name the candidates so a phone
                    // that appears and vanishes mid-pairing can be diagnosed.
                    let name = info.Name().map(|n| n.to_string()).unwrap_or_default();
                    let connected = pairing::bool_property(&info, PROP_IS_CONNECTED);
                    // Name the candidate and what tug takes it for, so a discoverable Classic iPhone
                    // (and a phone that appears then vanishes mid-pairing) is clear in the log.
                    let kind = crate::device_kind::classify(
                        &name,
                        pairing::uint_property(&info, pairing::PROP_LE_APPEARANCE).and_then(|a| u16::try_from(a).ok()),
                        pairing::uint_property(&info, pairing::PROP_COD_MAJOR),
                    );
                    log::info!("discovery: added {transport:?} {name:?} ({kind:?}, connected={connected})");
                    self.discovered.insert(id.to_string(), Discovered { info, transport });
                    self.discovered_dirty = true;
                }
            }
            Event::DeviceUpdated(update) => {
                if let Ok(id) = update.Id() {
                    if let Some(d) = self.discovered.get(&id.to_string()) {
                        let was = pairing::bool_property(&d.info, PROP_IS_CONNECTED);
                        let _ = d.info.Update(&update);
                        let now = pairing::bool_property(&d.info, PROP_IS_CONNECTED);
                        if was != now {
                            let name = d.info.Name().map(|n| n.to_string()).unwrap_or_default();
                            log::info!("discovery: {name:?} {}", if now { "connected" } else { "disconnected" });
                        }
                        self.discovered_dirty = true;
                    }
                }
            }
            Event::DeviceRemoved(id) => {
                if let Some(d) = self.discovered.get(&id) {
                    let name = d.info.Name().map(|n| n.to_string()).unwrap_or_default();
                    log::info!("discovery: removed {name:?}");
                }
                self.discovered.remove(&id);
                self.discovered_dirty = true;
            }
            Event::PairingDone {
                id,
                result,
                classic,
                reply,
            } => {
                log::info!("pairing finished: {result:?}");
                let result = match result {
                    // A Classic iPhone: adopt the LE bond derivation makes, remember Classic for texts.
                    Ok(()) => match classic {
                        Some(ctx) => self.adopt_le_after_classic(id, ctx).await,
                        None => self.use_device(&id).await,
                    },
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

        // The adapter stopped answering and has had its settle: rebuild the link now.
        if self.wedge_relink_at.is_some_and(|t| Instant::now() >= t) {
            log::info!("reconnecting to the iPhone after the Bluetooth stall");
            self.restart_link();
        }

        // A link-down that outlasted the blip grace is a real disconnect: tear ANCS down now.
        // (A shorter down→up blip keeps ANCS, so the pre-existing-notification replay isn't lost.)
        if self.link_down_at.is_some_and(|t| link_down_is_real(t.elapsed())) {
            self.finish_link_down();
        }

        if self.shared.watching() {
            self.cccd_check_in = self.cccd_check_in.min(CCCD_CHECK_WATCHING_SECS);
        }
        if self.cccd_check_in > 0 {
            self.cccd_check_in -= 1;
        } else {
            self.cccd_check_in = if self.shared.watching() {
                CCCD_CHECK_WATCHING_SECS
            } else {
                CCCD_CHECK_SECS
            };
            self.verify_ancs_subscription().await;
        }

        if self.discovered_dirty {
            self.discovered_dirty = false;
            self.emit_discovered();
        }

        // A radio that isn't On is read again now and then: a missed StateChanged would otherwise
        // leave a stale "off" blocking every connect below until tug restarted.
        if self.shared.status().radio == RadioState::On {
            self.radio_recheck_in = link_policy::RADIO_RECHECK_SECS;
        } else if self.radio_recheck_in > 0 {
            self.radio_recheck_in -= 1;
        } else {
            self.recheck_radio().await;
        }

        // A lost Data Source response would otherwise stall the queue forever. (Not while the queue is
        // paused for a stalled adapter: no request is failed for that.)
        let stalled = !self.wedged()
            && self
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
        self.inventory_tick();

        // While the user is on the iPhone screens (setup/Settings), a connect that's backed off
        // waiting for an unlock should retry promptly — they may be unlocking the phone right now.
        if self.shared.watching() && self.shared.status().awaiting_unlock {
            self.retry_in = self.retry_in.min(CCCD_CHECK_WATCHING_SECS);
        }

        let ready = self.link.as_ref().is_some_and(|l| l.connected && l.ancs.is_some());
        // Within the blip grace, don't reconnect: we're waiting to see if a down is just a flap. Nor
        // while a stalled adapter settles: the link is rebuilt once it has.
        let settling = self.link_down_at.is_some() || self.wedged();
        if self.device_id.is_none() || ready || settling || self.shared.status().radio == RadioState::Off {
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

    /// Recovering from a stalled adapter: requests are paused until the link is rebuilt.
    fn wedged(&self) -> bool {
        self.wedge_relink_at.is_some()
    }

    /// Feed the outcome of one GATT request on the live link to the wedge watch, and start recovering
    /// if it completes a run of timeouts. Returns whether the link is (now) wedged, so callers stop
    /// sending. Only requests on a link that reports connected count: a link that's down fails its
    /// requests for that reason, and the link-down path handles it.
    fn note_gatt<T>(&mut self, result: &Result<T, BleError>) -> bool {
        match result {
            Err(e) if e.is_timeout() => {
                let connected = self.link.as_ref().is_some_and(|l| l.connected);
                if connected && !self.wedged() {
                    match self.wedge.timed_out(Instant::now()) {
                        Some(Wedged::Relink { settle }) => self.begin_wedge_recovery(settle),
                        Some(Wedged::Persistent { first: true }) => log::warn!(
                            "Bluetooth adapter keeps stopping responding; no longer rebuilding the link for it until it's been quiet for {} min",
                            crate::wedge::RECUR_WITHIN.as_secs() / 60
                        ),
                        Some(Wedged::Persistent { first: false }) | None => {}
                    }
                }
            }
            // The phone answered, yes or no: the adapter is working.
            Ok(_) | Err(BleError::Protocol(_) | BleError::AccessDenied) => self.wedge.answered(),
            Err(_) => {}
        }
        self.wedged()
    }

    /// The link still reports connected but the adapter has stopped answering: stop sending, let it
    /// settle, then rebuild the link (from `tick`). Requests stay queued rather than being given up
    /// on; the rebuilt ANCS session re-fetches everything still on the phone, with fresh retries.
    fn begin_wedge_recovery(&mut self, settle: Duration) {
        log::warn!("Bluetooth adapter stopped responding; reconnecting");
        log::info!(
            "{} Bluetooth requests in a row went unanswered on a connected link; pausing requests and reconnecting in {}s",
            crate::wedge::STRIKES,
            settle.as_secs()
        );
        self.wedge_relink_at = Some(Instant::now() + settle);
        self.shared.update_status(|s| {
            s.connection = ConnectionState::Connecting;
            s.reconnecting = true;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_retries_back_off_then_cap() {
        let waits: Vec<u32> = (0..=8)
            .map(|f| retry_delay(RETRY_CONNECTED_SECS, f, MAX_RETRY_SECS))
            .collect();
        assert_eq!(waits, vec![2, 2, 4, 8, 16, 30, 30, 30, 30]);
        assert_eq!(retry_delay(RETRY_IDLE_SECS, 1, MAX_RETRY_SECS), 10);
        assert_eq!(retry_delay(RETRY_IDLE_SECS, 3, MAX_RETRY_SECS), 30);
        assert_eq!(
            retry_delay(RETRY_CONNECTED_SECS, u32::MAX, MAX_RETRY_SECS),
            MAX_RETRY_SECS,
            "no overflow"
        );
    }

    #[test]
    fn advertising_that_times_out_keeps_retrying_backing_off_to_a_minute() {
        // The review's block: a CreateAsync timeout used to leave advertising off for good.
        let waits: Vec<u32> = (1..=8)
            .map(|f| retry_delay(ADVERTISE_RETRY_SECS, f, MAX_ADVERTISE_RETRY_SECS))
            .collect();
        assert_eq!(waits, vec![3, 6, 12, 24, 48, 60, 60, 60]);
    }

    #[test]
    fn unlock_backoff_climbs_to_minutes_not_a_30s_loop() {
        // The overnight "ANCS not found" loop: back off (up to 2 min) instead of ~30 s forever,
        // while still retrying often enough that it reconnects within 2 min of the morning unlock.
        let waits: Vec<u32> = (1..=8)
            .map(|f| retry_delay(UNLOCK_RETRY_SECS, f, MAX_UNLOCK_RETRY_SECS))
            .collect();
        assert_eq!(waits, vec![30, 60, 120, 120, 120, 120, 120, 120]);
        assert!(waits.iter().all(|&w| w <= MAX_UNLOCK_RETRY_SECS));
    }

    #[test]
    fn a_sub_second_link_blip_is_not_a_real_disconnect() {
        // Keep ANCS (and the pre-existing-notification replay) through a quick down→up flap.
        assert!(!link_down_is_real(Duration::from_millis(200)));
        assert!(!link_down_is_real(Duration::from_millis(900)));
        assert!(link_down_is_real(LINK_BLIP_GRACE), "past the grace: a real disconnect");
        assert!(link_down_is_real(Duration::from_secs(5)));
    }

    #[test]
    fn a_link_up_is_judged_on_event_stamps_not_handling_time() {
        let down = Instant::now();
        assert_eq!(classify_link_up(None, down), LinkUp::Fresh, "no pending down");
        assert_eq!(
            classify_link_up(Some(down), down + Duration::from_millis(300)),
            LinkUp::Blip,
            "a quick flap keeps ANCS"
        );
        // The bug: an up arriving after the grace (before the 1 s tick tore down), or after the
        // actor sat 10-30 s in a bounded await, was taken for a blip and kept a stale session.
        assert_eq!(
            classify_link_up(Some(down), down + LINK_BLIP_GRACE),
            LinkUp::AfterOutage,
            "up just past the grace, before the tick ran"
        );
        assert_eq!(
            classify_link_up(Some(down), down + Duration::from_secs(25)),
            LinkUp::AfterOutage,
            "a long outage handled late"
        );
        // Both edges queued while the actor was stuck: they're still a blip by their own stamps.
        assert_eq!(
            classify_link_up(Some(down), down + Duration::from_millis(800)),
            LinkUp::Blip
        );
        // A stamp out of order can't panic or read as an outage.
        assert_eq!(
            classify_link_up(Some(down + Duration::from_secs(1)), down),
            LinkUp::Blip
        );
    }

    #[test]
    fn ancs_events_follow_the_live_subscription_generation() {
        // An event from the current subscription is kept even if a link-down flag flipped during a
        // blip; one from a replaced subscription is dropped. (The bug: a blip dropped current ones.)
        assert!(event_is_current(2, Some(2)), "current generation: keep");
        assert!(!event_is_current(2, Some(3)), "superseded by a resubscribe: drop");
        assert!(!event_is_current(2, None), "no live subscription: drop");
    }
}
