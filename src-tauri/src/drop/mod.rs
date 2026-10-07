//! tug Drop: move photos, files and text between the phone and this PC over the local Wi-Fi,
//! with nothing installed on the phone. Drop's panel shows a QR code for
//! `http://<LAN address>:<port>/#<secret>`; the phone's camera opens tug's small page in Safari,
//! which seals everything with keys derived from the secret (see `crypto.rs`).
//!
//! Lifetime: the server runs only while Drop is open, on the one address the phone can reach
//! (`net.rs`). It stops when the panel closes, when tug quits, or after 10 minutes without a
//! request; the secret is useless after that. Unfinished uploads are removed on start and stop.
//! tug never touches Windows Firewall: Windows asks the user the first time on its own.
//!
//! The pure parts (keys and sealing, auth, file names, adapter ranking, chunk bookkeeping) are
//! unit-tested in their modules; `tests.rs` runs a whole session over a real socket.

pub mod auth;
pub mod crypto;
mod names;
pub mod net;
mod page;
pub mod qr;
pub mod server;
pub mod session;
#[cfg(test)]
mod tests;
mod upload;

use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use qr::Qr;
use session::{DropIncoming, DropOffer, DropText, Session, Sink, Skipped};

/// Event carrying a fresh `DropStatus` whenever anything in the panel changes.
pub const EVENT: &str = "drop-status";
/// Drop turns itself off after this long without a request from the phone (or a panel action).
pub const IDLE_LIMIT: Duration = Duration::from_secs(10 * 60);
const TICK: Duration = Duration::from_secs(2);
/// Look for a network change every this many ticks.
const NET_EVERY: u32 = 3;
/// Progress events are coalesced to at most one per this long.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Off,
    /// Showing the QR code; no phone yet.
    Waiting,
    /// A phone has connected to this session.
    Connected,
    /// Open, but there's no network a phone could reach this PC on.
    NoNetwork,
}

/// Why Drop stopped by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Ended {
    Idle,
}

/// Everything the Drop panel shows. Mirrored in `src/types/protocol.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DropStatus {
    pub phase: Phase,
    /// The full link in the QR code (with the secret), also shown as the manual fallback.
    pub url: Option<String>,
    /// `192.168.1.20:53211`.
    pub address: Option<String>,
    pub qr: Option<Qr>,
    /// "iPhone", once one has connected.
    pub phone: Option<String>,
    /// The phone's page is open and checking in.
    pub phone_active: bool,
    /// Where received files go (`Pictures\tug Drop`).
    pub folder: Option<String>,
    pub incoming: Vec<DropIncoming>,
    pub outgoing: Vec<DropOffer>,
    pub texts: Vec<DropText>,
    /// Text currently offered to the phone.
    pub sent_text: Option<String>,
    pub ended: Option<Ended>,
}

struct Endpoint {
    ip: Ipv4Addr,
    port: u16,
    url: String,
    qr: Option<Qr>,
    shutdown: Option<oneshot::Sender<()>>,
    task: tauri::async_runtime::JoinHandle<()>,
}

impl Endpoint {
    /// Stop accepting; give open requests a moment, then cut them off (the session is already
    /// closed, so anything still running only gets "closed" back).
    fn close(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let task = self.task;
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            task.abort();
        });
    }
}

struct Running {
    generation: u64,
    secret: [u8; crypto::SECRET_LEN],
    session: Arc<Session>,
    /// `None` while there's no network to listen on.
    endpoint: Option<Endpoint>,
}

struct Inner {
    app: AppHandle,
    running: Mutex<Option<Running>>,
    ended: Mutex<Option<Ended>>,
    generation: AtomicU64,
    emit_pending: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Owns the one Drop session there can be at a time.
#[derive(Clone)]
pub struct DropService {
    inner: Arc<Inner>,
}

/// The session's link to the panel: status events and the clipboard.
struct PanelSink(Weak<Inner>);

impl Sink for PanelSink {
    fn changed(&self, urgent: bool) {
        let Some(inner) = self.0.upgrade() else { return };
        let svc = DropService { inner };
        if urgent {
            svc.emit();
        } else if !svc.inner.emit_pending.swap(true, Ordering::SeqCst) {
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(PROGRESS_EVERY).await;
                svc.inner.emit_pending.store(false, Ordering::SeqCst);
                svc.emit();
            });
        }
    }

    fn text(&self, text: &str) {
        let Some(inner) = self.0.upgrade() else { return };
        let text = text.to_string();
        // The WinRT clipboard needs the main (STA) thread.
        let _ = inner.app.run_on_main_thread(move || {
            if let Err(e) = crate::clipboard::set_text(&text) {
                log::warn!("drop: couldn't put the phone's text on the clipboard: {e}");
            }
        });
    }
}

/// Unfinished uploads, under tug's local app data.
fn incoming_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|p| p.join("drop-incoming"))
        .map_err(|_| "Couldn't find tug's data folder.".to_string())
}

