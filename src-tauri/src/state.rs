//! State shared between the Bluetooth actor, Tauri commands and the UI.
//! Every type here is mirrored in `src/types/protocol.ts`.

use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::ams::NowPlaying;
use crate::map::calls::CallRecord;
use crate::map::health::{LiveTexts, TextsPairing};
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
    /// Tray click/Open with unread texts: the frontend opens the newest unread conversation.
    pub const OPEN_LATEST_CONVERSATION: &str = "open-latest-conversation";
    pub const CALLS: &str = "calls";
    /// A pop-up's body or button was pressed and carried out (see `toast`).
    pub const TOAST_PRESSED: &str = "toast-pressed";
}

/// Settings keys stored in SQLite.
pub mod keys {
    pub const DEVICE_ID: &str = "device_id";
    pub const DEVICE_NAME: &str = "device_name";
    /// The phone's model identifier ("iPhone16,2") from its Device Information Service, kept so
    /// the sidebar can picture the phone while it's away. Cleared with the device.
    pub const DEVICE_MODEL: &str = "device_model";
    pub const ADVERTISE: &str = "advertise";
    /// The Classic (texts) device id tug last connected a MAP session to. Remembered so a
    /// phone rename can't make tug follow the old name onto the wrong device.
    pub const TEXTS_DEVICE_ID: &str = "texts_device_id";
    /// Unix ms of the last successful contact-photo pull (the slow WITH-PHOTO PBAP pass). Persisted
    /// so the photo pass runs at most once a day instead of on every connect and 15-min resync.
    pub const LAST_PHOTO_SYNC: &str = "last_photo_sync";
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
    /// Apple's model identifier ("iPhone16,2"), read over Bluetooth (Device Information Service).
    /// None until the phone has told us, or if it never does. The UI maps it to a name and picture.
    pub model: Option<String>,
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
    pub services: Services,
    pub last_error: Option<String>,
    /// Unix ms when `last_error` last changed to its current value, so the UI can say "2m ago".
    pub last_error_at: Option<i64>,
    /// The iPhone rejects this PC's notifications bond (forgotten on the phone): pair again.
    pub pairing_stale: bool,
    /// On a fresh bond iOS holds the ANCS subscribe (CCCD write) open until the user taps
    /// "Allow" on the phone. True while that write is in flight, so setup can say to look
    /// at the iPhone instead of looking stuck.
    pub awaiting_phone_allow: bool,
    /// The iPhone is connected but isn't offering notifications (ANCS) — it's locked after a
    /// restart, or mid-update before its first unlock. The UI says to unlock it to reconnect.
    pub awaiting_unlock: bool,
    /// tug is rebuilding the link on its own (Bluetooth stopped responding, the PC woke, Windows
    /// closed tug's Bluetooth objects): the phone doesn't need the user, so the UI says
    /// "Reconnecting…" rather than "Waiting for iPhone". Cleared once connected, or after a few
    /// failed attempts.
    pub reconnecting: bool,
    /// The iPhone is away (out of range, or Windows can't reach it). Sticky until it connects
    /// again, so the status stays steady instead of flipping with every background retry.
    pub away: bool,
    /// Why message access isn't available, when the user can fix it (e.g. consent).
    pub messages_error: Option<String>,
    /// Why the phone's contacts aren't available, when the user can fix it.
    pub contacts_error: Option<String>,
    /// Whether the phone shared contacts on the current connection (Sync Contacts on). Not
    /// "tug has contacts saved": kept history made the setup checklist say on while it was off.
    pub contacts_shared: bool,
    /// Whether the texts (Classic) pairing works, is missing, or needs making again.
    pub texts_pairing: TextsPairing,
    /// The phone Windows has paired for texts (what to remove when it needs re-pairing).
    pub texts_device: Option<String>,
    /// Whether live texts (MAP notifications) are off, starting, active, or fell back to polling.
    pub live_texts: LiveTexts,
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
    pub kind: DeviceKind,
}

