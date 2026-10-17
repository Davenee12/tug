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
    /// Tray menu's Settings…: the frontend opens Settings (the only mouse path there while
    /// the first-run Connect panel stands in for the Feed).
    pub const OPEN_SETTINGS: &str = "open-settings";
    pub const CALLS: &str = "calls";
    /// A pop-up's body or button was pressed and carried out (see `toast`).
    pub const TOAST_PRESSED: &str = "toast-pressed";
    /// The type-the-code shortcut was pressed: what it did (see `code_fill`), never the code.
    pub const CODE_FILLED: &str = "code-filled";
    /// A taskbar jump-list task was clicked: the frontend takes what's waiting with
    /// `take_quick_actions` (see `quick_actions`). No payload, so a lost nudge loses nothing.
    pub const QUICK_ACTION: &str = "quick-action";
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
    /// Unix ms of the last successful text sync, so a catch-up after time away can tell it may
    /// have missed texts (see `map::service`), and the very first sync doesn't claim a gap.
    pub const LAST_TEXT_SYNC: &str = "last_text_sync";
    /// The phone (its device id) "Turn on automatically when my iPhone connects" is on for, if
    /// any (see `pc_audio`). Off unless the owner switches it on; cleared on Forget.
    pub const PC_AUDIO_AUTO_DEVICE: &str = "pc_audio_auto_device";
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
    /// Counts the changes: every status that differs from the one before gets the next number,
    /// given under the lock (`update_locked`). The Bluetooth actor, the MAP worker and commands all
    /// update the status, so their events (and a `get_status` read) can reach the UI out of order;
    /// the UI keeps whichever has the highest `rev`, so an older status never replaces a newer one.
    pub rev: u64,
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
    /// The paired phone offers no notifications (ANCS) and doesn't look like an Apple device at
    /// all (an Android phone, say), so "unlock your iPhone" would be wrong forever. Set by the
    /// ANCS setup that finds no ANCS, cleared by one that finds it.
    pub not_iphone: bool,
    /// With `awaiting_unlock`: the phone was working this run and its notifications went away on
    /// a live link, so it restarted ("Your iPhone restarted. Unlock it and tug reconnects by
    /// itself."). Cleared with `awaiting_unlock`.
    pub phone_restarted: bool,
    /// Message access has worked with this phone since it was set up (or tug started), so a drop
    /// is a reconnect ("Reconnecting to your texts…"), not a setup problem. Cleared on Forget and
    /// when another phone is adopted.
    pub texts_were_connected: bool,
    /// Message access has failed several times in a row: the UI says it can't reach texts right
    /// now instead of "Reconnecting…". Cleared by the next working sync.
    pub texts_unreachable: bool,
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
    /// The phone keeps answering with an empty phonebook (or refused): Sync Contacts is off. A
    /// definite answer, so the UI says "off" instead of "Checking…" while tug keeps asking every
    /// ~30 s (`map::contacts_watch`). Never true together with `contacts_shared`.
    pub contacts_off: bool,
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

/// A value sent to the UI with a count of its changes (`rev`), like `DeviceStatus::rev`. Flattened,
/// so the UI sees the value's own fields plus `rev`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Stamped<T> {
    pub rev: u64,
    #[serde(flatten)]
    pub value: T,
}

/// Apply `f` to the status behind `m`. When anything changed, the status gets the next `rev` and
/// `emit` runs with it *while the lock is still held*, so two threads updating at once emit in the
/// order their changes were made: the newest status is always the last one sent. Returns the new
/// status when it changed.
fn update_locked(
    m: &Mutex<DeviceStatus>,
    now: i64,
    f: impl FnOnce(&mut DeviceStatus),
    emit: impl FnOnce(&DeviceStatus),
) -> Option<DeviceStatus> {
    let mut s = lock(m);
    let before = s.clone();
    f(&mut s);
    // Only this function counts revisions.
    s.rev = before.rev;
    // Stamp when the error text changes, so the UI can show how long ago it happened.
    if s.last_error != before.last_error {
        s.last_error_at = s.last_error.is_some().then_some(now);
    }
    if *s == before {
        return None;
    }
    s.rev = before.rev + 1;
    emit(&s);
    Some(s.clone())
}

