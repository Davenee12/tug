//! Background message service: keeps a MAP session with the iPhone, pulls new
//! inbox messages into the store and sends replies. It also owns the phone's other
//! Classic Bluetooth services: contacts and recent calls (PBAP), and the experimental
//! hands-free dialing (`crate::hfp`).
//!
//! The inbox is polled (MAP only exposes a small recent window, and iOS posts
//! no notification for a conversation that's open on the phone), and a
//! Messages notification from ANCS triggers an immediate refresh. Recent calls
//! are pulled every few minutes, and soon after a call rings out on ANCS.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use crate::messages::StoredMessage;
use crate::state::Shared;

pub enum MapCommand {
    Refresh,
    /// Pull recent calls once `after` has passed (a call that just ended needs a moment to
    /// reach the phone's log).
    RefreshCalls(Duration),
    /// Mark these stored messages read on the phone (those it still lists as unread).
    MarkRead(Vec<i64>),
    Send {
        address: String,
        text: String,
        reply: oneshot::Sender<Result<StoredMessage, String>>,
    },
    /// Experimental: open the iPhone's hands-free link and, given a number, dial it.
    /// Without one it's the check Settings runs before turning Call buttons on.
    Dial {
        number: Option<String>,
        reply: oneshot::Sender<Result<(), String>>,
    },
    /// Bluetooth inventory: the MAP folder names, from the open session (never opens one).
    InventoryFolders {
        reply: oneshot::Sender<crate::bt_inventory::Probe<Vec<String>>>,
    },
}

#[derive(Clone)]
pub struct MapHandle {
    tx: mpsc::UnboundedSender<MapCommand>,
}

impl MapHandle {
    pub fn refresh(&self) {
        let _ = self.tx.send(MapCommand::Refresh);
    }

    pub fn refresh_calls(&self, after: Duration) {
        let _ = self.tx.send(MapCommand::RefreshCalls(after));
    }

    pub fn mark_read(&self, ids: Vec<i64>) {
        let _ = self.tx.send(MapCommand::MarkRead(ids));
    }

    pub async fn send(&self, address: String, text: String) -> Result<StoredMessage, String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(MapCommand::Send { address, text, reply })
            .map_err(|_| "Message service stopped".to_string())?;
        rx.await.map_err(|_| "Message service stopped".to_string())?
    }

    pub async fn dial(&self, number: Option<String>) -> Result<(), String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(MapCommand::Dial { number, reply })
            .map_err(|_| "Message service stopped".to_string())?;
        rx.await.map_err(|_| "Message service stopped".to_string())?
    }

    /// The MAP folder names for the Bluetooth inventory.
    pub async fn inventory_folders(&self) -> crate::bt_inventory::Probe<Vec<String>> {
        let (reply, rx) = oneshot::channel();
        if self.tx.send(MapCommand::InventoryFolders { reply }).is_err() {
            return crate::bt_inventory::Probe::Unavailable("message service stopped".into());
        }
        rx.await
            .unwrap_or_else(|_| crate::bt_inventory::Probe::Unavailable("message service unavailable".into()))
    }
}

pub fn start(shared: Arc<Shared>) -> MapHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    #[cfg(windows)]
    {
        std::thread::Builder::new()
            .name("tug-map".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                    .expect("build MAP runtime");
                rt.block_on(worker::run(shared, rx));
                log::error!("MAP service exited");
            })
            .expect("spawn MAP thread");
    }
    #[cfg(not(windows))]
    {
        let _ = shared;
        let mut rx = rx;
        tauri::async_runtime::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    MapCommand::Send { reply, .. } => {
                        let _ = reply.send(Err("Messaging is only supported on Windows".into()));
                    }
                    MapCommand::Dial { reply, .. } => {
                        let _ = reply.send(Err("Calling is only supported on Windows".into()));
                    }
                    _ => {}
                }
            }
        });
    }
    MapHandle { tx }
}

#[cfg(windows)]
mod worker {
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
    use tokio::time::Instant;

    use super::MapCommand;
    use crate::map::address::normalize;
    use crate::map::health::{Attempt, Health, LiveTexts, TextsPairing};
    use crate::map::listing;
    use crate::map::mns::{self, Outgoing};
    use crate::map::obex::RSP_NOT_FOUND;
    use crate::map::pick::choose_device;
    use crate::map::session::{find_devices, pull_call_history, pull_contacts, MapDevice, MapError, MapSession};
    use crate::messages::{IncomingMessage, Status, StoredMessage, SOURCE_IPHONE_MAP};
    use crate::state::{events, keys, ConnectionState, Shared};

