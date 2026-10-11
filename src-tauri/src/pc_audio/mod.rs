//! "Play iPhone audio on this PC": the PC becomes a Bluetooth speaker (A2DP sink) for the paired
//! iPhone, through Windows' AudioPlaybackConnection (Windows 10 version 2004 and later).
//!
//! Turning it on finds the iPhone among the devices Windows can play audio from, creates the
//! connection, starts it and opens it. While it's open the iPhone lists this PC as an audio output
//! and its sound comes out of the PC's default speakers, so tug keeps the connection object alive
//! exactly as long as it's on, and lets it go on Stop, Forget, a phone switch and quitting.
//! Windows closing it (the phone went away, or picked another speaker) shows as off with Reconnect.
//!
//! Off by default. "Turn on automatically when my iPhone connects" is remembered for one phone (by
//! its device id) and only when the owner switches it on.
//!
//! The decisions live in `policy` (pure, tested); this file is the bookkeeping, and `io` the thin
//! Windows side. Every Windows call runs on a helper thread with a time limit
//! (`ble::winrt::off_thread`), so a stuck Bluetooth stack can't hold a command or the runtime.
//! AudioPlaybackConnection is agile (Send + Sync), so this needs no actor thread of its own.

pub mod policy;

use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use crate::state::{keys, ConnectionState, DeviceStatus, Shared};
pub use policy::PcAudioStatus;
use policy::{Action, Input, Machine, PcAudioState, PhoneIds};

/// The event carrying `PcAudioStatus` to the UI.
pub const EVENT: &str = "pc-audio";

/// After the iPhone connects, how long to wait before turning on by itself: the link's own setup
/// (notifications, texts) goes first, and a phone that only blipped in range is left alone.
const AUTO_DELAY: Duration = Duration::from_secs(5);

/// On quit, how long to wait for Windows to close the connection, so the phone's audio returns to
/// the phone rather than to a speaker that's gone.
const QUIT_WAIT: Duration = Duration::from_secs(2);

pub struct PcAudio {
    shared: Arc<Shared>,
    me: Weak<PcAudio>,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    machine: Machine,
    /// The open connection while it's on. Dropping it closes it.
    live: Option<io::Live>,
    supported: bool,
    /// The phone and whether it was connected at the last status update.
    last_device: Option<String>,
    last_connected: bool,
    /// What the UI was last sent, so it only hears about changes.
    sent: Option<PcAudioStatus>,
}