/// Like `update_locked`, for a `Stamped` value whose `f` reports whether it changed it.
fn update_stamped<T: Clone>(
    m: &Mutex<Stamped<T>>,
    f: impl FnOnce(&mut T) -> bool,
    emit: impl FnOnce(&Stamped<T>),
) -> Option<Stamped<T>> {
    let mut v = lock(m);
    if !f(&mut v.value) {
        return None;
    }
    v.rev += 1;
    emit(&v);
    Some(v.clone())
}

/// Numbers every copy of a state that is composed fresh each time it's sent (Tugboat's and
/// Developer tools' status): each copy, whether emitted or returned to a command, is composed and
/// numbered under one lock, so a higher `rev` always means a later, newer picture. Emitting under
/// the same lock keeps events in that order; the UI keeps the highest `rev` it has seen.
#[derive(Default)]
pub struct RevCounter(Mutex<u64>);

impl RevCounter {
    /// Compose the next copy (`compose` gets its `rev`) and hand it to `emit`, all under the lock.
    pub fn stamp<T>(&self, compose: impl FnOnce(u64) -> T, emit: impl FnOnce(&T)) -> T {
        let mut rev = lock(&self.0);
        *rev += 1;
        let value = compose(*rev);
        emit(&value);
        value
    }
}

/// Whether a status with revision `rev` is newer than the newest one seen (`seen`), recording it if
/// so. For code that follows the status from `Shared`'s status hook, which runs outside the lock and
/// so can see two updates in the wrong order.
pub fn take_newer(seen: &mut u64, rev: u64) -> bool {
    if rev > *seen {
        *seen = rev;
        true
    } else {
        false
    }
}

pub struct Shared {
    pub app: AppHandle,
    pub store: Arc<Store>,
    status: Mutex<DeviceStatus>,
    now_playing: Mutex<Stamped<NowPlaying>>,
    /// The phone's recent calls (PBAP), newest first. The last list is saved (`map::calls::save`)
    /// so Recents shows it at launch until the phone answers again.
    calls: Mutex<Vec<CallRecord>>,
    /// Current ANCS subscription id; rows from it can still take actions.
    live_session: Mutex<Option<String>>,
    /// The PIN-confirmation prompt waiting on the user, with its answer channel (`pin_confirm`).
    pairing_confirm: Mutex<crate::pin_confirm::PinConfirm<SyncSender<bool>>>,
    /// Message service, set once at startup.
    pub map: OnceLock<MapHandle>,
    /// The user is looking at the iPhone's switches (setup's sharing step, Settings): check
    /// them every couple of seconds so flipping one on the phone shows up right away.
    /// Until when fast checks run. Expires on its own unless the UI keeps renewing it, so a
    /// missed "stop" (window hidden mid-screen) can't leave the phone polled every 2 s forever.
    watching_until: Mutex<Option<std::time::Instant>>,
    /// Called after every Now Playing change (Windows' media controls follow it).
    now_playing_hook: OnceLock<Box<dyn Fn() + Send + Sync>>,
    /// Called after every status change, with the new status (PC audio follows the connection).
    status_hook: OnceLock<StatusHook>,
}