    const FIRST_SYNC_DELAY: Duration = Duration::from_secs(3);
    /// Contacts (a full PBAP pull) are retried no faster than this while watching.
    const CONTACTS_WATCHING: Duration = Duration::from_secs(10);
    /// How many of the newest inbox messages to look at each poll.
    const LIST_MAX: u16 = 20;
    /// Once per launch, page further back than that: a fresh install otherwise only sees the
    /// last few texts, often all from one person, and other recent chats never show up.
    const BACKFILL_MAX: u16 = 100;
    /// The phone sends nothing when a contact is added or renamed, so look again this often
    /// (a pull of a few hundred contacts takes about a second).
    const CONTACTS_RESYNC: Duration = Duration::from_secs(15 * 60);
    const CONTACTS_RETRY: Duration = Duration::from_secs(10 * 60);
    /// An empty phonebook or a refusal means Sync Contacts is still off: it's often switched
    /// on moments after messages connect, so look again soon, then back off.
    const CONTACTS_UNSHARED_RETRY: Duration = Duration::from_secs(20);
    const CONTACTS_UNSHARED_QUICK_TRIES: u32 = 15;
    const CONTACTS_PULL_TIMEOUT: Duration = Duration::from_secs(90);
    /// The slow WITH-PHOTO phonebook pull runs off this worker, so give it well over the ~60 s it
    /// took on Dave's phone for 27 photos — nothing is waiting on it, and a timeout only ends the
    /// pass cleanly.
    const PHOTO_PULL_TIMEOUT: Duration = Duration::from_secs(120);
    /// Contact photos rarely change; pull them at most once a day (the last time is persisted, so
    /// the pass skips every reconnect and 15-min resync in between).
    const PHOTO_SYNC_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
    /// How many recent calls to show, like the phone's own Recents screen.
    const CALLS_MAX: u16 = 50;
    const CALLS_RESYNC: Duration = Duration::from_secs(5 * 60);
    /// Each pull is a whole PBAP session: however often one is asked for, this far apart is plenty.
    const CALLS_MIN_GAP: Duration = Duration::from_secs(10);
    const CALLS_PULL_TIMEOUT: Duration = Duration::from_secs(45);
    /// Connect, hands-free setup, dial and a moment to hear the call start, end to end. Kept
    /// short: texts (send, sync, mark read) wait on this worker while a call is being placed.
    const DIAL_TIMEOUT: Duration = Duration::from_secs(15);
    /// How long to keep the hands-free link after dialing, so the call is under way before
    /// tug lets go of it.
    const DIAL_HOLD: Duration = Duration::from_secs(3);
    /// Live texts count as "fresh" (letting the backstop poll relax) for this long after the last
    /// event; if the phone goes quiet, the poll tightens back up to catch anything the MNS missed.
    const LIVE_EVENT_FRESH: Duration = Duration::from_secs(60);
    /// After registering, how long to wait for the phone to connect to our MNS before saying (once)
    /// that live texts aren't working and we're polling only.
    const MNS_CONNECT_GRACE: Duration = Duration::from_secs(30);
    /// At most one "reopen message access" per this long after the MNS link drops, so a phone
    /// that keeps dropping it can't make tug reconnect in a loop.
    const MNS_REOPEN_GAP: Duration = Duration::from_secs(60);
    /// When the iPhone is reachable but isn't offering message access (Classic/MAP not up — e.g.
    /// locked after a restart), retrying every POLL_DISCONNECTED all night is pointless (Dave's log
    /// had ~839 of these in 21 h). Back off progressively from there to a few minutes, while still
    /// retrying on a real change (BLE link up, PC resume, Bluetooth on, user on Settings › iPhone).
    const MAP_RETRY_BASE: Duration = Duration::from_secs(30);
    const MAP_RETRY_CAP: Duration = Duration::from_secs(5 * 60);

    /// How long to wait before the next message-access attempt after `failures` in a row: the base
    /// delay doubling each time, capped. Pure, so the backoff is unit-tested.
    fn map_retry_delay(failures: u32) -> Duration {
        let doublings = failures.saturating_sub(1).min(5);
        (MAP_RETRY_BASE * (1u32 << doublings)).min(MAP_RETRY_CAP)
    }

    fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    struct Worker {
        shared: Arc<Shared>,
        session: Option<MapSession>,
        /// Classic device the MAP session is on; contacts come from the same phone.
        device_id: Option<String>,
        next_contacts_sync: Instant,
        unshared_contact_pulls: u32,
        /// The last contacts pull came back with people in it, so Sync Contacts is on: an empty
        /// call list then really means the history was cleared.
        contacts_shared: bool,
        backfilled: bool,
        health: Health,
        /// The phone Windows has paired for texts, found even when connecting to it fails.
        texts_device: Option<String>,
        next_calls_sync: Instant,
        last_calls_pull: Option<Instant>,
        /// The running MNS server (live texts), while a session is up and it started.
        mns: Option<mns::MnsServer>,
        /// Cloned into each MNS server so the phone's events reach the worker's select loop.
        events_tx: UnboundedSender<mns::ServerMessage>,
        /// Live-texts state mirrored into `DeviceStatus`.
        live: LiveTexts,
        /// When the last MNS event (or connect) arrived, for the fresh-enough-to-relax-the-poll check.
        last_event_at: Option<Instant>,
        /// When registration was sent, to notice a phone that never connects to the MNS.
        mns_registered_at: Option<Instant>,
        /// So the "phone never connected, polling only" line is logged once per session.
        mns_grace_warned: bool,
        /// When the last MNS drop made tug reopen message access (rate-limits that recovery).
        mns_reopened_at: Option<Instant>,
        /// The background contact-photo pass, on its own thread (the WITH-PHOTO PBAP pull holds
        /// non-Send WinRT handles, so it can't live on the worker's runtime). Kept only to tell
        /// whether a previous pass is still running, so two never overlap.
        photo_task: Option<std::thread::JoinHandle<()>>,
        /// Whether the photo pass has already been started (or deliberately skipped) on this
        /// connection, so it runs at most once per connection. Reset when the session drops.
        photo_pass_started: bool,
        /// Last logged message-type counts, so the "message types" line logs only on change.
        last_type_counts: Option<(u32, u32, u32)>,
        /// Consecutive message-access failures while the phone is reachable but not offering it,
        /// and when to try again. Drives a progressive backoff so tug isn't retrying every 30 s all
        /// night; reset on success and bypassed on a real change (see the poll loop and `map.refresh`).
        connect_failures: u32,
        next_retry: Option<Instant>,
    }

