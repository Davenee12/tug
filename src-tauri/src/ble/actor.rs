//! The Bluetooth actor: advertising, discovery, pairing, and the GATT session
//! with the iPhone (ANCS notifications, AMS media, Battery Service).
//!
//! Connection model. The iPhone never advertises for us, so the PC advertises a
//! connectable tug GATT service and the iPhone connects to it (first time
//! via a BLE app such as LightBlue, afterwards iOS reconnects to the bonded PC
//! by itself). Once linked, the PC acts as GATT *client* of the iPhone's ANCS,
//! AMS and Battery services over that same link. `GattSession::MaintainConnection`
//! asks Windows to keep re-establishing the link whenever the phone is in range.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::sync_channel;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use windows::core::{Interface, GUID, HSTRING};
use windows::Devices::Bluetooth::GenericAttributeProfile::{
    GattCharacteristic, GattCharacteristicProperties, GattLocalCharacteristicParameters, GattProtectionLevel,
    GattServiceProvider, GattServiceProviderAdvertisementStatus,
    GattServiceProviderAdvertisementStatusChangedEventArgs, GattServiceProviderAdvertisingParameters, GattSession,
};
use windows::Devices::Bluetooth::{
    BluetoothAdapter, BluetoothConnectionStatus, BluetoothDevice, BluetoothError, BluetoothLEDevice,
};
use windows::Devices::Enumeration::{
    DeviceInformation, DeviceInformationCustomPairing, DeviceInformationKind, DeviceInformationUpdate,
    DevicePairingKinds, DevicePairingProtectionLevel, DevicePairingRequestedEventArgs, DevicePairingResultStatus,
    DeviceWatcher,
};
use windows::Devices::Radios::{Radio, RadioKind, RadioState as WinRadioState};
use windows::Foundation::{IReference, TypedEventHandler};
use windows_collections::IIterable;

use super::winrt::{self, BleError};
use super::{Command, Reply};
use crate::ams::{self, NowPlaying};
use crate::ancs::{self, Category, EventFlags, EventId, Response};
use crate::state::{
    events, keys, AdvertisingState, AppName, ConnectionState, DiscoveredDevice, PairedDevice, PairingRequest,
    RadioState, Services, Shared, Transport,
};
use crate::store::NewNotification;

/// tug's own advertised GATT service, so the iPhone has something to connect to.
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum Request {
    Notification(u32),
    App(String),
}

struct Ancs {
    control_point: GattCharacteristic,
    // Held so their ValueChanged registrations stay alive.
    _notification_source: GattCharacteristic,
    _data_source: GattCharacteristic,
    queue: VecDeque<Request>,
    inflight: Option<Instant>,
    reassembler: ancs::Reassembler,
    meta: HashMap<u32, (EventFlags, Category)>,
    rows: HashMap<u32, i64>,
    asked_apps: HashSet<String>,
}

struct Media {
    remote_command: GattCharacteristic,
    _entity_update: GattCharacteristic,
}