impl PcAudio {
    pub fn new(shared: Arc<Shared>) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            shared,
            me: me.clone(),
            inner: Mutex::default(),
        })
    }

    /// Check this Windows can do it, and follow the phone's connection (for the automatic switch,
    /// and to stop when tug moves to another phone).
    pub fn start(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        self.shared.set_status_hook(Box::new(move |s| {
            if let Some(me) = weak.upgrade() {
                me.on_device_status(s);
            }
        }));
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let supported = io::supported().await;
            if !supported {
                log::info!("PC audio: this version of Windows can't play a phone's audio");
            }
            me.lock().supported = supported;
            me.publish();
        });
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> PcAudioStatus {
        let auto = self.auto_now();
        Self::compose(&self.lock(), auto)
    }

    fn compose(inner: &Inner, auto: bool) -> PcAudioStatus {
        PcAudioStatus {
            supported: inner.supported,
            state: inner.machine.state(),
            problem: inner.machine.problem(),
            auto,
        }
    }

    /// Send the status to the UI if it changed. Emitted under the lock, so two updates racing
    /// can't reach the window in the wrong order.
    fn publish(&self) {
        let auto = self.auto_now();
        let mut inner = self.lock();
        let status = Self::compose(&inner, auto);
        if inner.sent != Some(status) {
            inner.sent = Some(status);
            self.shared.emit(EVENT, status);
        }
    }

    /// The phone the automatic switch was turned on for, if any.
    fn auto_device(&self) -> Option<String> {
        self.shared.store.setting(keys::PC_AUDIO_AUTO_DEVICE).ok().flatten()
    }

    /// Whether the automatic switch is on for the phone tug uses now.
    fn auto_now(&self) -> bool {
        let device = self.shared.status().device.map(|d| d.id);
        matches!((self.auto_device(), device), (Some(a), Some(d)) if a == d)
    }

    /// Feed the state machine and carry out what it decides. `result` is the connection an attempt
    /// produced (with `Input::Opened`); whatever isn't kept is dropped here, which closes it.
    fn step(&self, input: Input, mut result: Option<io::Live>) {
        let mut released = None;
        let mut open = None;
        {
            let mut inner = self.lock();
            match inner.machine.step(input) {
                Action::Nothing | Action::Discard => {}
                Action::Open(attempt) => open = Some(attempt),
                Action::Keep => {
                    released = inner.live.take();
                    inner.live = result.take();
                }
                Action::Release => released = inner.live.take(),
            }
        }
        // Closing happens on a helper thread (see `io::Live`), never under the lock.
        drop(result);
        if released.is_some() {
            log::info!("PC audio: off");
        }
        drop(released);
        if let Some(attempt) = open {
            self.spawn_open(attempt);
        }
        self.publish();
    }

    fn spawn_open(&self, attempt: u64) {
        let phone = self.phone_ids();
        let me = self.me.clone();
        let closed = self.me.clone();
        tauri::async_runtime::spawn(async move {
            let on_closed = move || {
                if let Some(me) = closed.upgrade() {
                    log::info!("PC audio: Windows closed the connection");
                    me.step(Input::Closed { attempt }, None);
                }
            };
            let (live, problem) = io::open(&phone, on_closed).await;
            if let Some(me) = me.upgrade() {
                me.step(Input::Opened { attempt, problem }, live);
            }
        });
    }

    /// What tug knows about its iPhone: the addresses inside its Windows device ids (the
    /// notifications side and the texts side) and its names.
    fn phone_ids(&self) -> PhoneIds {
        let status = self.shared.status();
        let texts_id = self.shared.store.setting(keys::TEXTS_DEVICE_ID).ok().flatten();
        let addresses = status
            .device
            .iter()
            .map(|d| d.id.as_str())
            .chain(texts_id.as_deref())
            .filter_map(policy::address_from_device_id)
            .collect();
        let names = status
            .device
            .iter()
            .map(|d| d.name.clone())
            .chain(status.texts_device.clone())
            .collect();
        PhoneIds { addresses, names }
    }

    /// "Play on this PC" (`true`), "Stop" (`false`). Turning on answers at once with Connecting;
    /// how it went arrives as a `pc-audio` event.
    pub fn set(&self, on: bool) -> PcAudioStatus {
        if on {
            if !self.lock().supported {
                return self.status();
            }
            log::info!("PC audio: turning on");
            self.step(Input::TurnOn, None);
        } else {
            self.step(Input::TurnOff, None);
        }
        self.status()
    }

    /// "Turn on automatically when my iPhone connects", for the phone tug uses now.
    pub fn set_auto(&self, on: bool) -> Result<PcAudioStatus, String> {
        let store = &self.shared.store;
        if on {
            let Some(device) = self.shared.status().device.map(|d| d.id) else {
                return Err("Connect your iPhone first.".into());
            };
            store.set_setting(keys::PC_AUDIO_AUTO_DEVICE, &device)
        } else {
            store.delete_setting(keys::PC_AUDIO_AUTO_DEVICE)
        }
        .map_err(|e| e.to_string())?;
        self.publish();
        Ok(self.status())
    }

    /// Forget: stop, and drop the automatic switch with the phone.
    pub fn forget(&self) {
        let _ = self.shared.store.delete_setting(keys::PC_AUDIO_AUTO_DEVICE);
        self.step(Input::TurnOff, None);
    }

    /// tug is quitting: close the connection now and wait (briefly) for Windows to finish, so the
    /// phone isn't left sending its audio to a PC that stopped listening.
    pub fn shutdown_now(&self) {
        let live = {
            let mut inner = self.lock();
            inner.machine.step(Input::TurnOff);
            inner.live.take()
        };
        if let Some(live) = live {
            log::info!("PC audio: off (tug is quitting)");
            live.close(QUIT_WAIT);
        }
    }

    /// Every status change (`Shared::update_status`): stop when tug moves to another phone (or
    /// forgets this one), and turn on by itself when the opted-in phone connects.
    fn on_device_status(&self, s: &DeviceStatus) {
        let device = s.device.as_ref().map(|d| d.id.clone());
        let connected = s.connection == ConnectionState::Connected;
        let (switched, was_connected, state, supported) = {
            let mut inner = self.lock();
            if inner.last_device == device && inner.last_connected == connected {
                return;
            }
            let switched = inner.last_device.is_some() && inner.last_device != device;
            let was = inner.last_connected && !switched;
            inner.last_device = device.clone();
            inner.last_connected = connected;
            (switched, was, inner.machine.state(), inner.supported)
        };
        if switched {
            self.step(Input::TurnOff, None);
        }
        let state = if switched { PcAudioState::Off } else { state };
        let auto_device = self.auto_device();
        if supported
            && policy::should_auto_start(
                auto_device.as_deref(),
                device.as_deref(),
                was_connected,
                connected,
                state,
            )
        {
            self.schedule_auto(device);
        }
        // The automatic switch is per phone, so a different phone can change it.
        self.publish();
    }

    fn schedule_auto(&self, device: Option<String>) {
        let me = self.me.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(AUTO_DELAY).await;
            let Some(me) = me.upgrade() else { return };
            let s = me.shared.status();
            let still = s.connection == ConnectionState::Connected
                && s.device.map(|d| d.id) == device
                && me.auto_device() == device;
            if still && me.lock().machine.state() == PcAudioState::Off {
                log::info!("PC audio: turning on (set to turn on when the iPhone connects)");
                me.step(Input::TurnOn, None);
            }
        });
    }
}