    pub async fn run(shared: Arc<Shared>, mut commands: UnboundedReceiver<MapCommand>) {
        // The MNS server pushes the phone's events here; the worker selects on them alongside
        // commands and the poll timer. Created up front so `events_rx` is always selectable even
        // before a server exists.
        let (events_tx, mut events_rx) = mpsc::unbounded_channel::<mns::ServerMessage>();
        let mut w = Worker {
            shared,
            session: None,
            device_id: None,
            next_contacts_sync: Instant::now(),
            unshared_contact_pulls: 0,
            contacts_shared: false,
            backfilled: false,
            health: Health::default(),
            texts_device: None,
            next_calls_sync: Instant::now(),
            last_calls_pull: None,
            mns: None,
            events_tx,
            live: LiveTexts::Off,
            last_event_at: None,
            mns_registered_at: None,
            mns_grace_warned: false,
            mns_reopened_at: None,
            photo_task: None,
            photo_pass_started: false,
            last_type_counts: None,
            connect_failures: 0,
            next_retry: None,
        };
        // Old names from renames that happened while tug wasn't keeping track.
        match w.shared.store.learn_aliases() {
            Ok(0) => {}
            Ok(n) => {
                log::info!("learned {n} earlier contact name(s)");
                if let Ok(all) = w.shared.store.contacts() {
                    w.shared.emit(events::CONTACTS, all);
                }
            }
            Err(e) => log::warn!("learning old contact names failed: {e}"),
        }
        let mut next = Instant::now() + FIRST_SYNC_DELAY;
        loop {
            tokio::select! {
                cmd = commands.recv() => match cmd {
                    None => break,
                    Some(MapCommand::Refresh) => w.refresh().await,
                    // Wake for it: the next refresh pulls the calls once they're due.
                    Some(MapCommand::RefreshCalls(after)) => next = next.min(w.calls_soon(after)),
                    Some(MapCommand::MarkRead(ids)) => w.mark_read(&ids).await,
                    Some(MapCommand::Send { address, text, reply }) => {
                        let _ = reply.send(w.send(&address, &text).await);
                    }
                    Some(MapCommand::Dial { number, reply }) => {
                        let _ = reply.send(w.dial(number.as_deref()).await);
                    }
                    Some(MapCommand::InventoryFolders { reply }) => {
                        let _ = reply.send(w.inventory_folders().await);
                    }
                },
                // The phone pushed a live-texts event (or just connected to the MNS).
                Some(msg) = events_rx.recv() => w.on_mns(msg).await,
                _ = tokio::time::sleep_until(next) => w.refresh().await,
            }
            if Instant::now() >= next {
                next = Instant::now() + mns::poll_after(w.shared.watching(), w.session.is_some(), w.live_fresh());
                // Honour the message-access backoff, unless the user is on the iPhone screens
                // (watching), when they want prompt retries while flipping switches.
                if let Some(retry) = w.next_retry.filter(|_| !w.shared.watching()) {
                    next = next.max(retry);
                }
            }
        }
        // Sender dropped (app exiting): deregister on the phone and stop advertising cleanly.
        w.shutdown().await;
    }

    impl Worker {
        /// Track whether the texts pairing works, so a broken one is shown instead of only logged.
        fn record_health(&mut self, result: &Result<usize, MapError>) {
            // WSAENETUNREACH: what a connection to a phone that no longer accepts the pairing gives.
            const NET_UNREACHABLE: windows::core::HRESULT = windows::core::HRESULT(0x8007_2743_u32 as i32);
            let attempt = match result {
                Ok(_) => Attempt::Connected,
                Err(MapError::NoDevice) => Attempt::NoDevice,
                Err(MapError::Consent) => Attempt::Answered,
                Err(MapError::NoService) => Attempt::Unreachable,
                Err(MapError::Win(e)) if e.code() == NET_UNREACHABLE => Attempt::Unreachable,
                Err(_) => Attempt::Other,
            };
            let linked = self.shared.status().connection == ConnectionState::Connected;
            let before = self.health.state();
            let state = self.health.record(attempt, linked, std::time::Instant::now());
            if state == TextsPairing::Broken && before != TextsPairing::Broken {
                log::warn!("texts pairing looks broken: the phone is nearby but won't take the connection");
            }
            if attempt == Attempt::NoDevice {
                self.texts_device = None;
            }
            let device = self.texts_device.clone();
            self.shared.update_status(|s| {
                s.texts_pairing = state;
                s.texts_device = device;
            });
        }

        fn set_state(&self, connected: bool, error: Option<String>) {
            self.shared.update_status(|s| {
                s.services.messages = connected;
                s.messages_error = error;
            });
        }

        /// The paired Classic device that is this iPhone. Only ever called once setup has adopted
        /// an LE phone (see `refresh`), so the texts device is picked to match that phone (by the
        /// remembered id first, then its name) rather than grabbing whatever is paired.
        async fn pick_device(&self) -> Result<MapDevice, MapError> {
            // Don't touch any phone until setup has chosen one. On a fresh install an old paired
            // Classic iPhone would otherwise be connected and retried before the user picked.
            if self.shared.status().device.is_none() {
                return Err(MapError::NoDevice);
            }
            let devices = find_devices().await?;
            // Prefer the phone we last connected to by id (survives a rename); then the
            // notifications phone's name; then a lone phone. Logic (and its tests) in `pick`.
            let stored = self.shared.store.setting(keys::TEXTS_DEVICE_ID).ok().flatten();
            let wanted = self.shared.status().device.map(|d| d.name);
            choose_device(&devices, stored.as_deref(), wanted.as_deref())
                .cloned()
                .ok_or(MapError::NoDevice)
        }

        /// Remember the phone a MAP session just connected to, so later picks prefer it by id
        /// even if it's renamed. Written only when it changes (the first connection, or a
        /// different phone).
        fn remember_texts_device(&self, id: &str) {
            if self
                .shared
                .store
                .setting(keys::TEXTS_DEVICE_ID)
                .ok()
                .flatten()
                .as_deref()
                == Some(id)
            {
                return;
            }
            if let Err(e) = self.shared.store.set_setting(keys::TEXTS_DEVICE_ID, id) {
                log::warn!("remembering the texts device failed: {e}");
            }
        }

        async fn ensure(&mut self) -> Result<&mut MapSession, MapError> {
            if self.session.is_none() {
                let device = self.pick_device().await?;
                self.texts_device = Some(device.name.clone());
                let session = MapSession::connect(&device.id).await?;
                log::info!("message access connected to {}", device.name);
                // Only once connecting worked: a device that won't connect isn't "the phone".
                self.remember_texts_device(&device.id);
                // The LE side often reports the bare "iPhone"; the Classic side carries the real
                // name ("Jordan's iPhone"). Adopt it when it's more specific.
                let current = self.shared.status().device.map(|d| d.name).unwrap_or_default();
                if crate::device_kind::more_specific_name(&current, &device.name) {
                    log::info!("using the Classic name '{}' for the iPhone", device.name);
                    let _ = self.shared.store.set_setting(keys::DEVICE_NAME, &device.name);
                    let name = device.name.clone();
                    self.shared.update_status(|s| {
                        if let Some(d) = s.device.as_mut() {
                            d.name = name;
                        }
                    });
                }
                self.device_id = Some(device.id.clone());
                self.session = Some(session);
                self.set_state(true, None);
                // Live texts: advertise the MNS and register for notifications. Failure here only
                // turns live texts off; the inbox poll keeps working.
                self.start_live_texts().await;
            }
            Ok(self.session.as_mut().expect("session just set"))
        }

