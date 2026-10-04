//! State shared between the Bluetooth actor, Tauri commands and the UI.
//! Every type here is mirrored in `src/types/protocol.ts`.

use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::ams::NowPlaying;
use crate::map::service::MapHandle;
use crate::store::Store;

/// Event names emitted to the frontend.
pub mod events {
    pub const DEVICE_STATUS: &str = "device-status";
    pub const NOW_PLAYING: &str = "now-playing";
    pub const NOTIFICATION: &str = "notification";
    pub const NOTIFICATION_REMOVED: &str = "notification-removed";
    pub const APP_NAME: &str = "app-name";
    pub const DISCOVERED_DEVICES: &str = "discovered-devices";
    pub const PAIRING_REQUEST: &str = "pairing-request";
    pub const PAIRING_REQUEST_CLOSED: &str = "pairing-request-closed";
    pub const MESSAGE: &str = "message";
    pub const CONTACTS: &str = "contacts";
}

/// Settings keys stored in SQLite.
pub mod keys {
    pub const DEVICE_ID: &str = "device_id";
    pub const DEVICE_NAME: &str = "device_name";
    pub const ADVERTISE: &str = "advertise";
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RadioState {
    #[default]
    Unknown,
    On,
    Off,
    Unavailable,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AdvertisingState {
    #[default]
    Off,
    Starting,
    On,
    Error,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionState {
    /// No iPhone chosen yet.
    #[default]
    NoDevice,
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Services {
    pub notifications: bool,
    pub media: bool,
    pub battery: bool,
    /// Bluetooth MAP session (read inbox, send replies) is up.
    pub messages: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStatus {
    pub radio: RadioState,
    pub peripheral_supported: Option<bool>,
    pub advertising: AdvertisingState,
    pub device: Option<PairedDevice>,
    pub connection: ConnectionState,
    pub battery: Option<u8>,
    /// Inferred from the level rising (true) or falling (false); unknown until it moves.
    pub charging: Option<bool>,
    pub services: Services,
    pub last_error: Option<String>,
    /// Why message access isn't available, when the user can fix it (e.g. consent).
    pub messages_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Transport {
    Le,
    Classic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredDevice {
    pub id: String,
    pub name: String,
    pub transport: Transport,
    pub paired: bool,
    pub connected: bool,
    pub can_pair: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingRequest {
    pub device_name: String,
    pub pin: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppName {
    pub app_id: String,
    pub app_name: String,
}

/// Charging isn't in the iPhone's Battery Service (level only), so infer it from
/// the direction the level moves.
#[derive(Debug, Default, Clone, Copy)]
pub struct BatteryTrend {
    last: Option<u8>,
    charging: Option<bool>,
}

impl BatteryTrend {
    pub fn observe(&mut self, level: u8) -> Option<bool> {
        match self.last {
            Some(prev) if level > prev => self.charging = Some(true),
            Some(prev) if level < prev => self.charging = Some(false),
            _ => {}
        }
        self.last = Some(level);
        self.charging
    }
}

pub struct Shared {
    pub app: AppHandle,
    pub store: Arc<Store>,
    status: Mutex<DeviceStatus>,
    now_playing: Mutex<NowPlaying>,
    /// Current ANCS subscription id; rows from it can still take actions.
    live_session: Mutex<Option<String>>,
    /// Answer channel for a PIN-confirmation prompt that is waiting on the user.
    pub pairing_confirm: Mutex<Option<SyncSender<bool>>>,
    /// Message service, set once at startup.
    pub map: OnceLock<MapHandle>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Shared {
    pub fn new(app: AppHandle, store: Arc<Store>) -> Self {
        Self {
            app,
            store,
            status: Mutex::default(),
            now_playing: Mutex::default(),
            live_session: Mutex::default(),
            pairing_confirm: Mutex::default(),
            map: OnceLock::new(),
        }
    }

    pub fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) {
        if let Err(e) = self.app.emit(event, payload) {
            log::warn!("emit {event} failed: {e}");
        }
    }

    pub fn status(&self) -> DeviceStatus {
        lock(&self.status).clone()
    }

    /// Mutate the status and push it to the UI if anything changed.
    pub fn update_status(&self, f: impl FnOnce(&mut DeviceStatus)) {
        let snapshot = {
            let mut s = lock(&self.status);
            let before = s.clone();
            f(&mut s);
            (*s != before).then(|| s.clone())
        };
        if let Some(s) = snapshot {
            self.emit(events::DEVICE_STATUS, s);
        }
    }

    pub fn now_playing(&self) -> NowPlaying {
        lock(&self.now_playing).clone()
    }

    pub fn update_now_playing(&self, f: impl FnOnce(&mut NowPlaying) -> bool) {
        let snapshot = {
            let mut np = lock(&self.now_playing);
            f(&mut np).then(|| np.clone())
        };
        if let Some(np) = snapshot {
            self.emit(events::NOW_PLAYING, np);
        }
    }

    pub fn live_session(&self) -> Option<String> {
        lock(&self.live_session).clone()
    }

    pub fn set_live_session(&self, session: Option<String>) {
        *lock(&self.live_session) = session;
    }

    pub fn set_pairing_confirm(&self, tx: Option<SyncSender<bool>>) {
        *lock(&self.pairing_confirm) = tx;
    }

    pub fn take_pairing_confirm(&self) -> Option<SyncSender<bool>> {
        lock(&self.pairing_confirm).take()
    }
}

#[cfg(test)]
mod tests {
    use super::BatteryTrend;

    #[test]
    fn infers_charging_from_level_direction() {
        let mut t = BatteryTrend::default();
        assert_eq!(t.observe(50), None, "unknown until the level moves");
        assert_eq!(t.observe(50), None);
        assert_eq!(t.observe(51), Some(true));
        assert_eq!(t.observe(51), Some(true), "holds while level is flat");
        assert_eq!(t.observe(100), Some(true), "full on the charger stays charging");
        assert_eq!(t.observe(99), Some(false));
    }
}
