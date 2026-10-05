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

    use tokio::sync::mpsc::UnboundedReceiver;
    use tokio::time::Instant;

    use super::MapCommand;
    use crate::map::address::normalize;
    use crate::map::health::{Attempt, Health, TextsPairing};
    use crate::map::listing;
    use crate::map::obex::RSP_NOT_FOUND;
    use crate::map::pick::choose_device;
    use crate::map::session::{find_devices, pull_call_history, pull_contacts, MapDevice, MapError, MapSession};
    use crate::messages::{IncomingMessage, Status, StoredMessage, SOURCE_IPHONE_MAP};
    use crate::state::{events, keys, ConnectionState, Shared};

    const FIRST_SYNC_DELAY: Duration = Duration::from_secs(3);
    const POLL_CONNECTED: Duration = Duration::from_secs(8);
    const RETRY_DISCONNECTED: Duration = Duration::from_secs(30);
    /// While the user watches the iPhone's switches, Show Message Notifications and Sync
    /// Contacts are checked this often, so flipping one shows up in tug right away.
    const WATCHING: Duration = Duration::from_secs(2);
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
        backfilled: bool,
        health: Health,
        /// The phone Windows has paired for texts, found even when connecting to it fails.
        texts_device: Option<String>,
        next_calls_sync: Instant,
        last_calls_pull: Option<Instant>,
    }

    pub async fn run(shared: Arc<Shared>, mut commands: UnboundedReceiver<MapCommand>) {
        let mut w = Worker {
            shared,
            session: None,
            device_id: None,
            next_contacts_sync: Instant::now(),
            unshared_contact_pulls: 0,
            backfilled: false,
            health: Health::default(),
            texts_device: None,
            next_calls_sync: Instant::now(),
            last_calls_pull: None,
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
                },
                _ = tokio::time::sleep_until(next) => w.refresh().await,
            }
            if Instant::now() >= next {
                next = Instant::now()
                    + if w.shared.watching() {
                        WATCHING
                    } else if w.session.is_some() {
                        POLL_CONNECTED
                    } else {
                        RETRY_DISCONNECTED
                    };
            }
        }
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

        /// The paired Classic device that is this iPhone.
        async fn pick_device(&self) -> Result<MapDevice, MapError> {
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
                self.device_id = Some(device.id.clone());
                self.session = Some(session);
                self.set_state(true, None);
            }
            Ok(self.session.as_mut().expect("session just set"))
        }

        fn fail(&mut self, e: &MapError) {
            if self.session.take().is_some() {
                log::info!("message access dropped: {e}");
            }
            let shown = match e {
                MapError::Consent | MapError::NoService => Some(e.to_string()),
                _ => None,
            };
            self.set_state(false, shown);
        }

        /// When to ask again while contacts aren't shared yet: soon at first, then the usual retry.
        fn soon(&mut self) -> Duration {
            if self.shared.watching() {
                return WATCHING;
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
            // Bounded as a whole: the pull runs on the same worker as sending and mark-read.
            let pulled = tokio::time::timeout(CONTACTS_PULL_TIMEOUT, pull_contacts(&device_id))
                .await
                .unwrap_or(Err(MapError::Timeout));
            match pulled {
                // The iPhone answers with an empty list, not a refusal, while Sync Contacts is
                // off. Keep any names already saved and ask again rather than in 6 hours.
                Ok(entries) if entries.is_empty() => {
                    log::info!("the iPhone shared no contacts (Sync Contacts off?), asking again soon");
                    self.next_contacts_sync = Instant::now() + self.soon();
                }
                Ok(entries) => {
                    self.unshared_contact_pulls = 0;
                    let pairs: Vec<(String, String)> = entries
                        .iter()
                        .flat_map(|e| e.numbers.iter().map(move |n| (normalize(n), e.name.clone())))
                        .collect();
                    match self.shared.store.save_phonebook(&pairs) {
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
                }
                Err(e) => {
                    log::info!("contacts sync failed: {e}");
                    let consent = matches!(e, MapError::ContactsConsent);
                    let shown = consent.then(|| e.to_string());
                    self.shared.update_status(|s| s.contacts_error = shown);
                    // A refusal is the switch still being off, like an empty list: during setup
                    // it's usually flipped seconds later, so ask again soon.
                    self.next_contacts_sync = Instant::now() + if consent { self.soon() } else { CONTACTS_RETRY };
                }
            }
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
            self.last_calls_pull = Some(Instant::now());
            self.next_calls_sync = Instant::now() + CALLS_RESYNC;
            let pulled = tokio::time::timeout(CALLS_PULL_TIMEOUT, pull_call_history(&device_id, CALLS_MAX))
                .await
                .unwrap_or(Err(MapError::Timeout));
            match pulled {
                // Like the phonebook, an empty answer is how Sync Contacts being off looks, so
                // it never wipes a list tug already has. (A refusal shows as the contacts error.)
                Ok(calls) if calls.is_empty() && !self.shared.calls().is_empty() => {
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
            let result = self.sync().await;
            self.record_health(&result);
            if result.is_ok() {
                self.sync_contacts_if_due().await;
                self.sync_calls_if_due().await;
            }
            match result {
                Ok(0) => {}
                Ok(n) => log::info!("{n} new message(s) from the iPhone"),
                Err(MapError::NoDevice) => self.set_state(false, None),
                Err(e) => {
                    log::debug!("message sync failed: {e}");
                    self.fail(&e);
                }
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
}