        /// Bring up live texts for the open session: advertise the MNS server, then (only once it
        /// is advertising) ask the phone to start pushing notifications. Any failure is logged once
        /// and leaves tug polling — an unpackaged-app RFCOMM server is unproven on iOS.
        async fn start_live_texts(&mut self) {
            if self.mns.is_some() {
                return;
            }
            // Advertise first, so the phone has an SDP record to find the moment it's registered.
            match mns::start(self.events_tx.clone()).await {
                Ok(server) => {
                    self.mns = Some(server);
                    log::info!("live texts: MNS server started, SDP published (service 0x1133, MAP profile 1.1)");
                }
                Err(e) => {
                    log::info!("live texts: couldn't start the MNS server ({e}); polling only");
                    self.set_live(LiveTexts::Unavailable);
                    return;
                }
            }
            let Some(session) = self.session.as_mut() else {
                return;
            };
            match session.set_notification_registration(true).await {
                Ok(()) => {
                    log::info!("live texts: notification registration sent; waiting for the iPhone to connect");
                    self.mns_registered_at = Some(Instant::now());
                    self.mns_grace_warned = false;
                    self.set_live(LiveTexts::Starting);
                }
                Err(e) => {
                    log::info!("live texts: the iPhone refused notification registration ({e}); polling only");
                    self.mns = None; // dropping it stops advertising
                    self.set_live(LiveTexts::Unavailable);
                }
            }
        }

        /// Tear live texts down (session dropped): stop advertising and forget the registration.
        fn stop_live_texts(&mut self) {
            if self.mns.take().is_some() {
                log::info!("live texts: stopped (message access dropped)");
            }
            self.mns_registered_at = None;
            self.last_event_at = None;
            self.set_live(LiveTexts::Off);
        }

        fn set_live(&mut self, state: LiveTexts) {
            if self.live != state {
                self.live = state;
                self.shared.update_status(|s| s.live_texts = state);
            }
        }

        /// Whether live texts have been heard from recently enough to relax the backstop poll.
        fn live_fresh(&self) -> bool {
            self.live == LiveTexts::Active && self.last_event_at.is_some_and(|t| t.elapsed() < LIVE_EVENT_FRESH)
        }

        /// Deregister cleanly on shutdown (best-effort; the app is exiting).
        async fn shutdown(&mut self) {
            if self.mns.is_some() {
                if let Some(session) = self.session.as_mut() {
                    let _ = session.set_notification_registration(false).await;
                }
            }
            self.stop_live_texts();
        }

        fn set_contacts_shared(&mut self, shared: bool) {
            self.contacts_shared = shared;
            self.shared.update_status(|s| s.contacts_shared = shared);
        }

        fn fail(&mut self, e: &MapError) {
            if self.session.take().is_some() {
                log::info!("message access dropped: {e}");
            }
            // What the phone shares is per connection; ask again on the next one.
            self.set_contacts_shared(false);
            self.reset_photo_pass();
            self.stop_live_texts();
            let shown = match e {
                MapError::Consent | MapError::NoService => Some(e.to_string()),
                _ => None,
            };
            self.set_state(false, shown);
        }

        /// Bluetooth inventory: list the MAP folders on the open session. A timeout or a closed link
        /// may leave the session out of step, so it's dropped like any failed sync.
        async fn inventory_folders(&mut self) -> crate::bt_inventory::Probe<Vec<String>> {
            use crate::bt_inventory::Probe;
            let Some(session) = self.session.as_mut() else {
                return Probe::Unavailable("message access isn't connected right now".into());
            };
            let result = crate::map::probe::folders(session).await;
            if let Err(e @ (MapError::Timeout | MapError::Closed)) = &result {
                self.fail(e);
            }
            Probe::from_result(result)
        }

        /// Handle one message from the MNS server: the phone connecting, or an event report.
        async fn on_mns(&mut self, msg: mns::ServerMessage) {
            match msg {
                mns::ServerMessage::Connected => {
                    if self.live != LiveTexts::Active {
                        log::info!("live texts: the iPhone connected to the notification server");
                    }
                    self.last_event_at = Some(Instant::now());
                    self.set_live(LiveTexts::Active);
                }
                mns::ServerMessage::Disconnected => {
                    // Seen on Jordan's iPhone: once registered, an open session's inbox listing
                    // stopped showing new texts after the MNS link died, so the poll found
                    // nothing until a fresh session. Reopen message access, which re-registers.
                    let recent = self.mns_reopened_at.is_some_and(|t| t.elapsed() < MNS_REOPEN_GAP);
                    if recent {
                        log::info!("live texts: the iPhone dropped the notification link again; polling only");
                        self.set_live(LiveTexts::Unavailable);
                        return;
                    }
                    log::info!("live texts: the iPhone dropped the notification link; reopening message access");
                    self.mns_reopened_at = Some(Instant::now());
                    self.session = None;
                    self.reset_photo_pass();
                    self.stop_live_texts();
                    self.refresh().await;
                }
                mns::ServerMessage::Event(event) => {
                    crate::bt_inventory::record_mns_event(event.kind);
                    self.last_event_at = Some(Instant::now());
                    self.set_live(LiveTexts::Active);
                    log::debug!(
                        "live texts: event {:?} handle {:?} folder {:?}",
                        event.kind,
                        event.handle,
                        event.folder
                    );
                    match mns::event_action(&event) {
                        mns::EventAction::Refresh => self.refresh().await,
                        mns::EventAction::SendConfirmed { handle } => {
                            self.confirm_send(handle.as_deref(), Status::Sent).await;
                        }
                        mns::EventAction::SendFailed { handle } => {
                            self.confirm_send(handle.as_deref(), Status::Failed).await;
                        }
                        mns::EventAction::Ignore => {}
                    }
                }
            }
        }