/// `Pictures\tug Drop`, from Windows' Pictures known folder (wherever it's been moved to).
fn drop_folder(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .picture_dir()
        .map(|p| p.join("tug Drop"))
        .map_err(|_| "Couldn't find your Pictures folder.".to_string())
}

async fn open_endpoint(
    ip: Ipv4Addr,
    port_hint: u16,
    secret: &[u8],
    session: Arc<Session>,
) -> std::io::Result<Endpoint> {
    // Keep the same port across a network change when we can; otherwise any free high port.
    let listener = match TcpListener::bind((ip, port_hint)).await {
        Ok(l) => l,
        Err(_) if port_hint != 0 => TcpListener::bind((ip, 0)).await?,
        Err(e) => return Err(e),
    };
    let port = listener.local_addr()?.port();
    let url = format!("http://{ip}:{port}/#{}", crypto::b64(secret));
    let qr = qr::encode(&url);
    let (tx, rx) = oneshot::channel::<()>();
    let task = tauri::async_runtime::spawn(server::serve(listener, session, async {
        let _ = rx.await;
    }));
    log::info!("drop: listening on port {port}");
    Ok(Endpoint {
        ip,
        port,
        url,
        qr,
        shutdown: Some(tx),
        task,
    })
}

impl DropService {
    pub fn new(app: AppHandle) -> DropService {
        DropService {
            inner: Arc::new(Inner {
                app,
                running: Mutex::new(None),
                ended: Mutex::new(None),
                generation: AtomicU64::new(0),
                emit_pending: AtomicBool::new(false),
            }),
        }
    }

    fn emit(&self) {
        if let Err(e) = self.inner.app.emit(EVENT, self.status()) {
            log::warn!("emit {EVENT} failed: {e}");
        }
    }

    fn session(&self) -> Option<Arc<Session>> {
        lock(&self.inner.running).as_ref().map(|r| r.session.clone())
    }

    pub fn status(&self) -> DropStatus {
        let folder = drop_folder(&self.inner.app)
            .ok()
            .map(|p| p.to_string_lossy().into_owned());
        let ended = *lock(&self.inner.ended);
        let (session, endpoint) = {
            let run = lock(&self.inner.running);
            match run.as_ref() {
                None => (None, None),
                Some(r) => (
                    Some(r.session.clone()),
                    r.endpoint
                        .as_ref()
                        .map(|e| (e.url.clone(), format!("{}:{}", e.ip, e.port), e.qr.clone())),
                ),
            }
        };
        let Some(session) = session else {
            return DropStatus {
                phase: Phase::Off,
                url: None,
                address: None,
                qr: None,
                phone: None,
                phone_active: false,
                folder,
                incoming: Vec::new(),
                outgoing: Vec::new(),
                texts: Vec::new(),
                sent_text: None,
                ended,
            };
        };
        let snap = session.snapshot();
        let phase = match (&endpoint, &snap.phone) {
            (None, _) => Phase::NoNetwork,
            (Some(_), Some(_)) => Phase::Connected,
            (Some(_), None) => Phase::Waiting,
        };
        let (url, address, qr) = match endpoint {
            Some((u, a, q)) => (Some(u), Some(a), q),
            None => (None, None, None),
        };
        DropStatus {
            phase,
            url,
            address,
            qr,
            phone: snap.phone,
            phone_active: snap.phone_active,
            folder,
            incoming: snap.incoming,
            outgoing: snap.outgoing,
            texts: snap.texts,
            sent_text: snap.sent_text,
            ended: None,
        }
    }

    /// Open Drop (or return the session already open): a new secret, a fresh port.
    pub async fn start(&self) -> Result<DropStatus, String> {
        if self.session().is_some() {
            return Ok(self.status());
        }
        let folder = drop_folder(&self.inner.app)?;
        let incoming = incoming_dir(&self.inner.app)?;
        {
            let incoming = incoming.clone();
            let _ = tokio::task::spawn_blocking(move || session::clean_incoming(&incoming)).await;
        }
        let secret = crypto::new_secret();
        let sink: Arc<dyn Sink> = Arc::new(PanelSink(Arc::downgrade(&self.inner)));
        let session = Arc::new(Session::new(&secret, folder, incoming, sink));
        let ip = tokio::task::spawn_blocking(net::current).await.ok().flatten();
        let endpoint = match ip {
            Some(ip) => match open_endpoint(ip, 0, &secret, session.clone()).await {
                Ok(e) => Some(e),
                Err(e) => {
                    log::warn!("drop: couldn't listen: {e}");
                    return Err("Couldn't start Drop on this network.".into());
                }
            },
            None => None,
        };
        let generation = self.inner.generation.fetch_add(1, Ordering::SeqCst) + 1;
        {
            let mut run = lock(&self.inner.running);
            if run.is_some() {
                // Another start won the race: keep it, drop ours.
                session.close();
                if let Some(e) = endpoint {
                    e.close();
                }
                drop(run);
                return Ok(self.status());
            }
            *run = Some(Running {
                generation,
                secret,
                session,
                endpoint,
            });
        }
        *lock(&self.inner.ended) = None;
        let svc = self.clone();
        tauri::async_runtime::spawn(async move { svc.supervise(generation).await });
        self.emit();
        Ok(self.status())
    }

