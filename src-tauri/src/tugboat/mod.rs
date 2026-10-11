//! Tugboat: move photos, files and text between the phone and this PC over the local Wi-Fi,
//! with nothing installed on the phone. Tugboat's panel shows a QR code for
//! `http://<LAN address>:<port>/#<secret>`; the phone's camera opens tug's small page in Safari,
//! which seals everything with keys derived from the secret (see `crypto.rs`).
//!
//! Lifetime: the server runs only while Tugboat is open, on the one address the phone can reach
//! (`net.rs`). It stops when the panel closes, when tug quits, when tug's window hides to the tray
//! (once any transfer in progress finishes), or after 10 minutes without a request; the secret is
//! useless after that. Unfinished uploads are removed on start and stop. Tugboat Run can also start
//! it, to use the phone as a game controller (`pad.rs`); the same rules then apply.
//! tug never touches Windows Firewall: Windows asks the user the first time on its own (an account
//! that isn't an administrator may need one to answer). It needs an IPv4 home or office network
//! the phone is also on; on a network Windows calls Public, inbound connections are blocked.
//!
//! The pure parts (keys and sealing, auth, file names, adapter ranking, chunk bookkeeping) are
//! unit-tested in their modules; `tests.rs` runs a whole session over a real socket.

pub mod auth;
pub mod crypto;
mod names;
pub mod net;
pub mod pad;
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
use session::{Session, Sink, Skipped, TugboatIncoming, TugboatOffer, TugboatText};

/// Event carrying a fresh `TugboatStatus` whenever anything in the panel changes.
pub const EVENT: &str = "tugboat-status";
/// Text from the phone went (or failed to go) onto the clipboard: `{ ok: bool }`.
pub const TEXT_EVENT: &str = "tugboat-text";
/// Files were dropped onto tug's window and offered: the ones skipped, so the panel can say why.
pub const DROPPED_EVENT: &str = "tugboat-dropped";
/// Tugboat turns itself off after this long without a request from the phone (or a panel action).
pub const IDLE_LIMIT: Duration = Duration::from_secs(10 * 60);
const TICK: Duration = Duration::from_secs(2);
/// Look for a network change every this many ticks.
const NET_EVERY: u32 = 3;
/// Progress events are coalesced to at most one per this long.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);
/// The game controller changed: `pad::PadEvent`. Only Tugboat Run listens.
pub const PAD_EVENT: &str = "game-pad";
/// How often an open controller channel looks for a phone that went quiet. The watch exists only
/// while the game has the channel open.
const PAD_TICK: Duration = Duration::from_millis(150);

/// Whether this Windows account is an administrator (elevated or not). The panel's help says the
/// firewall prompt may need an administrator when it isn't. Errs towards true (no extra line).
#[cfg(windows)]
pub fn user_is_admin() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevationType, TokenElevationTypeFull, TokenElevationTypeLimited,
        TOKEN_ELEVATION_TYPE, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::Win32::UI::Shell::IsUserAnAdmin;
    // SAFETY: plain FFI on this process's own token, closed before returning.
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return true;
        }
        let mut kind = TOKEN_ELEVATION_TYPE::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevationType,
            Some(&mut kind as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION_TYPE>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        // Limited: an administrator running without elevation (UAC's split token). Full: elevated.
        // Default: no split token, so either a standard user or UAC is off; ask directly.
        if ok && (kind == TokenElevationTypeLimited || kind == TokenElevationTypeFull) {
            return true;
        }
        IsUserAnAdmin().as_bool()
    }
}

#[cfg(not(windows))]
pub fn user_is_admin() -> bool {
    true
}

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

/// Why Tugboat stopped by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Ended {
    Idle,
    /// tug's window was hidden to the tray.
    Hidden,
}

/// Everything the Tugboat panel shows. Mirrored in `src/types/protocol.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TugboatStatus {
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
    /// Where received files go (`Pictures\Tugboat`).
    pub folder: Option<String>,
    pub incoming: Vec<TugboatIncoming>,
    pub outgoing: Vec<TugboatOffer>,
    pub texts: Vec<TugboatText>,
    /// Text currently offered to the phone.
    pub sent_text: Option<String>,
    /// The phone is downloading a file from the PC right now.
    pub sending: bool,
    /// A file from the phone is arriving right now (not merely unfinished: the phone may have
    /// given up on it).
    pub receiving: bool,
    pub ended: Option<Ended>,
}