        /// Update the outgoing message a Sending{Success,Failure} report is about. The report's
        /// handle may not equal the one PushMessage returned (and it carries no recipient), so the
        /// match is handle-first then newest-unconfirmed; see `mns::choose_outgoing`.
        async fn confirm_send(&mut self, handle: Option<&str>, status: Status) {
            let store = self.shared.store.clone();
            let candidates = match store.outgoing_unconfirmed(SOURCE_IPHONE_MAP) {
                Ok(c) => c,
                Err(e) => return log::warn!("reading unconfirmed sends failed: {e}"),
            };
            let outs: Vec<Outgoing> = candidates
                .into_iter()
                .map(|(id, handle, received_at)| Outgoing {
                    id,
                    handle,
                    received_at,
                })
                .collect();
            let Some(id) = mns::choose_outgoing(handle, &outs) else {
                log::debug!("live texts: a send report ({status:?}) matched no pending message");
                return;
            };
            match store.set_outgoing_status(id, status, None) {
                Ok(m) => {
                    log::info!(
                        "live texts: a sent message is now {}",
                        if status == Status::Sent {
                            "confirmed sent"
                        } else {
                            "marked failed"
                        }
                    );
                    self.shared.emit(events::MESSAGE, m);
                }
                Err(e) => log::warn!("updating sent status failed: {e}"),
            }
        }

        /// When to ask again while contacts aren't shared yet: soon at first, then the usual retry.
        fn soon(&mut self) -> Duration {
            // While the switches are on screen, still not every 2 s: each try is a whole PBAP
            // connection to the phone.
            if self.shared.watching() {
                return CONTACTS_WATCHING;
            }
            self.unshared_contact_pulls += 1;
            if self.unshared_contact_pulls <= CONTACTS_UNSHARED_QUICK_TRIES {
                CONTACTS_UNSHARED_RETRY
            } else {
                CONTACTS_RETRY
            }
        }

        /// Pull the phone's contacts (PBAP) now and then, for names and new chats.
        async fn sync_contacts_if_due(&mut self) {
            let Some(device_id) = self.device_id.clone() else {
                return;
            };
            if Instant::now() < self.next_contacts_sync {
                return;
            }
            // Bounded as a whole: the pull runs on the same worker as sending and mark-read. No
            // photos here (`with_photos: false`) — asking for them made the pull ~4x slower and so
            // blocked texts; a separate background pass fetches faces off this worker.
            let pulled = tokio::time::timeout(CONTACTS_PULL_TIMEOUT, pull_contacts(&device_id, false))
                .await
                .unwrap_or(Err(MapError::Timeout));
            match pulled {
                // The iPhone answers with an empty list, not a refusal, while Sync Contacts is
                // off. Keep any names already saved and ask again rather than in 6 hours.
                Ok(entries) if entries.is_empty() => {
                    self.set_contacts_shared(false);
                    log::debug!("the iPhone shared no contacts (Sync Contacts off?), asking again soon");
                    self.next_contacts_sync = Instant::now() + self.soon();
                }
                Ok(entries) => {
                    self.unshared_contact_pulls = 0;
                    self.set_contacts_shared(true);
                    // Names and numbers only. save_phonebook never touches the photo column, so the
                    // references the background photo pass set are preserved across every fast sync.
                    let mut rows: Vec<(String, String)> = Vec::new();
                    for e in &entries {
                        for n in &e.numbers {
                            rows.push((normalize(n), e.name.clone()));
                        }
                    }
                    match self.shared.store.save_phonebook(&rows) {
                        Ok(n) => {
                            log::info!("contacts synced: {} people, {n} numbers", entries.len());
                            // A rename on the phone may leave history under the old name.
                            if let Err(e) = self.shared.store.learn_aliases() {
                                log::warn!("learning old contact names failed: {e}");
                            }
                            if let Ok(all) = self.shared.store.contacts() {
                                self.shared.emit(events::CONTACTS, all);
                            }
                        }
                        Err(e) => log::warn!("saving contacts failed: {e}"),
                    }
                    self.shared.update_status(|s| s.contacts_error = None);
                    self.next_contacts_sync = Instant::now() + CONTACTS_RESYNC;
                    // Recent calls sit behind the same switch: if it just came on, they're there too.
                    self.next_calls_sync = Instant::now();
                    // Faces come from a slower WITH-PHOTO pull on its own PBAP link, off this worker
                    // so texts keep flowing; start it now that the contact rows exist (at most daily).
                    self.maybe_sync_photos();
                }
                Err(e) => {
                    log::info!("contacts sync failed: {e}");
                    let consent = matches!(e, MapError::ContactsConsent);
                    if consent {
                        self.set_contacts_shared(false);
                    }
                    let shown = consent.then(|| e.to_string());
                    self.shared.update_status(|s| s.contacts_error = shown);
                    // A refusal is the switch still being off, like an empty list: during setup
                    // it's usually flipped seconds later, so ask again soon.
                    self.next_contacts_sync = Instant::now() + if consent { self.soon() } else { CONTACTS_RETRY };
                }
            }
        }