/// One opened iPhone. Survives disconnects; services are rebuilt on reconnect.
struct Link {
    gen: u64,
    device: BluetoothLEDevice,
    _gatt_session: Option<GattSession>,
    connected: bool,
    /// Identifies this ANCS subscription; iOS notification UIDs are only valid within it.
    session_id: Option<String>,
    ancs: Option<Ancs>,
    media: Option<Media>,
    _battery: Option<GattCharacteristic>,
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
            Command::UseDevice { id, reply } => {
                let _ = reply.send(self.use_device(&id).await);
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
                    self.start_advertising().await;
                } else {
                    self.stop_advertising();
                }
                let _ = reply.send(Ok(()));
            }
        }
    }

    async fn event(&mut self, ev: Event) {
        let current = self.link.as_ref().map(|l| l.gen);
        match ev {
            Event::NotificationSource { gen, data } if Some(gen) == current => self.on_notification_source(&data).await,
            Event::DataSource { gen, data } if Some(gen) == current => self.on_data_source(&data).await,
            Event::MediaEntity { gen, data } if Some(gen) == current => {
                self.shared.update_now_playing(|np| np.apply_entity_update(&data));
            }
            Event::MediaCommands { gen, data } if Some(gen) == current => {
                self.shared.update_now_playing(|np| np.apply_available_commands(&data));
            }
            Event::Battery { gen, data } if Some(gen) == current => {
                if let Some(&level) = data.first() {
                    self.shared.update_status(|s| s.battery = Some(level.min(100)));
                }
            }
            Event::Connection { gen, connected } if Some(gen) == current => self.on_connection(connected),
            Event::NotificationSource { .. }
            | Event::DataSource { .. }
            | Event::MediaEntity { .. }
            | Event::MediaCommands { .. }
            | Event::Battery { .. }
            | Event::Connection { .. } => {} // from a link we've since replaced
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

        if self.discovered_dirty {
            self.discovered_dirty = false;
            self.emit_discovered();
        }

        // A lost Data Source response would otherwise stall the queue forever.
        let stalled = self
            .link
            .as_ref()
            .and_then(|l| l.ancs.as_ref())
            .and_then(|a| a.inflight)
            .is_some_and(|t| t.elapsed() > ANCS_RESPONSE_TIMEOUT);
        if stalled {
            log::warn!("ANCS response timed out; skipping");
            if let Some(a) = self.link.as_mut().and_then(|l| l.ancs.as_mut()) {
                a.inflight = None;
                a.reassembler.reset();
            }
            self.pump().await;
        }

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

    // ---------------------------------------------------------------- radio / adapter

    async fn watch_radio(&mut self) -> windows::core::Result<()> {
        let radios = Radio::GetRadiosAsync()?.await?;
        let Some(radio) = radios.into_iter().find(|r| r.Kind().ok() == Some(RadioKind::Bluetooth)) else {
            self.shared.update_status(|s| s.radio = RadioState::Unavailable);
            return Ok(());
        };
        let map = |r: &Radio| match r.State() {
            Ok(WinRadioState::On) => RadioState::On,
            Ok(WinRadioState::Off) => RadioState::Off,
            Ok(WinRadioState::Disabled) => RadioState::Unavailable,
            _ => RadioState::Unknown,
        };
        let state = map(&radio);
        log::info!("Bluetooth radio: {state:?}");
        self.shared.update_status(|s| s.radio = state);
        let tx = self.tx.clone();
        radio.StateChanged(&TypedEventHandler::<Radio, windows::core::IInspectable>::new(
            move |r, _| {
                if let Some(r) = r.as_ref() {
                    let _ = tx.send(Event::Radio(map(r)));
                }
                Ok(())
            },
        ))?;
        self._radio = Some(radio);
        Ok(())
    }

    async fn peripheral_supported(&self) -> windows::core::Result<bool> {
        BluetoothAdapter::GetDefaultAsync()?.await?.IsPeripheralRoleSupported()
    }

    // ---------------------------------------------------------------- advertising

    async fn start_advertising(&mut self) {
        if self.provider.is_some() {
            return;
        }
        self.shared
            .update_status(|s| s.advertising = AdvertisingState::Starting);
        match self.create_provider().await {
            Ok(provider) => self.provider = Some(provider),
            Err(e) => {
                log::error!("advertising failed: {e}");
                self.shared.update_status(|s| {
                    s.advertising = AdvertisingState::Error;
                    s.last_error = Some(format!("Couldn't advertise to the iPhone: {e}"));
                });
            }
        }
    }

    async fn create_provider(&self) -> Result<GattServiceProvider, BleError> {
        let res = GattServiceProvider::CreateAsync(guid(TUG_SERVICE))?.await?;
        let err = res.Error()?;
        if err != BluetoothError::Success {
            return Err(BleError::Win(windows::core::Error::new(
                windows::core::HRESULT(-1),
                format!("GATT service provider unavailable ({err:?}); this adapter may not support peripheral mode"),
            )));
        }
        let provider = res.ServiceProvider()?;

        let params = GattLocalCharacteristicParameters::new()?;
        params.SetCharacteristicProperties(GattCharacteristicProperties::Read)?;
        params.SetReadProtectionLevel(GattProtectionLevel::Plain)?;
        params.SetStaticValue(&winrt::to_buffer(b"tug")?)?;
        params.SetUserDescription(&HSTRING::from("tug"))?;
        provider
            .Service()?
            .CreateCharacteristicAsync(guid(TUG_INFO), &params)?
            .await?;

        let tx = self.tx.clone();
        provider.AdvertisementStatusChanged(&TypedEventHandler::<
            GattServiceProvider,
            GattServiceProviderAdvertisementStatusChangedEventArgs,
        >::new(move |_, args| {
            if let Some(args) = args.as_ref() {
                if let Ok(status) = args.Status() {
                    let _ = tx.send(Event::Advertising(status));
                }
            }
            Ok(())
        }))?;

        let adv = GattServiceProviderAdvertisingParameters::new()?;
        adv.SetIsConnectable(true)?;
        adv.SetIsDiscoverable(true)?;
        provider.StartAdvertisingWithParameters(&adv)?;
        Ok(provider)
    }

    fn stop_advertising(&mut self) {
        self.advertise_retry_in = None;
        if let Some(p) = self.provider.take() {
            let _ = p.StopAdvertising();
        }
        self.shared.update_status(|s| s.advertising = AdvertisingState::Off);
    }

    // ---------------------------------------------------------------- discovery & pairing

    fn start_discovery(&mut self) -> windows::core::Result<()> {
        if !self.watchers.is_empty() {
            self.emit_discovered();
            return Ok(());
        }
        self.discovered.clear();
        let le = format!("System.Devices.Aep.ProtocolId:=\"{AEP_PROTOCOL_LE}\"");
        let classic = format!(
            "System.Devices.Aep.ProtocolId:=\"{AEP_PROTOCOL_CLASSIC}\" AND System.Devices.Aep.IsPaired:=System.StructuredQueryType.Boolean#True"
        );
        for (aqs, transport) in [(le, Transport::Le), (classic, Transport::Classic)] {
            let props = IIterable::<HSTRING>::from(vec![
                HSTRING::from(PROP_IS_CONNECTED),
                HSTRING::from("System.Devices.Aep.IsPaired"),
            ]);
            let watcher = DeviceInformation::CreateWatcherWithKindAqsFilterAndAdditionalProperties(
                &HSTRING::from(aqs),
                &props,
                DeviceInformationKind::AssociationEndpoint,
            )?;
            let tx = self.tx.clone();
            watcher.Added(&TypedEventHandler::<DeviceWatcher, DeviceInformation>::new(
                move |_, info| {
                    if let Some(info) = info.as_ref() {
                        let _ = tx.send(Event::DeviceAdded(info.clone(), transport));
                    }
                    Ok(())
                },
            ))?;
            let tx = self.tx.clone();
            watcher.Updated(&TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(
                move |_, u| {
                    if let Some(u) = u.as_ref() {
                        let _ = tx.send(Event::DeviceUpdated(u.clone()));
                    }
                    Ok(())
                },
            ))?;
            let tx = self.tx.clone();
            watcher.Removed(&TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(
                move |_, u| {
                    if let Some(id) = u.as_ref().and_then(|u| u.Id().ok()) {
                        let _ = tx.send(Event::DeviceRemoved(id.to_string()));
                    }
                    Ok(())
                },
            ))?;
            watcher.Start()?;
            self.watchers.push(watcher);
        }
        Ok(())
    }

    fn stop_discovery(&mut self) {
        for w in self.watchers.drain(..) {
            let _ = w.Stop();
        }
        self.discovered.clear();
    }

    fn emit_discovered(&self) {
        let mut list: Vec<DiscoveredDevice> = self
            .discovered
            .iter()
            .filter_map(|(id, d)| {
                let connected = bool_property(&d.info, PROP_IS_CONNECTED);
                let name = d.info.Name().map(|n| n.to_string()).unwrap_or_default();
                // A just-connected iPhone can appear before Windows learns its name.
                let name = match (name.is_empty(), connected) {
                    (false, _) => name,
                    (true, true) => "Unnamed device".to_string(),
                    (true, false) => return None,
                };
                let pairing = d.info.Pairing().ok();
                Some(DiscoveredDevice {
                    id: id.clone(),
                    name,
                    transport: d.transport,
                    paired: pairing.as_ref().and_then(|p| p.IsPaired().ok()).unwrap_or(false),
                    can_pair: pairing.as_ref().and_then(|p| p.CanPair().ok()).unwrap_or(false),
                    connected,
                })
            })
            .collect();
        // Connected first: when the iPhone connects from LightBlue it jumps to the top.
        list.sort_by(|a, b| {
            b.connected
                .cmp(&a.connected)
                .then(b.paired.cmp(&a.paired))
                .then(a.name.cmp(&b.name))
        });
        self.shared.emit(events::DISCOVERED_DEVICES, list);
    }

    fn pair(&mut self, id: String, reply: Reply) {
        let Some(d) = self.discovered.get(&id) else {
            let _ = reply.send(Err("That device is no longer in range".into()));
            return;
        };
        let info = d.info.clone();
        log::info!(
            "pairing started: {}",
            info.Name().map(|n| n.to_string()).unwrap_or_default()
        );
        let shared = self.shared.clone();
        let tx = self.tx.clone();
        tokio::task::spawn_local(async move {
            let result = pair_device(&info, shared)
                .await
                .map_err(|e| e.message().to_string())
                .and_then(|status| match status {
                    DevicePairingResultStatus::Paired | DevicePairingResultStatus::AlreadyPaired => Ok(()),
                    DevicePairingResultStatus::RejectedByHandler | DevicePairingResultStatus::PairingCanceled => {
                        Err("Pairing was cancelled".to_string())
                    }
                    DevicePairingResultStatus::AuthenticationTimeout => Err("Pairing timed out".to_string()),
                    other => Err(format!("Pairing failed ({other:?})")),
                });
            let _ = tx.send(Event::PairingDone { id, result, reply });
        });
    }

    /// Adopt a device as "the iPhone": persist it and start connecting.
    async fn use_device(&mut self, id: &str) -> Result<(), String> {
        let le = resolve_le_device(id, self.discovered.get(id).map(|d| d.transport))
            .await
            .map_err(|e| format!("Couldn't open that device over Bluetooth LE: {}", e.message()))?;
        let le_id = le.DeviceId().map_err(|e| e.message().to_string())?.to_string();
        log::info!("using device {le_id}");
        let name = le.Name().map(|n| n.to_string()).unwrap_or_default();
        let name = if name.is_empty() { "iPhone".to_string() } else { name };
        let store = &self.shared.store;
        store.set_setting(keys::DEVICE_ID, &le_id).map_err(|e| e.to_string())?;
        store.set_setting(keys::DEVICE_NAME, &name).map_err(|e| e.to_string())?;
        self.drop_link();
        self.device_id = Some(le_id.clone());
        self.retry_in = 0;
        self.stop_discovery();
        self.shared.update_status(|s| {
            s.device = Some(PairedDevice { id: le_id, name });
            s.connection = ConnectionState::Disconnected;
            s.last_error = None;
        });
        Ok(())
    }

    async fn forget(&mut self) -> Result<(), String> {
        let id = self.device_id.take();
        self.drop_link();
        let store = &self.shared.store;
        let _ = store.delete_setting(keys::DEVICE_ID);
        let _ = store.delete_setting(keys::DEVICE_NAME);
        self.shared.update_status(|s| {
            s.device = None;
            s.connection = ConnectionState::NoDevice;
            s.last_error = None;
        });
        if let Some(id) = id {
            // Best effort: a stale Windows bond makes re-pairing fail silently.
            if let Ok(info) = async { DeviceInformation::CreateFromIdAsync(&HSTRING::from(id))?.await }.await {
                if let Ok(op) = info.Pairing().and_then(|p| p.UnpairAsync()) {
                    let _ = op.await;
                }
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------- link lifecycle

    fn drop_link(&mut self) {
        if let Some(link) = self.link.take() {
            let _ = link.device.Close();
        }
        self.shared.set_live_session(None);
        self.shared.update_status(|s| {
            s.battery = None;
            s.services = Services::default();
        });
        self.shared.update_now_playing(|np| {
            let changed = *np != NowPlaying::default();
            *np = NowPlaying::default();
            changed
        });
    }

    async fn connect(&mut self) {
        let Some(id) = self.device_id.clone() else { return };
        log::debug!("connecting to {id}");
        self.shared
            .update_status(|s| s.connection = ConnectionState::Connecting);
        if self.link.is_none() {
            match self.open_link(&id).await {
                Ok(link) => self.link = Some(link),
                Err(e) => {
                    self.fail_connect(e.to_string());
                    return;
                }
            }
        }
        match self.setup_services().await {
            Ok(()) => {
                if let Some(l) = self.link.as_mut() {
                    l.connected = true;
                }
                self.shared.update_status(|s| {
                    s.connection = ConnectionState::Connected;
                    s.last_error = None;
                });
            }
            Err(e) => self.fail_connect(e.to_string()),
        }
    }

    fn fail_connect(&mut self, message: String) {
        log::info!("connect attempt failed: {message}");
        let linked = self.link.as_ref().is_some_and(|l| l.connected);
        self.retry_in = if linked { RETRY_CONNECTED_SECS } else { RETRY_IDLE_SECS };
        self.shared.update_status(|s| {
            s.connection = ConnectionState::Disconnected;
            s.last_error = Some(message);
        });
    }

    async fn open_link(&mut self, id: &str) -> Result<Link, BleError> {
        let device = BluetoothLEDevice::FromIdAsync(&HSTRING::from(id))?.await?;
        self.gen += 1;
        let gen = self.gen;
        let tx = self.tx.clone();
        device.ConnectionStatusChanged(
            &TypedEventHandler::<BluetoothLEDevice, windows::core::IInspectable>::new(move |d, _| {
                if let Some(d) = d.as_ref() {
                    let connected = d.ConnectionStatus().ok() == Some(BluetoothConnectionStatus::Connected);
                    let _ = tx.send(Event::Connection { gen, connected });
                }
                Ok(())
            }),
        )?;
        // Ask Windows to keep the link up and re-establish it when the phone returns.
        let gatt_session = match async { GattSession::FromDeviceIdAsync(&device.BluetoothDeviceId()?)?.await }.await {
            Ok(s) => {
                let _ = s.SetMaintainConnection(true);
                Some(s)
            }
            Err(e) => {
                log::warn!("GattSession unavailable: {e}");
                None
            }
        };
        let connected = device.ConnectionStatus()? == BluetoothConnectionStatus::Connected;
        if let Ok(name) = device.Name() {
            let name = name.to_string();
            if !name.is_empty() {
                let _ = self.shared.store.set_setting(keys::DEVICE_NAME, &name);
                self.shared.update_status(|s| {
                    if let Some(d) = s.device.as_mut() {
                        d.name = name;
                    }
                });
            }
        }
        Ok(Link {
            gen,
            device,
            _gatt_session: gatt_session,
            connected,
            session_id: None,
            ancs: None,
            media: None,
            _battery: None,
        })
    }

    fn on_connection(&mut self, connected: bool) {
        log::info!("iPhone link {}", if connected { "up" } else { "down" });
        let Some(link) = self.link.as_mut() else { return };
        link.connected = connected;
        if connected {
            if link.ancs.is_none() {
                self.retry_in = 0;
            }
            return;
        }
        // Services and notification UIDs don't survive a disconnect.
        link.ancs = None;
        link.media = None;
        link._battery = None;
        link.session_id = None;
        self.shared.set_live_session(None);
        self.retry_in = RETRY_CONNECTED_SECS;
        self.shared.update_status(|s| {
            s.connection = ConnectionState::Disconnected;
            s.battery = None;
            s.services = Services::default();
        });
        self.shared.update_now_playing(|np| {
            let changed = *np != NowPlaying::default();
            *np = NowPlaying::default();
            changed
        });
    }

    async fn setup_services(&mut self) -> Result<(), BleError> {
        let (device, gen) = match self.link.as_ref() {
            Some(l) => (l.device.clone(), l.gen),
            None => return Err(BleError::Unreachable),
        };

        // ANCS is required; media and battery are optional extras.
        let ancs = self.setup_ancs(&device, gen).await?;
        let media = match self.setup_media(&device, gen).await {
            Ok(m) => Some(m),
            Err(e) => {
                log::info!("AMS unavailable: {e}");
                None
            }
        };
        let battery = match self.setup_battery(&device, gen).await {
            Ok(b) => Some(b),
            Err(e) => {
                log::info!("Battery Service unavailable: {e}");
                None
            }
        };
        log::info!(
            "iPhone services ready: notifications, media={}, battery={}",
            media.is_some(),
            battery.is_some()
        );
        self.shared.update_status(|s| {
            s.services = Services {
                notifications: true,
                media: media.is_some(),
                battery: battery.is_some(),
            }
        });
        if let Some(link) = self.link.as_mut() {
            link.ancs = Some(ancs);
            link.media = media;
            link._battery = battery;
        }
        Ok(())
    }

    async fn setup_ancs(&mut self, device: &BluetoothLEDevice, gen: u64) -> Result<Ancs, BleError> {
        let svc = winrt::service(device, guid(ancs::SERVICE))
            .await?
            .ok_or(BleError::NotFound("Notification service (ANCS)"))?;
        let control_point = winrt::characteristic(&svc, guid(ancs::CONTROL_POINT), "ANCS control point").await?;
        let ns = winrt::characteristic(&svc, guid(ancs::NOTIFICATION_SOURCE), "ANCS notification source").await?;
        let ds = winrt::characteristic(&svc, guid(ancs::DATA_SOURCE), "ANCS data source").await?;

        // New session id before subscribing: iOS immediately replays every
        // notification still on the phone, and those must map to this session.
        let session_id = format!("{}-{gen}", now_ms());
        self.shared.set_live_session(Some(session_id.clone()));
        if let Some(link) = self.link.as_mut() {
            link.session_id = Some(session_id);
        }

        // Data Source first, so no attribute response can arrive unheard.
        let tx = self.tx.clone();
        winrt::subscribe(&ds, move |data| {
            let _ = tx.send(Event::DataSource { gen, data });
        })
        .await?;
        let tx = self.tx.clone();
        winrt::subscribe(&ns, move |data| {
            let _ = tx.send(Event::NotificationSource { gen, data });
        })
        .await?;
        Ok(Ancs {
            control_point,
            _notification_source: ns,
            _data_source: ds,
            queue: VecDeque::new(),
            inflight: None,
            reassembler: ancs::Reassembler::default(),
            meta: HashMap::new(),
            rows: HashMap::new(),
            asked_apps: HashSet::new(),
        })
    }

    async fn setup_media(&self, device: &BluetoothLEDevice, gen: u64) -> Result<Media, BleError> {
        let svc = winrt::service(device, guid(ams::SERVICE))
            .await?
            .ok_or(BleError::NotFound("Media service (AMS)"))?;
        let remote = winrt::characteristic(&svc, guid(ams::REMOTE_COMMAND), "AMS remote command").await?;
        let entity = winrt::characteristic(&svc, guid(ams::ENTITY_UPDATE), "AMS entity update").await?;
        let tx = self.tx.clone();
        winrt::subscribe(&remote, move |data| {
            let _ = tx.send(Event::MediaCommands { gen, data });
        })
        .await?;
        let tx = self.tx.clone();
        winrt::subscribe(&entity, move |data| {
            let _ = tx.send(Event::MediaEntity { gen, data });
        })
        .await?;
        for registration in ams::registrations() {
            winrt::write(&entity, &registration).await?;
        }
        Ok(Media {
            remote_command: remote,
            _entity_update: entity,
        })
    }

    async fn setup_battery(&self, device: &BluetoothLEDevice, gen: u64) -> Result<GattCharacteristic, BleError> {
        let svc = winrt::service(device, winrt::sig_uuid(BATTERY_SERVICE))
            .await?
            .ok_or(BleError::NotFound("Battery service"))?;
        let level = winrt::characteristic(&svc, winrt::sig_uuid(BATTERY_LEVEL), "battery level").await?;
        let initial = winrt::read(&level).await?;
        let _ = self.tx.send(Event::Battery { gen, data: initial });
        let tx = self.tx.clone();
        if let Err(e) = winrt::subscribe(&level, move |data| {
            let _ = tx.send(Event::Battery { gen, data });
        })
        .await
        {
            log::info!("battery notifications unavailable, showing last read value: {e}");
        }
        Ok(level)
    }

    // ---------------------------------------------------------------- ANCS

    async fn on_notification_source(&mut self, data: &[u8]) {
        let ev = match ancs::parse_notification_source(data) {
            Ok(ev) => ev,
            Err(e) => return log::warn!("bad ANCS notification source packet: {e}"),
        };
        let Some(link) = self.link.as_mut() else { return };
        let (Some(a), Some(session)) = (link.ancs.as_mut(), link.session_id.as_deref()) else {
            return;
        };
        match ev.event {
            EventId::Added | EventId::Modified => {
                a.meta.insert(ev.uid, (ev.flags, ev.category));
                let req = Request::Notification(ev.uid);
                if !a.queue.contains(&req) {
                    a.queue.push_back(req);
                }
            }
            EventId::Removed => {
                a.meta.remove(&ev.uid);
                a.rows.remove(&ev.uid);
                a.queue.retain(|r| *r != Request::Notification(ev.uid));
                match self.shared.store.mark_removed(session, ev.uid, now_ms()) {
                    Ok(Some(id)) => self.shared.emit(events::NOTIFICATION_REMOVED, id),
                    Ok(None) => {}
                    Err(e) => log::error!("mark_removed: {e}"),
                }
            }
        }
        self.pump().await;
    }

    async fn on_data_source(&mut self, data: &[u8]) {
        let Some(a) = self.link.as_mut().and_then(|l| l.ancs.as_mut()) else {
            return;
        };
        match a.reassembler.push(data) {
            Ok(None) => return,
            Ok(Some(resp)) => {
                a.inflight = None;
                self.on_response(resp);
            }
            Err(e) => {
                log::warn!("ANCS data source: {e}");
                a.inflight = None;
            }
        }
        self.pump().await;
    }

    fn on_response(&mut self, resp: Response) {
        let Some(link) = self.link.as_mut() else { return };
        let (Some(a), Some(session)) = (link.ancs.as_mut(), link.session_id.as_deref()) else {
            return;
        };
        match resp {
            Response::Notification { uid, attrs } => {
                let (flags, category) = a.meta.get(&uid).copied().unwrap_or_default();
                let stored = self.shared.store.upsert_notification(&NewNotification {
                    session,
                    uid,
                    category,
                    flags,
                    attrs: &attrs,
                    received_at: now_ms(),
                });
                match stored {
                    Ok(n) => {
                        a.rows.insert(uid, n.id);
                        if n.app_name.is_none() && !attrs.app_id.is_empty() && a.asked_apps.insert(attrs.app_id.clone())
                        {
                            a.queue.push_back(Request::App(attrs.app_id.clone()));
                        }
                        self.shared.emit(events::NOTIFICATION, n);
                    }
                    Err(e) => log::error!("store notification: {e}"),
                }
            }
            Response::App {
                app_id,
                display_name: Some(name),
            } => {
                if let Err(e) = self.shared.store.set_app_name(&app_id, &name) {
                    log::error!("store app name: {e}");
                }
                self.shared.emit(events::APP_NAME, AppName { app_id, app_name: name });
            }
            Response::App { .. } => {}
        }
    }

    /// Send the next queued Control Point request if none is in flight.
    async fn pump(&mut self) {
        loop {
            let Some(a) = self.link.as_mut().and_then(|l| l.ancs.as_mut()) else {
                return;
            };
            if a.inflight.is_some() {
                return;
            }
            let Some(req) = a.queue.pop_front() else { return };
            let bytes = match &req {
                Request::Notification(uid) => {
                    a.reassembler.expect_notification(*uid);
                    ancs::get_notification_attributes(*uid)
                }
                Request::App(app_id) => {
                    a.reassembler.expect_app(app_id);
                    ancs::get_app_attributes(app_id)
                }
            };
            a.inflight = Some(Instant::now());
            let cp = a.control_point.clone();
            match winrt::write(&cp, &bytes).await {
                Ok(()) => return,
                Err(e) => {
                    // 0xA2 means the notification vanished before we asked; just move on.
                    if !matches!(e, BleError::Protocol(Some(ancs::ERR_INVALID_PARAMETER))) {
                        log::warn!("ANCS request {req:?} failed: {e}");
                    }
                    if let Some(a) = self.link.as_mut().and_then(|l| l.ancs.as_mut()) {
                        a.inflight = None;
                        a.reassembler.reset();
                    }
                }
            }
        }
    }

    async fn perform_action(&mut self, id: i64, positive: bool) -> Result<(), String> {
        let a = self
            .link
            .as_ref()
            .and_then(|l| l.ancs.as_ref())
            .ok_or_else(|| "iPhone isn't connected".to_string())?;
        let uid = a
            .rows
            .iter()
            .find_map(|(uid, row)| (*row == id).then_some(*uid))
            .ok_or_else(|| "That notification is no longer on the iPhone".to_string())?;
        let (flags, _) = a.meta.get(&uid).copied().unwrap_or_default();
        if (positive && !flags.positive_action) || (!positive && !flags.negative_action) {
            return Err("The iPhone doesn't offer that action for this notification".into());
        }
        let cp = a.control_point.clone();
        winrt::write(&cp, &ancs::perform_action(uid, positive))
            .await
            .map_err(|e| match e {
                BleError::Protocol(Some(ancs::ERR_ACTION_FAILED)) => {
                    "The iPhone couldn't perform that action".to_string()
                }
                BleError::Protocol(Some(ancs::ERR_UNKNOWN_COMMAND | ancs::ERR_INVALID_COMMAND)) => {
                    "This iOS version doesn't support notification actions".to_string()
                }
                other => other.to_string(),
            })
    }

    fn set_error(&self, message: String) {
        self.shared.update_status(|s| s.last_error = Some(message));
    }
}

/// Pair with a PIN prompt routed through the tug UI.
async fn pair_device(
    info: &DeviceInformation,
    shared: Arc<Shared>,
) -> windows::core::Result<DevicePairingResultStatus> {
    let pairing = info.Pairing()?;
    if pairing.IsPaired()? {
        return Ok(DevicePairingResultStatus::AlreadyPaired);
    }
    let device_name = info.Name()?.to_string();
    let custom = pairing.Custom()?;
    custom.PairingRequested(&TypedEventHandler::<
        DeviceInformationCustomPairing,
        DevicePairingRequestedEventArgs,
    >::new(move |_, args| {
        let Some(args) = args.as_ref() else { return Ok(()) };
        log::info!("pairing requested by Windows: kind {:?}", args.PairingKind()?);
        if args.PairingKind()? == DevicePairingKinds::ConfirmOnly {
            return args.Accept();
        }
        // ConfirmPinMatch / DisplayPin: show the code and let the user decide.
        let pin = args.Pin().ok().map(|p| p.to_string()).filter(|p| !p.is_empty());
        let deferral = args.GetDeferral()?;
        let (tx, rx) = sync_channel(1);
        shared.set_pairing_confirm(Some(tx));
        shared.emit(
            events::PAIRING_REQUEST,
            PairingRequest {
                device_name: device_name.clone(),
                pin,
            },
        );
        let args = args.clone();
        let shared = shared.clone();
        std::thread::spawn(move || {
            if rx.recv_timeout(PIN_CONFIRM_TIMEOUT).unwrap_or(false) {
                let _ = args.Accept();
            }
            let _ = deferral.Complete();
            shared.set_pairing_confirm(None);
            shared.emit(events::PAIRING_REQUEST_CLOSED, ());
        });
        Ok(())
    }))?;
    let kinds = DevicePairingKinds::ConfirmOnly | DevicePairingKinds::ConfirmPinMatch | DevicePairingKinds::DisplayPin;
    let result = custom
        .PairWithProtectionLevelAsync(kinds, DevicePairingProtectionLevel::Encryption)?
        .await?;
    result.Status()
}

/// Turn a watcher id into a `BluetoothLEDevice`. A phone paired through
/// Windows Settings appears as a Classic device; its LE side shares the address.
async fn resolve_le_device(id: &str, transport: Option<Transport>) -> windows::core::Result<BluetoothLEDevice> {
    let hid = HSTRING::from(id);
    if transport != Some(Transport::Classic) {
        if let Ok(le) = BluetoothLEDevice::FromIdAsync(&hid)?.await {
            return Ok(le);
        }
    }
    let classic = BluetoothDevice::FromIdAsync(&hid)?.await?;
    BluetoothLEDevice::FromBluetoothAddressAsync(classic.BluetoothAddress()?)?.await
}

fn bool_property(info: &DeviceInformation, key: &str) -> bool {
    info.Properties()
        .and_then(|p| p.Lookup(&HSTRING::from(key)))
        .and_then(|v| v.cast::<IReference<bool>>())
        .and_then(|r| r.Value())
        .unwrap_or(false)
}