    /// Close Drop: stop listening, invalidate the secret, remove unfinished uploads.
    pub async fn stop(&self, ended: Option<Ended>) {
        let running = lock(&self.inner.running).take();
        if let Some(r) = running {
            r.session.close();
            if let Some(e) = r.endpoint {
                e.close();
            }
            let incoming = r.session.incoming.clone();
            let _ = tokio::task::spawn_blocking(move || session::clean_incoming(&incoming)).await;
            log::info!("drop: closed{}", if ended.is_some() { " (idle)" } else { "" });
        }
        *lock(&self.inner.ended) = ended;
        self.emit();
    }

    /// tug is quitting: stop at once, from a sync context.
    pub fn shutdown_now(&self) {
        if let Some(r) = lock(&self.inner.running).take() {
            r.session.close();
            if let Some(mut e) = r.endpoint {
                if let Some(tx) = e.shutdown.take() {
                    let _ = tx.send(());
                }
                e.task.abort();
            }
            session::clean_incoming(&r.session.incoming);
        }
    }

    /// Watch the open session: idle timeout, the phone coming and going, network changes.
    async fn supervise(&self, generation: u64) {
        let mut ticks = 0u32;
        let mut was_active = false;
        loop {
            tokio::time::sleep(TICK).await;
            let (session, ip, port) = {
                let run = lock(&self.inner.running);
                match run.as_ref() {
                    Some(r) if r.generation == generation => (
                        r.session.clone(),
                        r.endpoint.as_ref().map(|e| e.ip),
                        r.endpoint.as_ref().map_or(0, |e| e.port),
                    ),
                    _ => return,
                }
            };
            if session.idle_for() >= IDLE_LIMIT {
                self.stop(Some(Ended::Idle)).await;
                return;
            }
            let active = session.phone_active();
            if active != was_active {
                was_active = active;
                self.emit();
            }
            ticks += 1;
            if ticks.is_multiple_of(NET_EVERY) || ip.is_none() {
                let now = tokio::task::spawn_blocking(net::current).await.ok().flatten();
                if now != ip {
                    log::info!("drop: network changed");
                    self.rebind(generation, now, port).await;
                }
            }
        }
    }

    /// Move the session to a new address (the QR code changes; the session and secret don't).
    async fn rebind(&self, generation: u64, ip: Option<Ipv4Addr>, port_hint: u16) {
        let (secret, session, old) = {
            let mut run = lock(&self.inner.running);
            match run.as_mut() {
                Some(r) if r.generation == generation => (r.secret, r.session.clone(), r.endpoint.take()),
                _ => return,
            }
        };
        if let Some(e) = old {
            e.close();
        }
        let endpoint = match ip {
            Some(ip) => open_endpoint(ip, port_hint, &secret, session).await.ok(),
            None => None,
        };
        {
            let mut run = lock(&self.inner.running);
            match run.as_mut() {
                Some(r) if r.generation == generation => r.endpoint = endpoint,
                _ => {
                    if let Some(e) = endpoint {
                        e.close();
                    }
                    return;
                }
            }
        }
        self.emit();
    }

    /// Offer files to the phone, opening Drop first if it's closed (files dragged onto tug).
    pub async fn offer(&self, paths: Vec<PathBuf>) -> Result<Vec<Skipped>, String> {
        if self.session().is_none() {
            self.start().await?;
        }
        let session = self.session().ok_or("Drop isn't open.")?;
        Ok(tokio::task::spawn_blocking(move || session.offer(&paths))
            .await
            .unwrap_or_default())
    }

    pub fn remove_offer(&self, id: &str) {
        if let Some(s) = self.session() {
            s.remove_offer(id);
        }
    }

    pub fn send_text(&self, text: &str) -> Result<(), String> {
        self.session().ok_or("Drop isn't open.")?.set_pc_text(text)
    }

    /// Open the Drop folder in Explorer, or select one file this session saved.
    pub fn open_folder(&self, file: Option<&str>) -> Result<(), String> {
        let folder = drop_folder(&self.inner.app)?;
        std::fs::create_dir_all(&folder).map_err(|_| "Couldn't open the tug Drop folder.".to_string())?;
        let mut cmd = std::process::Command::new("explorer.exe");
        match file.map(Path::new) {
            Some(p) if self.session().is_some_and(|s| s.saved_path(p)) && p.exists() => {
                // Explorer wants `/select,"C:\path with spaces\file"` verbatim, not Rust's quoting
                // of the whole argument. The path is one this session saved (checked above).
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    cmd.raw_arg(format!("/select,\"{}\"", p.display()));
                }
                #[cfg(not(windows))]
                cmd.arg(p);
            }
            _ => {
                cmd.arg(&folder);
            }
        }
        cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
    }
}