        /// Start the background contact-photo pass if it's time. It runs off this worker on its own
        /// PBAP OBEX link (PBAP and MAP are separate RFCOMM services, so they coexist), so it never
        /// holds up message sync, sending, mark-read or live-text events. Started at most once per
        /// connection, and — across connections — at most once a day (the last success is persisted).
        fn maybe_sync_photos(&mut self) {
            if self.photo_pass_started {
                return;
            }
            // Never two at once: a pass from a quick earlier reconnect may still be finishing.
            if self.photo_task.as_ref().is_some_and(|h| !h.is_finished()) {
                return;
            }
            let Some(device_id) = self.device_id.clone() else {
                return;
            };
            // One attempt per connection either way: mark it before the due check so a not-due
            // connection doesn't re-read the setting on every 15-minute resync.
            self.photo_pass_started = true;
            let last = self
                .shared
                .store
                .setting(keys::LAST_PHOTO_SYNC)
                .ok()
                .flatten()
                .and_then(|v| v.parse::<i64>().ok());
            if !photo_sync_due(last, now_ms(), PHOTO_SYNC_INTERVAL.as_millis() as i64) {
                log::debug!("contact photos synced within the day; skipping the photo pass");
                return;
            }
            let shared = self.shared.clone();
            // Its own thread with its own current-thread runtime, like the MAP worker: the pull's
            // WinRT handles aren't Send, so it can't be a task on the worker's runtime, and this
            // keeps the slow pull entirely off the worker so texts never wait on it.
            self.photo_task = std::thread::Builder::new()
                .name("tug-photos".into())
                .spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_time()
                        .build()
                        .expect("build photo runtime");
                    rt.block_on(sync_contact_photos(shared, device_id));
                })
                .map_err(|e| log::warn!("couldn't start the contact-photo pass: {e}"))
                .ok();
        }

        /// Let the next connection start a fresh photo pass. A pass already running isn't force
        /// killed — it's its own thread — but it ends on its own: once the phone is gone the pull
        /// fails fast (bounded by a timeout anyway), and a failed pull never clears existing photos,
        /// so a late result is harmless. `maybe_sync_photos` won't start another until it finishes.
        fn reset_photo_pass(&mut self) {
            self.photo_pass_started = false;
        }

        /// Pull recent calls no sooner than `after` from now (nor `CALLS_MIN_GAP` after the
        /// last pull); returns when.
        fn calls_soon(&mut self, after: Duration) -> Instant {
            let mut at = Instant::now() + after;
            if let Some(last) = self.last_calls_pull {
                at = at.max(last + CALLS_MIN_GAP);
            }
            self.next_calls_sync = self.next_calls_sync.min(at);
            self.next_calls_sync
        }

        /// Pull the phone's recent calls (PBAP call history) when they're due.
        async fn sync_calls_if_due(&mut self) {
            let Some(device_id) = self.device_id.clone() else {
                return;
            };
            if Instant::now() < self.next_calls_sync {
                return;
            }
            // The background photo pass holds the phone's contacts (PBAP) link; a second PBAP
            // connection fails with "only one usage of each socket address". Wait for it.
            if self.photo_task.as_ref().is_some_and(|t| !t.is_finished()) {
                self.next_calls_sync = Instant::now() + Duration::from_secs(5);
                return;
            }
            self.last_calls_pull = Some(Instant::now());
            self.next_calls_sync = Instant::now() + CALLS_RESYNC;
            let pulled = tokio::time::timeout(CALLS_PULL_TIMEOUT, pull_call_history(&device_id, CALLS_MAX))
                .await
                .unwrap_or(Err(MapError::Timeout));
            match pulled {
                // An empty answer is also how Sync Contacts being off looks, so it only wipes the
                // list when contacts are coming through (then the history really was cleared).
                // (A refusal shows as the contacts error.)
                Ok(calls) if calls.is_empty() && !self.contacts_shared && !self.shared.calls().is_empty() => {
                    log::info!("the iPhone shared no recent calls; keeping the ones tug has");
                }
                Ok(calls) => {
                    log::info!("recent calls synced: {}", calls.len());
                    self.shared.set_calls(calls);
                }
                Err(e) => log::info!("recent calls sync failed: {e}"),
            }
        }

        /// Experimental hands-free dialing (`crate::hfp`); without a number, only the check.
        async fn dial(&mut self, number: Option<&str>) -> Result<(), String> {
            let device_id = match self.device_id.clone() {
                Some(id) => id,
                None => self.pick_device().await.map_err(|e| e.to_string())?.id,
            };
            let hold = if number.is_some() { DIAL_HOLD } else { Duration::ZERO };
            let result = tokio::time::timeout(DIAL_TIMEOUT, crate::hfp::session::run(&device_id, number, hold))
                .await
                .unwrap_or(Err(crate::hfp::session::HfpError::Timeout));
            match result {
                Ok(report) => {
                    log::info!(
                        "hands-free {}: phone features {:?}, call {:?}",
                        if number.is_some() { "dial" } else { "check" },
                        report.ag_features,
                        report.progress
                    );
                    Ok(())
                }
                Err(e) => {
                    log::warn!(
                        "hands-free {}: {e}",
                        if number.is_some() {
                            "dial failed"
                        } else {
                            "check failed"
                        }
                    );
                    Err(e.to_string())
                }
            }
        }

        async fn refresh(&mut self) {
            // Until setup has adopted an LE phone, don't connect to (or report health for) any
            // paired phone: a fresh install must not latch onto a stale bond before the user picks.
            if self.shared.status().device.is_none() {
                if self.session.take().is_some() {
                    log::info!("message access paused: setup hasn't adopted an iPhone yet");
                    self.device_id = None;
                    self.set_state(false, None);
                }
                return;
            }
            let result = self.sync().await;
            self.record_health(&result);
            if result.is_ok() {
                self.sync_contacts_if_due().await;
                self.sync_calls_if_due().await;
            }
            self.check_mns_grace();
            match result {
                Ok(0) => self.note_sync_ok(),
                Ok(n) => {
                    self.note_sync_ok();
                    log::info!("{n} new message(s) from the iPhone");
                }
                // No paired phone to reach yet: nothing to back off from.
                Err(MapError::NoDevice) => {
                    self.note_sync_ok();
                    self.set_state(false, None);
                }
                Err(e) => {
                    self.note_sync_failure(&e);
                    self.fail(&e);
                }
            }
        }

        /// Message access is up: clear the backoff so the next change retries promptly.
        fn note_sync_ok(&mut self) {
            self.connect_failures = 0;
            self.next_retry = None;
        }

        /// Message access failed (phone reachable but not offering it, or the connection dropped):
        /// back off progressively and log the first few, then a periodic summary, instead of a line
        /// every poll all night.
        fn note_sync_failure(&mut self, e: &MapError) {
            self.connect_failures = self.connect_failures.saturating_add(1);
            if self.connect_failures <= 3 || self.connect_failures.is_multiple_of(10) {
                log::info!("message access retry failed ({} in a row): {e}", self.connect_failures);
            } else {
                log::debug!("message sync failed ({} in a row): {e}", self.connect_failures);
            }
            self.next_retry = Some(Instant::now() + map_retry_delay(self.connect_failures));
        }

        /// If registration went out but the phone still hasn't connected to the MNS, say so once
        /// and mark live texts unavailable. The poll backstop keeps running regardless.
        fn check_mns_grace(&mut self) {
            if self.live != LiveTexts::Starting || self.mns_grace_warned {
                return;
            }
            if self.mns_registered_at.is_some_and(|t| t.elapsed() >= MNS_CONNECT_GRACE) {
                log::info!(
                    "live texts: the iPhone hasn't connected to the MNS within {}s; polling only \
                     (expected if iOS won't connect to an unpackaged app's RFCOMM server)",
                    MNS_CONNECT_GRACE.as_secs()
                );
                self.mns_grace_warned = true;
                self.set_live(LiveTexts::Unavailable);
            }
        }

        async fn sync(&mut self) -> Result<usize, MapError> {
            let shared = self.shared.clone();
            let backfill = !self.backfilled;
            let session = self.ensure().await?;
            if let Err(e) = session.update_inbox().await {
                log::debug!("UpdateInbox: {e}");
            }
            let mut listed = session.list("inbox", LIST_MAX, 0).await?;
            if backfill {
                let mut seen: std::collections::HashSet<String> = listed.iter().map(|m| m.handle.clone()).collect();
                while !listed.is_empty() && listed.len() < BACKFILL_MAX as usize {
                    let offset = listed.len() as u16;
                    let page = session.list("inbox", LIST_MAX, offset).await?;
                    // Stop at the end, at messages tug already has (everything older is known
                    // too), or if the phone ignores the offset and repeats itself.
                    let fresh: Vec<_> = page.into_iter().filter(|m| seen.insert(m.handle.clone())).collect();
                    let known = fresh
                        .iter()
                        .all(|m| shared.store.has_message(SOURCE_IPHONE_MAP, &m.handle).unwrap_or(false));
                    if fresh.is_empty() || known {
                        break;
                    }
                    listed.extend(fresh);
                }
                log::info!("looked back over {} inbox message(s)", listed.len());
            }
            // One line per sync showing how the phone typed this listing, so a hardware run reveals
            // whether iOS distinguishes iMessage (IM) from a plain text (SMS_*). "other" covers
            // SMS_CDMA, MMS, EMAIL and any the phone left blank.
            let mut type_counts: Option<(u32, u32, u32)> = None;
            if !listed.is_empty() {
                crate::bt_inventory::record_listing_types(listed.iter().map(|m| m.msg_type.as_str()));
                let (mut sms_gsm, mut im, mut other) = (0u32, 0u32, 0u32);
                for m in &listed {
                    match m.msg_type.as_str() {
                        "SMS_GSM" => sms_gsm += 1,
                        "IM" => im += 1,
                        _ => other += 1,
                    }
                }
                type_counts = Some((sms_gsm, im, other));
            }
            let mut added = 0;
            // Oldest first so arrival order matches the phone.
            for item in listed.iter().rev() {
                if shared.store.has_message(SOURCE_IPHONE_MAP, &item.handle)? {
                    // Read on the phone since: nothing left for tug to mark.
                    if item.read {
                        shared.store.set_read_on_phone(SOURCE_IPHONE_MAP, &item.handle)?;
                    }
                    continue;
                }
                let (originator, full_body) = match session.get_message(&item.handle).await {
                    Ok(msg) => (msg.originator_address, msg.body),
                    // The phone refused this one message (e.g. an attachment it won't serialize).
                    // Don't let it block every newer text or drop the session: keep the listing's
                    // preview if there is one, otherwise skip it.
                    Err(MapError::Obex { code, .. }) => {
                        log::info!("phone refused message {}: {code:#04x}; using its preview", item.handle);
                        if item.subject.is_empty() {
                            continue;
                        }
                        (None, String::new())
                    }
                    Err(e) => return Err(e),
                };
                let address = normalize(originator.as_deref().unwrap_or(&item.sender_addressing));
                let body = if full_body.is_empty() {
                    item.subject.clone()
                } else {
                    full_body
                };
                let sent_at = listing::datetime_to_iso(&item.datetime);
                let sender_name = Some(item.sender_name.as_str()).filter(|n| !n.is_empty() && *n != address);
                let stored = shared.store.insert_incoming(&IncomingMessage {
                    source: SOURCE_IPHONE_MAP,
                    handle: &item.handle,
                    address: &address,
                    sender_name,
                    body: &body,
                    sent_at: sent_at.as_deref(),
                    received_at: now_ms(),
                    unread_on_phone: !item.read,
                    msg_type: Some(item.msg_type.as_str()).filter(|t| !t.is_empty()),
                })?;
                if let Some(m) = stored {
                    shared.emit(events::MESSAGE, m);
                    added += 1;
                }
            }
            if added > 0 {
                let named = !shared.store.learn_contacts()?.is_empty();
                let aliased = shared.store.learn_aliases()? > 0;
                if named || aliased {
                    shared.emit(events::CONTACTS, shared.store.contacts()?);
                }
            }
            // Only once it all went through: a sync that failed partway looks back again.
            self.backfilled = true;
            // Once per change, not every poll (logged here, after the session borrow ends).
            if let Some(counts @ (sms_gsm, im, other)) = type_counts {
                if self.last_type_counts != Some(counts) {
                    self.last_type_counts = Some(counts);
                    log::info!("message types: SMS_GSM={sms_gsm}, IM={im}, other={other}");
                }
            }
            Ok(added)
        }

        /// Mark messages read on the phone. Only those it still lists as unread are
        /// sent, so reopening a conversation costs nothing.
        async fn mark_read(&mut self, ids: &[i64]) {
            let handles = match self.shared.store.unread_on_phone(SOURCE_IPHONE_MAP, ids) {
                Ok(h) if !h.is_empty() => h,
                Ok(_) => return,
                Err(e) => return log::warn!("reading unread messages failed: {e}"),
            };
            let store = self.shared.store.clone();
            let session = match self.ensure().await {
                Ok(s) => s,
                Err(e) => return log::debug!("mark read: {e}"),
            };
            for handle in &handles {
                match session.set_read(handle, true).await {
                    Ok(()) => {
                        if let Err(e) = store.set_read_on_phone(SOURCE_IPHONE_MAP, handle) {
                            log::warn!("saving read state failed: {e}");
                        }
                    }
                    // Gone from the phone (deleted there): nothing left to mark, stop trying.
                    Err(MapError::Obex {
                        code: RSP_NOT_FOUND, ..
                    }) => {
                        if let Err(e) = store.set_read_on_phone(SOURCE_IPHONE_MAP, handle) {
                            log::warn!("saving read state failed: {e}");
                        }
                    }
                    // Refused for another reason; try again next time it's opened.
                    Err(MapError::Obex { code, .. }) => log::info!("phone refused mark-read for {handle}: {code:#04x}"),
                    Err(e) => {
                        log::debug!("mark read failed: {e}");
                        return self.fail(&e);
                    }
                }
            }
            log::info!("marked {} message(s) read on the iPhone", handles.len());
        }

        async fn send(&mut self, address: &str, text: &str) -> Result<StoredMessage, String> {
            let text = text.trim();
            if text.is_empty() {
                return Err("Type a message first".into());
            }
            let address = normalize(address);
            let store = self.shared.store.clone();
            let pending = store
                .insert_outgoing(SOURCE_IPHONE_MAP, &address, text, now_ms())
                .map_err(|e| e.to_string())?;
            self.shared.emit(events::MESSAGE, pending.clone());

            let result = match self.ensure().await {
                Ok(session) => session.push_message(&address, text).await,
                Err(e) => Err(e),
            };
            let (status, handle, error) = match result {
                Ok(handle) => (Status::Accepted, handle, None),
                Err(e) => {
                    log::warn!("send failed: {e}");
                    let message = e.to_string();
                    self.fail(&e);
                    (Status::Failed, None, Some(message))
                }
            };
            let updated = store
                .set_outgoing_status(pending.id, status, handle.as_deref())
                .map_err(|e| e.to_string())?;
            self.shared.emit(events::MESSAGE, updated.clone());
            match error {
                Some(e) => Err(e),
                None => Ok(updated),
            }
        }
    }

    /// Whether the contact-photo pass is due: never pulled before, or the last successful pull was
    /// at least `interval_ms` ago. A last-sync time in the future (the clock moved back) also
    /// counts as due, so a bad clock can't park the photos forever. Pure, so it's unit-tested.
    fn photo_sync_due(last_sync_ms: Option<i64>, now_ms: i64, interval_ms: i64) -> bool {
        match last_sync_ms {
            None => true,
            Some(last) => now_ms < last || now_ms - last >= interval_ms,
        }
    }

    /// The background contact-photo pass: pull the phonebook WITH PHOTO over its own PBAP link,
    /// store the faces, update only the photo references, then refresh the UI. Bounded by a
    /// timeout; if the phone has gone the pull just fails and this returns, and the worker aborts
    /// it on disconnect either way. Only a successful pull records the sync time and runs the
    /// orphan-file cleanup, so a failed or switched-off pull never clears photos tug already has.
    async fn sync_contact_photos(shared: Arc<Shared>, device_id: String) {
        let pulled = tokio::time::timeout(PHOTO_PULL_TIMEOUT, pull_contacts(&device_id, true))
            .await
            .unwrap_or(Err(MapError::Timeout));
        let entries = match pulled {
            Ok(entries) => entries,
            Err(e) => return log::info!("contact photos sync failed: {e}"),
        };
        // Empty means Sync Contacts went off between the fast sync and now: leave photos as they are.
        if entries.is_empty() {
            log::debug!("the iPhone shared no contacts for the photo pass; keeping existing photos");
            return;
        }
        use tauri::Manager as _;
        let Ok(dir) = shared.app.path().app_data_dir() else {
            return log::warn!("no app data dir; can't store contact photos");
        };
        let mut photos = 0usize;
        let mut rows: Vec<(String, Option<String>)> = Vec::new();
        for e in &entries {
            // Validate and write the inlined face to its own file; the row keeps only the reference.
            let key = e
                .photo
                .as_ref()
                .and_then(|bytes| crate::contact_photos::store_photo(&dir, bytes));
            if key.is_some() {
                photos += 1;
            }
            for n in &e.numbers {
                rows.push((normalize(n), key.clone()));
            }
        }
        match shared.store.update_contact_photos(&rows) {
            Ok(()) => {
                log::info!("contact photos synced: {photos} photos");
                if let Err(e) = shared.store.set_setting(keys::LAST_PHOTO_SYNC, &now_ms().to_string()) {
                    log::warn!("recording the photo-sync time failed: {e}");
                }
                // Only after a successful WITH-PHOTO pull: drop files no contact points at any more.
                if let Ok(keep) = shared.store.photo_keys() {
                    crate::contact_photos::cleanup(&dir, &keep);
                }
                if let Ok(all) = shared.store.contacts() {
                    shared.emit(events::CONTACTS, all);
                }
            }
            Err(e) => log::warn!("saving contact photos failed: {e}"),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{map_retry_delay, photo_sync_due, MAP_RETRY_CAP};
        use std::time::Duration;

        #[test]
        fn map_retry_backs_off_to_minutes_then_caps() {
            // The overnight "isn't offering this service" loop: climb off 30 s instead of retrying
            // every 30 s forever, capped at a few minutes.
            let secs: Vec<u64> = (1..=8).map(|f| map_retry_delay(f).as_secs()).collect();
            assert_eq!(secs, vec![30, 60, 120, 240, 300, 300, 300, 300]);
            assert!(
                (1..=1000).all(|f| map_retry_delay(f) <= MAP_RETRY_CAP),
                "never over the cap"
            );
            assert_eq!(map_retry_delay(0), Duration::from_secs(30), "no underflow at zero");
        }

        #[test]
        fn photo_sync_due_is_daily_and_survives_a_clock_jump() {
            let day = 24 * 60 * 60 * 1000;
            assert!(photo_sync_due(None, 1_000, day), "never synced: due");
            assert!(
                !photo_sync_due(Some(1_000), 1_000 + day - 1, day),
                "less than a day on: not yet"
            );
            assert!(photo_sync_due(Some(1_000), 1_000 + day, day), "a day on: due");
            assert!(
                photo_sync_due(Some(5_000), 1_000, day),
                "clock moved back (last-sync in the future): due"
            );
        }
    }
}
