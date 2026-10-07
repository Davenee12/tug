//! Developer tools (v0.5.12): AI tools (through `tug mcp`) and the `tug` command use tug over
//! its local bridge (`tug-bridge`: a named pipe only this Windows user can open, never a network
//! port, with a per-install token).
//!
//! Everything is off until the user switches on Settings › Developer tools › "Let AI tools use
//! tug". Then each switch gates its tools (reading on, music and texts off), every tool is
//! rate-limited, and `send_text` never sends without a click on tug's confirmation card. Each
//! call is logged to tug.log by tool name, who asked, outcome and count, never by content.
//!
//! Pure parts are tested in their modules: `confirm` (the confirmation state machine),
//! `rate_limit`, `recipient` (who "Sam" is), `dev_apps` (the developer-app allow-list), `files`
//! (the Tugboat folder), `settings`, `path_env` and `phone_model`; the protocol, auth and the
//! whole exchange are tested in `tug-bridge`.

pub mod confirm;
pub mod dev_apps;
pub mod files;
pub mod path_env;
pub mod phone_model;
pub mod rate_limit;
pub mod recipient;
pub mod settings;
mod tools;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;
use tug_bridge::protocol::{
    effective_permissions, BridgeError, ClientInfo, ErrorCode, Permission, Request, SendOutcome, SendResult,
};
use tug_bridge::server::{BoxFuture, Handler};

use crate::ble::BleHandle;
use crate::state::{now_ms, Shared};
use crate::tugboat::TugboatService;
use confirm::{ConfirmRequest, Confirmations, Resolution, CONFIRM_TIMEOUT_MS};
use rate_limit::RateLimiter;
use settings::ClientRecord;

/// A fresh `DevToolsStatus` whenever anything in Settings › Developer tools changes.
pub const STATUS_EVENT: &str = "devtools-status";
/// The confirmation card: a `ConfirmRequest` to show, or `null` once it's answered or gone.
pub const CONFIRM_EVENT: &str = "devtools-confirm";
/// Sending, once the person clicked Send.
const SEND_TIMEOUT: Duration = Duration::from_secs(60);

/// One switch in Settings › Developer tools. Mirrored in `src/types/protocol.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionState {
    pub key: &'static str,
    pub label: &'static str,
    pub on: bool,
}

/// Everything Settings › Developer tools shows. Mirrored in `src/types/protocol.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevToolsStatus {
    pub enabled: bool,
    pub permissions: Vec<PermissionState>,
    pub clients: Vec<ClientRecord>,
    /// `...\tug\bin\tug.exe`, the command and MCP server; `None` if it isn't installed.
    pub cli_path: Option<String>,
    /// The folder "Add tug to PATH" adds.
    pub cli_dir: Option<String>,
    pub on_path: bool,
    /// The bridge is listening (false if another copy of tug holds it).
    pub bridge_running: bool,
    pub pending: Option<ConfirmRequest>,
}

pub struct DevTools {
    app: AppHandle,
    shared: Arc<Shared>,
    ble: BleHandle,
    tugboat: TugboatService,
    token_path: Option<PathBuf>,
    token: Mutex<Option<String>>,
    limiter: Mutex<RateLimiter>,
    confirms: Mutex<Confirmations>,
    waiters: Mutex<HashMap<u64, oneshot::Sender<Resolution>>>,
    running: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl DevTools {
    pub fn new(app: AppHandle, shared: Arc<Shared>, ble: BleHandle, tugboat: TugboatService) -> Arc<DevTools> {
        let token_path = app
            .path()
            .app_local_data_dir()
            .ok()
            .map(|d| d.join(tug_bridge::paths::TOKEN_FILE));
        let dt = Arc::new(DevTools {
            app,
            shared,
            ble,
            tugboat,
            token_path,
            token: Mutex::new(None),
            limiter: Mutex::new(RateLimiter::default()),
            confirms: Mutex::new(Confirmations::default()),
            waiters: Mutex::new(HashMap::new()),
            running: AtomicBool::new(false),
        });
        // Load the token if Developer tools were set up before (made fresh if switched on and
        // the file went missing).
        if let Some(path) = &dt.token_path {
            let loaded = if dt.is_enabled() {
                tug_bridge::token_file::load_or_create(path).ok()
            } else {
                tug_bridge::token_file::load(path).ok()
            };
            *lock(&dt.token) = loaded;
        }
        dt
    }

    /// Start listening on the bridge (for the life of the app). With Developer tools off, the
    /// pipe only ever answers "off".
    pub fn start(self: &Arc<Self>) {
        let dt = self.clone();
        tauri::async_runtime::spawn(async move {
            #[cfg(windows)]
            {
                let sid = match tug_bridge::win::current_user_sid() {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!("devtools: bridge unavailable (no user SID: {e})");
                        return;
                    }
                };
                let pipe = tug_bridge::paths::pipe_name(&sid, &dt.app.config().identifier);
                dt.running.store(true, Ordering::SeqCst);
                let handler: Arc<dyn Handler> = dt.clone();
                let result = tug_bridge::server::serve(pipe, sid, handler, std::future::pending()).await;
                dt.running.store(false, Ordering::SeqCst);
                if let Err(e) = result {
                    log::warn!("devtools: bridge stopped: {e}");
                    dt.emit_status();
                }
            }
            #[cfg(not(windows))]
            let _ = dt;
        });
    }