type StatusHook = Box<dyn Fn(&DeviceStatus) + Send + Sync>;

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
        let calls = crate::map::calls::load(&store);
        Self {
            app,
            store,
            status: Mutex::default(),
            now_playing: Mutex::default(),
            calls: Mutex::new(calls),
            live_session: Mutex::default(),
            pairing_confirm: Mutex::default(),
            map: OnceLock::new(),
            watching_until: Mutex::new(None),
            now_playing_hook: OnceLock::new(),
            status_hook: OnceLock::new(),
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

    /// Mutate the status and push it to the UI if anything changed. The event is sent under the
    /// lock with the next `rev` (see `update_locked`), so updates from different threads can't
    /// reach the window in the wrong order.
    pub fn update_status(&self, f: impl FnOnce(&mut DeviceStatus)) {
        let changed = update_locked(&self.status, now_ms(), f, |s| self.emit(events::DEVICE_STATUS, s));
        if let Some(s) = changed {
            // After the lock is released: the hook may read the status itself. Two hooks can run
            // in the wrong order, so the hook checks `rev` (`take_newer`).
            if let Some(hook) = self.status_hook.get() {
                hook(&s);
            }
        }
    }

    /// Set once at startup: run `hook` after every status change.
    pub fn set_status_hook(&self, hook: StatusHook) {
        if self.status_hook.set(hook).is_err() {
            log::warn!("status hook already set");
        }
    }

    pub fn now_playing(&self) -> NowPlaying {
        lock(&self.now_playing).value.clone()
    }

    /// Now Playing with its `rev`, for the UI's startup read (an event that overtook the read wins).
    pub fn now_playing_stamped(&self) -> Stamped<NowPlaying> {
        lock(&self.now_playing).clone()
    }

    /// Mutate Now Playing (`f` says whether it changed it) and push it to the UI under the lock,
    /// with the next `rev`, like `update_status`.
    pub fn update_now_playing(&self, f: impl FnOnce(&mut NowPlaying) -> bool) {
        let changed = update_stamped(&self.now_playing, f, |np| self.emit(events::NOW_PLAYING, np));
        if changed.is_some() {
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
            crate::map::calls::save(&self.store, &calls);
            self.emit(events::CALLS, calls);
        }
    }

    pub fn live_session(&self) -> Option<String> {
        lock(&self.live_session).clone()
    }

    pub fn set_live_session(&self, session: Option<String>) {
        *lock(&self.live_session) = session;
    }

    /// A PIN prompt is showing: returns its id, for `close_pairing_confirm`.
    pub fn open_pairing_confirm(&self, tx: SyncSender<bool>) -> u64 {
        lock(&self.pairing_confirm).open(tx)
    }

    /// An informational pairing prompt is showing (nothing to answer): returns its id, for
    /// `close_pairing_confirm`.
    pub fn open_pairing_info(&self) -> u64 {
        lock(&self.pairing_confirm).open_info()
    }

    pub fn take_pairing_confirm(&self) -> Option<SyncSender<bool>> {
        lock(&self.pairing_confirm).take()
    }

    /// Close PIN prompt `id` if it's still the one showing; true if it was (only then should the
    /// dialog be told to close).
    pub fn close_pairing_confirm(&self, id: u64) -> bool {
        lock(&self.pairing_confirm).close(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emitted(m: &Mutex<DeviceStatus>, f: impl FnOnce(&mut DeviceStatus)) -> Option<u64> {
        let mut sent = None;
        update_locked(m, 1_000, f, |s| sent = Some(s.rev));
        sent
    }

    #[test]
    fn rev_counts_changes_only() {
        let m = Mutex::new(DeviceStatus::default());
        assert_eq!(emitted(&m, |s| s.battery = Some(80)), Some(1));
        // The same value again is no change: nothing sent, no new revision.
        assert_eq!(emitted(&m, |s| s.battery = Some(80)), None);
        assert_eq!(emitted(&m, |s| s.connection = ConnectionState::Connected), Some(2));
        assert_eq!(lock(&m).rev, 2);
    }

    #[test]
    fn callers_cannot_set_the_rev() {
        let m = Mutex::new(DeviceStatus::default());
        // Touching only the rev is no change, and can't move the count.
        assert_eq!(emitted(&m, |s| s.rev = 99), None);
        let both = |s: &mut DeviceStatus| {
            s.rev = 99;
            s.battery = Some(5);
        };
        assert_eq!(emitted(&m, both), Some(1));
    }

    #[test]
    fn error_time_follows_the_error_text() {
        let m = Mutex::new(DeviceStatus::default());
        update_locked(&m, 1_000, |s| s.last_error = Some("x".into()), |_| ());
        assert_eq!(lock(&m).last_error_at, Some(1_000));
        update_locked(&m, 2_000, |s| s.battery = Some(1), |_| ());
        assert_eq!(lock(&m).last_error_at, Some(1_000), "an unchanged error keeps its time");
        update_locked(&m, 3_000, |s| s.last_error = None, |_| ());
        assert_eq!(lock(&m).last_error_at, None);
    }

    /// The audit's race: the Bluetooth actor, the MAP worker and a command thread update the
    /// status at once. Each update is sent while the lock is held, so the UI receives them in the
    /// order they were made, and the last one sent is the newest.
    #[test]
    fn concurrent_updates_are_sent_in_order() {
        const THREADS: usize = 8;
        const EACH: usize = 250;
        let m = Arc::new(Mutex::new(DeviceStatus::default()));
        let sent = Arc::new(Mutex::new(Vec::new()));
        let handles: Vec<_> = (0..THREADS)
            .map(|t| {
                let (m, sent) = (m.clone(), sent.clone());
                std::thread::spawn(move || {
                    for i in 0..EACH {
                        update_locked(
                            &m,
                            0,
                            |s| s.messages_error = Some(format!("{t}-{i}")),
                            |s| lock(&sent).push((s.rev, s.messages_error.clone())),
                        );
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let sent = lock(&sent);
        let revs: Vec<u64> = sent.iter().map(|(r, _)| *r).collect();
        let want: Vec<u64> = (1..=(THREADS * EACH) as u64).collect();
        assert_eq!(revs, want, "every change sent once, in order");
        let last = lock(&m);
        assert_eq!(sent.last().unwrap(), &(last.rev, last.messages_error.clone()));
    }

    #[test]
    fn a_read_taken_before_an_update_is_older() {
        // get_status at startup, then an event: the event's rev is higher, so the UI keeps it.
        let m = Mutex::new(DeviceStatus::default());
        update_locked(&m, 0, |s| s.connection = ConnectionState::Connecting, |_| ());
        let read = lock(&m).clone();
        let event = update_locked(&m, 0, |s| s.connection = ConnectionState::Connected, |_| ()).unwrap();
        assert!(event.rev > read.rev);
    }

    #[test]
    fn stamped_counts_reported_changes() {
        let m = Mutex::new(Stamped::<u32>::default());
        let mut sent = Vec::new();
        let set = |n: u32| {
            move |v: &mut u32| {
                *v = n;
                true
            }
        };
        update_stamped(&m, set(1), |s| sent.push(s.rev));
        update_stamped(&m, |_| false, |s| sent.push(s.rev));
        update_stamped(&m, set(2), |s| sent.push(s.rev));
        assert_eq!(sent, vec![1, 2]);
        assert_eq!(lock(&m).value, 2);
    }

    #[test]
    fn stamped_serializes_flat() {
        let np = Stamped {
            rev: 3,
            value: NowPlaying {
                title: Some("Song".into()),
                ..Default::default()
            },
        };
        let json = serde_json::to_value(&np).unwrap();
        assert_eq!(json["rev"], 3);
        assert_eq!(json["title"], "Song");
        assert!(json.get("value").is_none());
    }

    #[test]
    fn device_status_sends_its_rev() {
        let s = DeviceStatus {
            rev: 7,
            ..Default::default()
        };
        assert_eq!(serde_json::to_value(&s).unwrap()["rev"], 7);
    }

    /// Tugboat and Developer tools: emits from several threads and a command's read. Every copy
    /// gets a distinct rev in the order it was composed, and emits leave in that order.
    #[test]
    fn rev_counter_numbers_copies_in_composition_order() {
        let counter = Arc::new(RevCounter::default());
        let sent = Arc::new(Mutex::new(Vec::new()));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let (counter, sent) = (counter.clone(), sent.clone());
                std::thread::spawn(move || {
                    for _ in 0..250 {
                        counter.stamp(|rev| rev, |rev| lock(&sent).push(*rev));
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(*lock(&sent), (1..=2000).collect::<Vec<u64>>());
        // A command's read composed after the last event outranks it.
        let read = counter.stamp(|rev| rev, |_| ());
        assert!(read > 2000);
    }

    #[test]
    fn take_newer_drops_late_hooks() {
        let mut seen = 0;
        assert!(take_newer(&mut seen, 2));
        // The hook for rev 1 ran after the one for rev 2: ignored.
        assert!(!take_newer(&mut seen, 1));
        assert!(!take_newer(&mut seen, 2));
        assert!(take_newer(&mut seen, 3));
        assert_eq!(seen, 3);
    }
}