#[cfg(windows)]
mod io {
    //! The Windows side: list, create, start, open, close. Each call that can hang runs on a helper
    //! thread with a time limit. Nothing about the phone (names, addresses) is logged.

    use std::time::Duration;

    use windows::core::{IInspectable, HSTRING};
    use windows::Devices::Enumeration::DeviceInformation;
    use windows::Foundation::TypedEventHandler;
    use windows::Media::Audio::{AudioPlaybackConnection, AudioPlaybackConnectionState};

    use super::policy::{self, Candidate, OpenStatus, PcAudioProblem, PhoneIds};
    use crate::ble::winrt::{off_thread, BleError};

    /// Checking the API exists.
    const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
    /// Listing the devices Windows can play audio from.
    const FIND_TIMEOUT: Duration = Duration::from_secs(15);
    /// Creating the connection and starting it.
    const START_TIMEOUT: Duration = Duration::from_secs(15);
    /// Opening it: Windows asks the phone and reports `RequestTimedOut` itself; this is the
    /// backstop past that.
    const OPEN_TIMEOUT: Duration = Duration::from_secs(40);

    /// An open (or opening) connection and its StateChanged handler. Dropping it removes the
    /// handler and closes the connection on a helper thread, so no path can leave one behind.
    pub struct Live {
        conn: Option<AudioPlaybackConnection>,
        token: i64,
    }

    impl Live {
        /// Close now, waiting up to `wait` for Windows to finish.
        pub fn close(mut self, wait: Duration) {
            if let Some(conn) = self.conn.take() {
                close_conn(conn, self.token, wait);
            }
        }
    }

    impl Drop for Live {
        fn drop(&mut self) {
            if let Some(conn) = self.conn.take() {
                close_conn(conn, self.token, Duration::ZERO);
            }
        }
    }

    fn close_conn(conn: AudioPlaybackConnection, token: i64, wait: Duration) {
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("tug-pc-audio".into()).spawn(move || {
            let _ = conn.RemoveStateChanged(token);
            if let Err(e) = conn.Close() {
                log::debug!("PC audio: closing the connection failed: {}", e.message());
            }
            let _ = tx.send(());
        });
        match spawned {
            Ok(_) => {
                if !wait.is_zero() && rx.recv_timeout(wait).is_err() {
                    log::warn!("PC audio: Windows is still closing the connection");
                }
            }
            // The closure (and with it the last reference to the connection) is dropped.
            Err(e) => log::warn!("PC audio: couldn't start closing the connection: {e}"),
        }
    }

