//! Background message service: keeps a MAP session with the iPhone, pulls new
//! inbox messages into the store and sends replies.
//!
//! The inbox is polled (MAP only exposes a small recent window, and iOS posts
//! no notification for a conversation that's open on the phone), and a
//! Messages notification from ANCS triggers an immediate refresh.

use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use crate::messages::StoredMessage;
use crate::state::Shared;

pub enum MapCommand {
    Refresh,
    Send {
        address: String,
        text: String,
        reply: oneshot::Sender<Result<StoredMessage, String>>,
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

    pub async fn send(&self, address: String, text: String) -> Result<StoredMessage, String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(MapCommand::Send { address, text, reply })
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
                if let MapCommand::Send { reply, .. } = cmd {
                    let _ = reply.send(Err("Messaging is only supported on Windows".into()));
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
    use crate::ancs::ancs_date_to_iso;
    use crate::map::address::normalize;
    use crate::map::session::{find_devices, pull_contacts, MapError, MapSession};
    use crate::messages::{IncomingMessage, Status, StoredMessage, SOURCE_IPHONE_MAP};
    use crate::state::{events, Shared};

    const FIRST_SYNC_DELAY: Duration = Duration::from_secs(3);
    const POLL_CONNECTED: Duration = Duration::from_secs(8);
    const RETRY_DISCONNECTED: Duration = Duration::from_secs(30);
    /// How many of the newest inbox messages to look at each poll.
    const LIST_MAX: u16 = 20;
    const CONTACTS_RESYNC: Duration = Duration::from_secs(6 * 60 * 60);
    const CONTACTS_RETRY: Duration = Duration::from_secs(10 * 60);

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
    }

    pub async fn run(shared: Arc<Shared>, mut commands: UnboundedReceiver<MapCommand>) {
        let mut w = Worker {
            shared,
            session: None,
            device_id: None,
            next_contacts_sync: Instant::now(),
        };
        let mut next = Instant::now() + FIRST_SYNC_DELAY;
        loop {
            tokio::select! {
                cmd = commands.recv() => match cmd {
                    None => break,
                    Some(MapCommand::Refresh) => w.refresh().await,
                    Some(MapCommand::Send { address, text, reply }) => {
                        let _ = reply.send(w.send(&address, &text).await);
                    }
                },
                _ = tokio::time::sleep_until(next) => w.refresh().await,
            }
            if Instant::now() >= next {
                next = Instant::now()
                    + if w.session.is_some() {
                        POLL_CONNECTED
                    } else {
                        RETRY_DISCONNECTED
                    };
            }
        }
    }

    impl Worker {
        fn set_state(&self, connected: bool, error: Option<String>) {
            self.shared.update_status(|s| {
                s.services.messages = connected;
                s.messages_error = error;
            });
        }

        async fn ensure(&mut self) -> Result<&mut MapSession, MapError> {
            if self.session.is_none() {
                let devices = find_devices().await?;
                // Prefer the phone tug is paired with; the Classic and LE names usually match.
                let wanted = self.shared.status().device.map(|d| d.name);
                let device = devices
                    .iter()
                    .find(|d| wanted.as_deref() == Some(d.name.as_str()))
                    .or(devices.first())
                    .ok_or(MapError::NoDevice)?;
                let session = MapSession::connect(&device.id).await?;
                log::info!("message access connected to {}", device.name);
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

        /// Pull the phone's contacts (PBAP) now and then, for names and new chats.
        async fn sync_contacts_if_due(&mut self) {
            let Some(device_id) = self.device_id.clone() else {
                return;
            };
            if Instant::now() < self.next_contacts_sync {
                return;
            }
            match pull_contacts(&device_id).await {
                Ok(entries) => {
                    let pairs: Vec<(String, String)> = entries
                        .iter()
                        .flat_map(|e| e.numbers.iter().map(move |n| (normalize(n), e.name.clone())))
                        .collect();
                    match self.shared.store.save_phonebook(&pairs) {
                        Ok(n) => {
                            log::info!("contacts synced: {} people, {n} numbers", entries.len());
                            if let Ok(all) = self.shared.store.contacts() {
                                self.shared.emit(events::CONTACTS, all);
                            }
                        }
                        Err(e) => log::warn!("saving contacts failed: {e}"),
                    }
                    self.shared.update_status(|s| s.contacts_error = None);
                    self.next_contacts_sync = Instant::now() + CONTACTS_RESYNC;
                }
                Err(e) => {
                    log::info!("contacts sync failed: {e}");
                    let shown = matches!(e, MapError::ContactsConsent).then(|| e.to_string());
                    self.shared.update_status(|s| s.contacts_error = shown);
                    self.next_contacts_sync = Instant::now() + CONTACTS_RETRY;
                }
            }
        }

        async fn refresh(&mut self) {
            let result = self.sync().await;
            if result.is_ok() {
                self.sync_contacts_if_due().await;
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
            let session = self.ensure().await?;
            if let Err(e) = session.update_inbox().await {
                log::debug!("UpdateInbox: {e}");
            }
            let listed = session.list("inbox", LIST_MAX).await?;
            let mut added = 0;
            // Oldest first so arrival order matches the phone.
            for item in listed.iter().rev() {
                if shared.store.has_message(SOURCE_IPHONE_MAP, &item.handle)? {
                    continue;
                }
                let msg = session.get_message(&item.handle).await?;
                let address = normalize(msg.originator_address.as_deref().unwrap_or(&item.sender_addressing));
                let body = if msg.body.is_empty() {
                    item.subject.clone()
                } else {
                    msg.body
                };
                let sent_at = ancs_date_to_iso(&item.datetime);
                let sender_name = Some(item.sender_name.as_str()).filter(|n| !n.is_empty() && *n != address);
                let stored = shared.store.insert_incoming(&IncomingMessage {
                    source: SOURCE_IPHONE_MAP,
                    handle: &item.handle,
                    address: &address,
                    sender_name,
                    body: &body,
                    sent_at: sent_at.as_deref(),
                    received_at: now_ms(),
                })?;
                if let Some(m) = stored {
                    shared.emit(events::MESSAGE, m);
                    added += 1;
                }
            }
            if added > 0 && !shared.store.learn_contacts()?.is_empty() {
                shared.emit(events::CONTACTS, shared.store.contacts()?);
            }
            Ok(added)
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