    fn setting(&self, key: &str) -> Option<String> {
        self.shared.store.setting(key).ok().flatten()
    }

    pub fn is_enabled(&self) -> bool {
        settings::parse_enabled(self.setting(settings::ENABLED).as_deref())
    }

    fn permissions(&self) -> BTreeMap<Permission, bool> {
        effective_permissions(&settings::parse_permissions(
            self.setting(settings::PERMISSIONS).as_deref(),
        ))
    }

    fn cli_path() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let cli = exe.parent()?.join("bin").join("tug.exe");
        // A debug build without `npm run build:cli` has an empty placeholder there.
        std::fs::metadata(&cli).ok().filter(|m| m.len() > 0).map(|_| cli)
    }

    pub fn status(&self) -> DevToolsStatus {
        let perms = self.permissions();
        let cli = Self::cli_path();
        let cli_dir = cli
            .as_ref()
            .and_then(|p| p.parent())
            .map(|d| d.to_string_lossy().into_owned());
        DevToolsStatus {
            enabled: self.is_enabled(),
            permissions: Permission::ALL
                .into_iter()
                .map(|p| PermissionState {
                    key: p.key(),
                    label: p.label(),
                    on: perms[&p],
                })
                .collect(),
            clients: settings::parse_clients(self.setting(settings::CLIENTS).as_deref()),
            cli_path: cli.map(|p| p.to_string_lossy().into_owned()),
            on_path: cli_dir.as_deref().is_some_and(path_env::is_on_path),
            cli_dir,
            bridge_running: self.running.load(Ordering::SeqCst),
            pending: lock(&self.confirms).pending().cloned(),
        }
    }

    fn emit_status(&self) {
        if let Err(e) = self.app.emit(STATUS_EVENT, self.status()) {
            log::warn!("emit {STATUS_EVENT} failed: {e}");
        }
    }

    /// The master switch. Switching on makes the token (once); switching off drops any
    /// question waiting on the confirmation card.
    pub fn set_enabled(&self, on: bool) -> Result<DevToolsStatus, String> {
        if on {
            let path = self.token_path.as_ref().ok_or("Couldn't find tug's data folder.")?;
            let token = tug_bridge::token_file::load_or_create(path).map_err(|e| {
                log::warn!("devtools: couldn't write the token: {e}");
                "Couldn't set up access for AI tools.".to_string()
            })?;
            *lock(&self.token) = Some(token);
        } else {
            self.cancel_pending();
        }
        self.shared
            .store
            .set_setting(settings::ENABLED, if on { "true" } else { "false" })
            .map_err(|e| e.to_string())?;
        log::info!("devtools: AI tools {}", if on { "on" } else { "off" });
        self.emit_status();
        Ok(self.status())
    }

    pub fn set_permission(&self, key: &str, on: bool) -> Result<DevToolsStatus, String> {
        let p = Permission::from_key(key).ok_or_else(|| format!("unknown tool switch {key}"))?;
        let mut saved = settings::parse_permissions(self.setting(settings::PERMISSIONS).as_deref());
        saved.insert(p.key().to_string(), on);
        let json = serde_json::to_string(&saved).map_err(|e| e.to_string())?;
        self.shared
            .store
            .set_setting(settings::PERMISSIONS, &json)
            .map_err(|e| e.to_string())?;
        if p == Permission::SendText && !on {
            self.cancel_pending();
        }
        log::info!("devtools: {} {}", p.key(), if on { "on" } else { "off" });
        self.emit_status();
        Ok(self.status())
    }

    /// "Revoke access": a new token, so every AI tool connected now has to be restarted (and
    /// anything that copied the old token is locked out); the Connected tools list starts over.
    pub fn revoke(&self) -> Result<DevToolsStatus, String> {
        let path = self.token_path.as_ref().ok_or("Couldn't find tug's data folder.")?;
        let token = tug_bridge::token_file::rotate(path).map_err(|e| {
            log::warn!("devtools: couldn't rotate the token: {e}");
            "Couldn't reset access.".to_string()
        })?;
        *lock(&self.token) = Some(token);
        self.cancel_pending();
        let _ = self.shared.store.delete_setting(settings::CLIENTS);
        log::info!("devtools: access revoked (new token)");
        self.emit_status();
        Ok(self.status())
    }

    pub fn add_to_path(&self, add: bool) -> Result<DevToolsStatus, String> {
        let dir = Self::cli_path()
            .and_then(|p| p.parent().map(|d| d.to_string_lossy().into_owned()))
            .ok_or("The tug command isn't installed with this copy of tug.")?;
        if add {
            path_env::add(&dir)?;
        } else {
            path_env::remove(&dir)?;
        }
        log::info!(
            "devtools: tug command {} PATH",
            if add { "added to" } else { "removed from" }
        );
        self.emit_status();
        Ok(self.status())
    }

    /// The person answered the confirmation card.
    pub fn confirm(&self, id: u64, send: bool) {
        let resolution = lock(&self.confirms).decide(id, send, now_ms());
        if let Some(r) = resolution {
            if let Some(tx) = lock(&self.waiters).remove(&id) {
                let _ = tx.send(r);
            }
        }
    }

    fn cancel_pending(&self) {
        let cancelled = lock(&self.confirms).cancel();
        if let Some(id) = cancelled {
            if let Some(tx) = lock(&self.waiters).remove(&id) {
                let _ = tx.send(Resolution::Cancelled);
            }
            let _ = self.app.emit(CONFIRM_EVENT, None::<ConfirmRequest>);
        }
    }

    fn record_client(&self, client: &ClientInfo) {
        let list = settings::parse_clients(self.setting(settings::CLIENTS).as_deref());
        let list = settings::record_client(list, client, now_ms());
        if let Ok(json) = serde_json::to_string(&list) {
            let _ = self.shared.store.set_setting(settings::CLIENTS, &json);
        }
        self.emit_status();
    }

    /// Ask the person, and wait (up to 2 minutes) for Send or Don't send.
    async fn ask_to_send(
        &self,
        client: &ClientInfo,
        to_name: String,
        to_address: String,
        message: String,
    ) -> Resolution {
        let opened = lock(&self.confirms).open(client.display_name(), to_name, to_address, message, now_ms());
        let Ok(req) = opened else {
            return Resolution::Cancelled;
        };
        let (tx, rx) = oneshot::channel();
        lock(&self.waiters).insert(req.id, tx);
        let id = req.id;
        let _ = self.app.emit(CONFIRM_EVENT, Some(req));
        crate::tray::show(&self.app);
        let wait = Duration::from_millis(CONFIRM_TIMEOUT_MS as u64);
        let resolution = match tokio::time::timeout(wait, rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Resolution::Cancelled,
            Err(_) => {
                let expired = lock(&self.confirms).expire(id);
                lock(&self.waiters).remove(&id);
                expired.unwrap_or(Resolution::Cancelled)
            }
        };
        let _ = self.app.emit(CONFIRM_EVENT, None::<ConfirmRequest>);
        resolution
    }

    /// Whether a confirmation is already showing (checked before asking, so a second request
    /// gets "busy" rather than queueing).
    fn confirm_busy(&self) -> bool {
        lock(&self.confirms).pending().is_some_and(|p| now_ms() < p.expires_at)
    }

    /// `send_text` after the recipient is known: ask, then send only on Send.
    async fn confirm_and_send(
        &self,
        client: &ClientInfo,
        to_name: String,
        address: String,
        message: String,
    ) -> Result<SendResult, BridgeError> {
        if self.confirm_busy() {
            return Err(BridgeError::new(
                ErrorCode::Busy,
                "tug is already asking about another text. Try again once that's answered.",
            ));
        }
        let resolution = self
            .ask_to_send(client, to_name.clone(), address.clone(), message.clone())
            .await;
        let (outcome, detail) = match resolution {
            Resolution::Declined => (SendOutcome::Declined, Some("They chose Don't send.".to_string())),
            Resolution::TimedOut => (
                SendOutcome::TimedOut,
                Some("Nobody answered within 2 minutes, so it wasn't sent.".to_string()),
            ),
            Resolution::Cancelled => (SendOutcome::Cancelled, Some("It was cancelled in tug.".to_string())),
            Resolution::Approved => {
                // The same path as tug's own composer (#99): saved and shown as "Sending…" first,
                // and a send the phone didn't take comes back as a failed row, not an error.
                let sent = tokio::time::timeout(
                    SEND_TIMEOUT,
                    crate::map::service::send_text(&self.shared, &address, &message),
                )
                .await
                .unwrap_or_else(|_| Err("The phone didn't answer in time.".to_string()));
                match sent {
                    Ok(m) if m.status == crate::messages::Status::Failed => (
                        SendOutcome::Failed,
                        Some("Your iPhone didn't send it. Retry it from the conversation in tug.".to_string()),
                    ),
                    Ok(m) if m.status == crate::messages::Status::Unconfirmed => (
                        SendOutcome::Sent,
                        Some("It may have sent; check your iPhone to be sure.".to_string()),
                    ),
                    Ok(_) => (SendOutcome::Sent, None),
                    Err(e) => (SendOutcome::Failed, Some(e)),
                }
            }
        };
        Ok(SendResult {
            outcome,
            to: to_name,
            detail,
        })
    }
}