    /// Whether this Windows has AudioPlaybackConnection (Windows 10 version 2004 and later).
    pub async fn supported() -> bool {
        off_thread(PROBE_TIMEOUT, || Ok(AudioPlaybackConnection::GetDeviceSelector()?))
            .await
            .is_ok()
    }

    fn problem(e: &BleError) -> PcAudioProblem {
        if e.is_timeout() {
            PcAudioProblem::TimedOut
        } else {
            PcAudioProblem::Failed
        }
    }

    /// Find the iPhone, create its connection, start it and open it. Returns the connection when
    /// it opened, else why not. `on_closed` runs when Windows reports the connection closed.
    pub async fn open(
        phone: &PhoneIds,
        on_closed: impl Fn() + Send + Sync + 'static,
    ) -> (Option<Live>, Option<PcAudioProblem>) {
        let found = off_thread(FIND_TIMEOUT, || {
            let selector = AudioPlaybackConnection::GetDeviceSelector()?;
            let infos = DeviceInformation::FindAllAsyncAqsFilter(&selector)?.join()?;
            let mut out = Vec::new();
            for info in infos {
                out.push(Candidate {
                    id: info.Id()?.to_string(),
                    name: info.Name().map(|n| n.to_string()).unwrap_or_default(),
                });
            }
            Ok(out)
        })
        .await;
        let candidates = match found {
            Ok(c) => c,
            Err(e) => {
                log::warn!("PC audio: couldn't list audio devices: {e}");
                return (None, Some(problem(&e)));
            }
        };
        let Some(i) = policy::pick(&candidates, phone) else {
            log::info!(
                "PC audio: the iPhone isn't among the {} device(s) Windows can play audio from",
                candidates.len()
            );
            return (None, Some(PcAudioProblem::NotFound));
        };
        let id = HSTRING::from(candidates[i].id.as_str());
        let started = off_thread(START_TIMEOUT, move || {
            let conn = AudioPlaybackConnection::TryCreateFromId(&id)?;
            let handler = TypedEventHandler::<AudioPlaybackConnection, IInspectable>::new(move |sender, _| {
                if let Some(c) = sender.as_ref() {
                    if c.State()? == AudioPlaybackConnectionState::Closed {
                        on_closed();
                    }
                }
                Ok(())
            });
            let token = conn.StateChanged(&handler)?;
            // Owned from here on: if starting fails, dropping it closes the connection again.
            let live = Live {
                conn: Some(conn.clone()),
                token,
            };
            conn.StartAsync()?.join()?;
            Ok(live)
        })
        .await;
        let live = match started {
            Ok(l) => l,
            Err(e) => {
                log::warn!("PC audio: couldn't start the connection: {e}");
                return (None, Some(problem(&e)));
            }
        };
        let Some(conn) = live.conn.clone() else {
            return (None, Some(PcAudioProblem::Failed));
        };
        let opened = off_thread(OPEN_TIMEOUT, move || {
            let r = conn.OpenAsync()?.join()?;
            Ok((r.Status()?.0, r.ExtendedError().map(|h| h.0).unwrap_or(0)))
        })
        .await;
        match opened {
            Ok((raw, extended)) => {
                let status = OpenStatus::from_raw(raw);
                match status.problem() {
                    None => {
                        log::info!("PC audio: on");
                        (Some(live), None)
                    }
                    Some(p) => {
                        log::info!("PC audio: Windows didn't open it ({status:?}, 0x{extended:08X})");
                        (None, Some(p))
                    }
                }
            }
            Err(e) => {
                log::warn!("PC audio: opening failed: {e}");
                (None, Some(problem(&e)))
            }
        }
    }
}

#[cfg(not(windows))]
mod io {
    //! Not Windows: never supported.
    use std::time::Duration;

    use super::policy::{PcAudioProblem, PhoneIds};

    pub struct Live;

    impl Live {
        pub fn close(self, _wait: Duration) {}
    }

    pub async fn supported() -> bool {
        false
    }

    pub async fn open(
        _phone: &PhoneIds,
        _on_closed: impl Fn() + Send + Sync + 'static,
    ) -> (Option<Live>, Option<PcAudioProblem>) {
        (None, Some(PcAudioProblem::Failed))
    }
}