#[derive(Debug, Clone, Serialize)]
struct TextArrived {
    ok: bool,
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
    /// Stop accepting and end every open connection: the serve loop aborts them all when told to
    /// stop. The task is aborted too a moment later, in case it was busy. (On close the session is
    /// marked closed first, so anything that slips through only gets "closed" back.)
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
    /// The last port listened on, reused after a network blip so the link changes as little as
    /// possible.
    port: u16,
    /// The address last shown in the QR code. When it changes the session is unbound (a new
    /// address is a new browser origin); a blip back to the same address isn't a change.
    advertised: Option<(Ipv4Addr, u16)>,
    /// tug's window was hidden while files were moving: stop as soon as they're done.
    stop_when_quiet: bool,
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

/// Owns the one Tugboat session there can be at a time.
#[derive(Clone)]
pub struct TugboatService {
    inner: Arc<Inner>,
}

/// The session's link to the panel: status events and the clipboard.
struct PanelSink(Weak<Inner>);

impl Sink for PanelSink {
    fn changed(&self, urgent: bool) {
        let Some(inner) = self.0.upgrade() else { return };
        let svc = TugboatService { inner };
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
        let app = inner.app.clone();
        // The WinRT clipboard needs the main (STA) thread. The panel says what happened only once
        // it really did.
        let _ = inner.app.run_on_main_thread(move || {
            let ok = match crate::clipboard::set_text(&text) {
                Ok(()) => true,
                Err(e) => {
                    log::warn!("tugboat: couldn't put the phone's text on the clipboard: {e}");
                    false
                }
            };
            let _ = app.emit(TEXT_EVENT, TextArrived { ok });
        });
    }

    fn pad(&self, event: pad::PadEvent) {
        let Some(inner) = self.0.upgrade() else { return };
        if let Err(e) = inner.app.emit(PAD_EVENT, event) {
            log::warn!("emit {PAD_EVENT} failed: {e}");
        }
    }
}

/// Unfinished uploads, under tug's local app data.
fn incoming_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|p| p.join("tugboat-incoming"))
        .map_err(|_| "Couldn't find tug's data folder.".to_string())
}

/// `Pictures\Tugboat`, from Windows' Pictures known folder (wherever it's been moved to).
fn tugboat_folder(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .picture_dir()
        .map(|p| p.join("Tugboat"))
        .map_err(|_| "Couldn't find your Pictures folder.".to_string())
}

/// The link in the QR code: the address, and the secret after the `#`. Nothing else, ever: who's
/// bound stays on the PC, so a second phone scanning a re-shown code can't pass for the first.
fn link(ip: Ipv4Addr, port: u16, secret: &[u8]) -> String {
    format!("http://{ip}:{port}/#{}", crypto::b64(secret))
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
    let url = link(ip, port, secret);
    let qr = qr::encode(&url);
    let (tx, rx) = oneshot::channel::<()>();
    let task = tauri::async_runtime::spawn(server::serve(listener, session, async {
        let _ = rx.await;
    }));
    log::info!("tugboat: listening on port {port}");
    Ok(Endpoint {
        ip,
        port,
        url,
        qr,
        shutdown: Some(tx),
        task,
    })
}