/// How many results an answer carried, for the log line.
fn count(v: &serde_json::Value) -> usize {
    match v {
        serde_json::Value::Array(a) => a.len(),
        serde_json::Value::Null => 0,
        _ => 1,
    }
}

impl Handler for DevTools {
    fn enabled(&self) -> bool {
        self.is_enabled()
    }

    fn token(&self) -> Option<String> {
        lock(&self.token).clone()
    }

    fn now_ms(&self) -> i64 {
        now_ms()
    }

    fn time_limit(&self, request: &Request) -> Duration {
        match request {
            Request::SendText { .. } => {
                Duration::from_millis(CONFIRM_TIMEOUT_MS as u64) + SEND_TIMEOUT + Duration::from_secs(10)
            }
            _ => Duration::from_secs(15),
        }
    }

    fn rejected(&self, client: &ClientInfo, why: ErrorCode) {
        log::warn!("devtools: refused a call from {:?} ({why:?})", client.display_name());
    }

    fn handle(&self, client: ClientInfo, request: Request) -> BoxFuture<'_, Result<serde_json::Value, BridgeError>> {
        Box::pin(async move {
            let tool = request.tool();
            let who = format!("{:?} {:?}", client.kind, client.display_name());
            if let Some(p) = request.permission() {
                if !self.permissions()[&p] {
                    log::info!("devtools: {tool} by {who}: switched off");
                    return Err(BridgeError::new(
                        ErrorCode::ToolOff,
                        format!("\"{}\" is switched off in tug › Settings › Developer tools.", p.label()),
                    ));
                }
            }
            let limited = lock(&self.limiter).check(tool, now_ms());
            if let Err(secs) = limited {
                log::info!("devtools: {tool} by {who}: rate limited");
                return Err(BridgeError::new(
                    ErrorCode::RateLimited,
                    format!("Too many {tool} calls. Try again in {secs} s."),
                ));
            }
            self.record_client(&client);
            let result = tools::run(self, &client, request).await;
            match &result {
                Ok(v) => match v.get("outcome").and_then(|o| o.as_str()) {
                    Some(outcome) => log::info!("devtools: {tool} by {who}: {outcome}"),
                    None => log::info!("devtools: {tool} by {who}: ok ({})", count(v)),
                },
                Err(e) => log::info!("devtools: {tool} by {who}: {:?}", e.code),
            }
            result
        })
    }
}