/// What a discovered device says it is, so setup never offers a keyboard as the iPhone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceKind {
    Phone,
    /// Keyboards, mice, headphones, watches: never the phone.
    Accessory,
    /// Nothing known yet (a just-connected iPhone is often nameless).
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingRequest {
    pub device_name: String,
    pub pin: Option<String>,
    /// ConfirmOnly pairing: Windows accepts on its own and the code (if any) is confirmed on the
    /// iPhone, so tug shows "Tap Pair on your iPhone" and offers no buttons of its own.
    pub confirm_on_phone: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppName {
    pub app_id: String,
    pub app_name: String,
}

pub struct Shared {
    pub app: AppHandle,
    pub store: Arc<Store>,
    status: Mutex<DeviceStatus>,
    now_playing: Mutex<NowPlaying>,
    /// The phone's recent calls (PBAP), newest first. Not stored: the phone keeps the real list.
    calls: Mutex<Vec<CallRecord>>,
    /// Current ANCS subscription id; rows from it can still take actions.
    live_session: Mutex<Option<String>>,
    /// Answer channel for a PIN-confirmation prompt that is waiting on the user.
    pub pairing_confirm: Mutex<Option<SyncSender<bool>>>,
    /// Message service, set once at startup.
    pub map: OnceLock<MapHandle>,
    /// The user is looking at the iPhone's switches (setup's sharing step, Settings): check
    /// them every couple of seconds so flipping one on the phone shows up right away.
    /// Until when fast checks run. Expires on its own unless the UI keeps renewing it, so a
    /// missed "stop" (window hidden mid-screen) can't leave the phone polled every 2 s forever.
    watching_until: Mutex<Option<std::time::Instant>>,
    /// Called after every Now Playing change (Windows' media controls follow it).
    now_playing_hook: OnceLock<Box<dyn Fn() + Send + Sync>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Wall-clock time in Unix milliseconds (for stamping when an error happened).
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Shared {
    pub fn new(app: AppHandle, store: Arc<Store>) -> Self {
        Self {
            app,
            store,
            status: Mutex::default(),
            now_playing: Mutex::default(),
            calls: Mutex::default(),
            live_session: Mutex::default(),
            pairing_confirm: Mutex::default(),
            map: OnceLock::new(),
            watching_until: Mutex::new(None),
            now_playing_hook: OnceLock::new(),
        }
    }

    pub fn watching(&self) -> bool {
        lock(&self.watching_until).is_some_and(|until| std::time::Instant::now() < until)
    }

    /// Start (or renew, for WATCH_FOR) or stop fast checks. Returns whether this turned them
    /// on, so callers can check right away.
    pub fn set_watching(&self, on: bool) -> bool {
        const WATCH_FOR: std::time::Duration = std::time::Duration::from_secs(90);
        let was = self.watching();
        *lock(&self.watching_until) = on.then(|| std::time::Instant::now() + WATCH_FOR);
        on && !was
    }

    pub fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) {
        if let Err(e) = self.app.emit(event, payload) {
            log::warn!("emit {event} failed: {e}");
        }
    }

    /// The phone cleared notification `id`: tell the UI, and take back its pop-up.
    pub fn notification_removed(&self, id: i64) {
        self.emit(events::NOTIFICATION_REMOVED, id);
        crate::toast::withdraw(&self.app, id);
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
            // Stamp when the error text changes, so the UI can show how long ago it happened.
            if s.last_error != before.last_error {
                s.last_error_at = s.last_error.is_some().then(now_ms);
            }
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
            if let Some(hook) = self.now_playing_hook.get() {
                hook();
            }
        }
    }

    /// Set once at startup: run `hook` after every Now Playing change.
    pub fn set_now_playing_hook(&self, hook: Box<dyn Fn() + Send + Sync>) {
        if self.now_playing_hook.set(hook).is_err() {
            log::warn!("now-playing hook already set");
        }
    }

    pub fn calls(&self) -> Vec<CallRecord> {
        lock(&self.calls).clone()
    }

    /// Replace the recent calls and push them to the UI if they changed.
    pub fn set_calls(&self, calls: Vec<CallRecord>) {
        let changed = {
            let mut current = lock(&self.calls);
            let changed = *current != calls;
            *current = calls;
            changed.then(|| current.clone())
        };
        if let Some(calls) = changed {
            self.emit(events::CALLS, calls);
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