impl TugboatService {
    pub fn new(app: AppHandle) -> TugboatService {
        // Partial uploads a crash left behind are pre-sized (up to 8 GB each): free that space now
        // rather than whenever Tugboat is next opened.
        if let Ok(dir) = incoming_dir(&app) {
            session::clean_incoming(&dir);
        }
        TugboatService {
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

    pub fn status(&self) -> TugboatStatus {
        let folder = tugboat_folder(&self.inner.app)
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
            return TugboatStatus {
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
                sending: false,
                receiving: false,
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
        TugboatStatus {
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
            sending: snap.sending,
            receiving: snap.receiving,
            ended: None,
        }
    }

    /// Open Tugboat (or return the session already open): a new secret, a fresh port.
    pub async fn start(&self) -> Result<TugboatStatus, String> {
        if self.session().is_some() {
            return Ok(self.status());
        }
        let folder = tugboat_folder(&self.inner.app)?;
        let incoming = incoming_dir(&self.inner.app)?;
        {
            let incoming = incoming.clone();
            let folder = folder.clone();
            let _ = tokio::task::spawn_blocking(move || {
                session::clean_incoming(&incoming);
                session::sweep_temp(&folder);
            })
            .await;
        }
        let secret = crypto::new_secret();
        let sink: Arc<dyn Sink> = Arc::new(PanelSink(Arc::downgrade(&self.inner)));
        let session = Arc::new(Session::new(&secret, folder, incoming, sink));
        let ip = tokio::task::spawn_blocking(net::current).await.ok().flatten();
        let endpoint = match ip {
            Some(ip) => match open_endpoint(ip, 0, &secret, session.clone()).await {
                Ok(e) => Some(e),
                Err(e) => {
                    log::warn!("tugboat: couldn't listen: {e}");
                    return Err("Couldn't start Tugboat on this network.".into());
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
            let port = endpoint.as_ref().map_or(0, |e| e.port);
            let advertised = endpoint.as_ref().map(|e| (e.ip, e.port));
            *run = Some(Running {
                generation,
                secret,
                session,
                endpoint,
                port,
                advertised,
                stop_when_quiet: false,
            });
        }
        *lock(&self.inner.ended) = None;
        let svc = self.clone();
        tauri::async_runtime::spawn(async move { svc.supervise(generation).await });
        self.emit();
        Ok(self.status())
    }

    /// Close Tugboat: stop listening, invalidate the secret, remove unfinished uploads.
    pub async fn stop(&self, ended: Option<Ended>) {
        let running = lock(&self.inner.running).take();
        if let Some(r) = running {
            // The game hears its controller went with Tugboat.
            r.session.pad_close();
            r.session.close();
            if let Some(e) = r.endpoint {
                e.close();
            }
            let incoming = r.session.incoming.clone();
            let _ = tokio::task::spawn_blocking(move || session::clean_incoming(&incoming)).await;
            let why = match ended {
                Some(Ended::Idle) => " (idle)",
                Some(Ended::Hidden) => " (window hidden)",
                None => "",
            };
            log::info!("tugboat: closed{why}");
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
        let mut was_sending = false;
        let mut was_receiving = false;
        loop {
            tokio::time::sleep(TICK).await;
            let (session, ip, port, stop_when_quiet) = {
                let run = lock(&self.inner.running);
                match run.as_ref() {
                    Some(r) if r.generation == generation => (
                        r.session.clone(),
                        r.endpoint.as_ref().map(|e| e.ip),
                        r.port,
                        r.stop_when_quiet,
                    ),
                    _ => return,
                }
            };
            if session.idle_for() >= IDLE_LIMIT {
                self.stop(Some(Ended::Idle)).await;
                return;
            }
            if stop_when_quiet && !session.transferring() {
                self.stop(Some(Ended::Hidden)).await;
                return;
            }
            let active = session.phone_active();
            let sending = session.sending();
            let receiving = session.receiving();
            if active != was_active || sending != was_sending || receiving != was_receiving {
                was_active = active;
                was_sending = sending;
                was_receiving = receiving;
                self.emit();
            }
            ticks += 1;
            if ticks.is_multiple_of(NET_EVERY) || ip.is_none() {
                let now = tokio::task::spawn_blocking(net::current).await.ok().flatten();
                if now != ip {
                    log::info!("tugboat: network changed");
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
            Some(ip) => open_endpoint(ip, port_hint, &secret, session.clone()).await.ok(),
            None => None,
        };
        let mut moved = false;
        {
            let mut run = lock(&self.inner.running);
            match run.as_mut() {
                Some(r) if r.generation == generation => {
                    if let Some(e) = &endpoint {
                        r.port = e.port;
                        let now = Some((e.ip, e.port));
                        moved = r.advertised.is_some() && r.advertised != now;
                        r.advertised = now;
                    }
                    r.endpoint = endpoint;
                }
                _ => {
                    if let Some(e) = endpoint {
                        e.close();
                    }
                    return;
                }
            }
        }
        // A new address is a new browser origin: the phone comes back with a new client id, so let
        // whoever scans the new code bind (the panel shows the code again and says so).
        if moved {
            log::info!("tugboat: new address, waiting for the phone to scan again");
            session.unbind();
        }
        self.emit();
    }

    /// tug's window hid to the tray. Nothing on screen would show Tugboat is still listening, so
    /// stop now, or as soon as files already moving are done.
    pub fn window_hidden(&self) {
        {
            let mut run = lock(&self.inner.running);
            let Some(r) = run.as_mut() else { return };
            if r.session.transferring() {
                r.stop_when_quiet = true;
                return;
            }
        }
        let svc = self.clone();
        tauri::async_runtime::spawn(async move { svc.stop(Some(Ended::Hidden)).await });
    }

    /// The window is back: a transfer that outlived the hide can keep Tugboat open after all.
    pub fn window_shown(&self) {
        if let Some(r) = lock(&self.inner.running).as_mut() {
            r.stop_when_quiet = false;
        }
    }

    /// Files dropped onto tug's window (handled here, not in the webview, so page script can never
    /// name paths to offer): open Tugboat if needed, offer them, and tell the panel what was skipped.
    pub fn offer_dropped(&self, paths: Vec<PathBuf>) {
        let svc = self.clone();
        tauri::async_runtime::spawn(async move {
            match svc.offer(paths).await {
                Ok(skipped) => {
                    let _ = svc.inner.app.emit(DROPPED_EVENT, skipped);
                }
                Err(e) => log::warn!("tugboat: couldn't offer dropped files: {e}"),
            }
        });
    }

    /// "Copy link": the QR link on the clipboard, kept out of Windows' clipboard history and cloud
    /// sync, since it carries the session secret.
    pub fn copy_link(&self) -> Result<(), String> {
        let url = lock(&self.inner.running)
            .as_ref()
            .and_then(|r| r.endpoint.as_ref().map(|e| e.url.clone()))
            .ok_or("Tugboat isn't open.")?;
        crate::clipboard::set_text_private(&url)
    }

    /// Offer files to the phone, opening Tugboat first if it's closed (files dragged onto tug).
    pub async fn offer(&self, paths: Vec<PathBuf>) -> Result<Vec<Skipped>, String> {
        if self.session().is_none() {
            self.start().await?;
        }
        let session = self.session().ok_or("Tugboat isn't open.")?;
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
        self.session().ok_or("Tugboat isn't open.")?.set_pc_text(text)
    }

    /// Tugboat Run wants the phone as a controller: open Tugboat if it's off (same session, same
    /// code, same rules), open the controller channel, and watch it for a phone that goes quiet.
    pub async fn pad_open(&self) -> Result<TugboatStatus, String> {
        if self.session().is_none() {
            self.start().await?;
        }
        let session = self.session().ok_or("Tugboat isn't open.")?;
        if session.pad_open() {
            let watched = session.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(PAD_TICK).await;
                    if watched.is_closed() || !watched.pad_tick() {
                        return;
                    }
                }
            });
        }
        if let Some(e) = session.pad_event() {
            let _ = self.inner.app.emit(PAD_EVENT, e);
        }
        Ok(self.status())
    }

    /// The game closed: refuse controller inputs again. Tugboat itself carries on under its
    /// usual rules (closing the panel, hiding tug, or 10 minutes unused).
    pub fn pad_close(&self) {
        if let Some(s) = self.session() {
            s.pad_close();
        }
    }

    /// What the phone's controller shows: paused, and hits so far (it buzzes on a new one).
    pub fn pad_feedback(&self, reply: pad::PadReply) {
        if let Some(s) = self.session() {
            s.pad_feedback(reply);
        }
    }

    /// Open the Tugboat folder in Explorer, or select one file this session saved.
    pub fn open_folder(&self, file: Option<&str>) -> Result<(), String> {
        let folder = tugboat_folder(&self.inner.app)?;
        std::fs::create_dir_all(&folder).map_err(|_| "Couldn't open the Tugboat folder.".to_string())?;
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
