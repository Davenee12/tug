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
pub mod outgoing;
pub mod path_env;
pub mod phone_model;
pub mod rate_limit;
pub mod recipient;
pub mod settings;
mod tools;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;
use tug_bridge::protocol::{
    effective_permissions, offer_span, BridgeError, ClientInfo, ErrorCode, OfferOutcome, OfferResult, Permission,
    Progress, Request, SendOutcome, SendResult,
};
use tug_bridge::server::{BoxFuture, Handler, ProgressSink};

use crate::ble::BleHandle;
use crate::state::{now_ms, RevCounter, Shared};
use crate::tugboat::held::{self, Verdict};
use crate::tugboat::TugboatService;
use confirm::{Ask, ConfirmRequest, Confirmations, Resolution, CONFIRM_TIMEOUT_MS};
use rate_limit::RateLimiter;
use settings::ClientRecord;

/// A fresh `DevToolsStatus` whenever anything in Settings › Developer tools changes.
pub const STATUS_EVENT: &str = "devtools-status";
/// The confirmation card: a `ConfirmRequest` to show, or `null` once it's answered or gone.
pub const CONFIRM_EVENT: &str = "devtools-confirm";
/// How long the AI tool's call waits for the phone once the person clicked Send. The text is
/// saved and queued before this starts and the worker always answers, so running out only stops
/// the wait: the text may still go, and the answer is "queued", never "failed" (an AI tool told
/// "failed" sends it again).
const SEND_WAIT: Duration = Duration::from_secs(60);

/// The bridge's limit for a `send_text` call: the card's 2 minutes plus the wait for the phone,
/// with room to answer. Under the client's own limit (`tug_bridge::client::SEND_TEXT_TIMEOUT`), so
/// the client always hears the outcome.
const fn send_text_time_limit() -> Duration {
    Duration::from_millis(CONFIRM_TIMEOUT_MS as u64 + SEND_WAIT.as_millis() as u64 + 10_000)
}

/// The bridge's limit for an `offer_files` call: the card, the hold and the grace for a file still
/// being fetched (`offer_span`), with room to answer. Under the client's own limit
/// (`tug_bridge::client::time_limit`), so the command always hears how it went.
fn offer_time_limit(hold_secs: u32) -> Duration {
    offer_span(hold_secs) + Duration::from_secs(10)
}

/// What came of an approved text, as far as the AI tool's call saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Delivery {
    /// The worker answered with the stored row in this state.
    Stored(crate::messages::Status),
    /// Nothing was saved (no message service, empty text, a store error): safe to try again.
    NotSaved(String),
    /// Saved and queued, but the phone hadn't answered within `SEND_WAIT`.
    StillQueued,
}

/// The outcome and detail an approved text reports. Pure, so the mapping is unit-tested: above
/// all, a text that's saved and may still go is never reported as failed.
fn approved_outcome(delivery: Delivery) -> (SendOutcome, Option<String>) {
    use crate::messages::Status;
    const QUEUED: &str = "It's saved in tug and still waiting for your iPhone, so it may still send. \
                          Check the conversation in tug. Don't send it again.";
    match delivery {
        Delivery::Stored(Status::Failed) => (
            SendOutcome::Failed,
            Some("Your iPhone didn't send it. Retry it from the conversation in tug.".to_string()),
        ),
        Delivery::Stored(Status::Unconfirmed) => (
            SendOutcome::Sent,
            Some("It may have sent; check your iPhone to be sure. Don't send it again.".to_string()),
        ),
        Delivery::Stored(Status::Accepted | Status::Sent) => (SendOutcome::Sent, None),
        Delivery::Stored(Status::Pending | Status::Received) | Delivery::StillQueued => {
            (SendOutcome::Queued, Some(QUEUED.to_string()))
        }
        Delivery::NotSaved(e) => (SendOutcome::Failed, Some(e)),
    }
}

/// How often a `tug boat` waiting on the phone looks again.
const HOLD_POLL: Duration = Duration::from_millis(500);

/// What an `offer_files` call says when the card wasn't answered with Send. `None` for Send.
fn unapproved(resolution: Resolution) -> Option<(OfferOutcome, Option<String>)> {
    match resolution {
        Resolution::Approved => None,
        Resolution::Declined => Some((OfferOutcome::Declined, Some("They chose Don't send.".into()))),
        Resolution::TimedOut => Some((
            OfferOutcome::NotConfirmed,
            Some("Nobody answered within 2 minutes, so nothing was sent.".into()),
        )),
        Resolution::Cancelled => Some((OfferOutcome::Cancelled, Some("it was cancelled in tug.".into()))),
    }
}

/// The wait for the phone's next step.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// Keep waiting; it has fetched `fetched` of `total`.
    Wait { fetched: usize, total: usize },
    /// Done: answer this.
    Answer(OfferOutcome, Option<String>),
    /// Stop holding the files (what the phone hasn't fetched is withdrawn), then answer this.
    Withdraw(OfferOutcome, Option<String>),
}

/// Pure, so each way a wait can end is tested: what the hold says comes first (a phone that got
/// everything has it, whatever happened next), then Tugboat closing, the switch going off, and
/// the bridge's own limit coming up.
fn wait_step(verdict: Verdict, closed: bool, switched_off: bool, overdue: bool) -> Step {
    match verdict {
        Verdict::Delivered => Step::Answer(OfferOutcome::Delivered, None),
        Verdict::TimedOut => Step::Answer(OfferOutcome::TimedOut, None),
        Verdict::Withdrawn => Step::Answer(
            OfferOutcome::Cancelled,
            Some("a file was taken off Tugboat in tug, so the rest weren't sent either.".into()),
        ),
        Verdict::Waiting { .. } if closed => Step::Answer(
            OfferOutcome::Cancelled,
            Some("Tugboat was closed in tug before your iPhone downloaded everything.".into()),
        ),
        Verdict::Waiting { .. } if switched_off => Step::Withdraw(
            OfferOutcome::Cancelled,
            Some("sending files was switched off in tug.".into()),
        ),
        Verdict::Waiting { .. } if overdue => Step::Withdraw(OfferOutcome::TimedOut, None),
        Verdict::Waiting { fetched, total } => Step::Wait { fetched, total },
    }
}

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
    /// Numbers each copy in the order it was composed (`state::RevCounter`): an event and a
    /// command's answer can arrive in either order, and the UI keeps the newest.
    pub rev: u64,
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
    /// Numbers status copies (see `DevToolsStatus::rev`).
    revs: RevCounter,
    /// Moves on whenever sending files is turned off (its switch, the master switch, Revoke
    /// access): a `tug boat` still waiting on the phone sees it and withdraws its files.
    files_epoch: AtomicU64,
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
            revs: RevCounter::default(),
            files_epoch: AtomicU64::new(0),
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

    /// The status as a command answers it, numbered after every copy composed before it.
    pub fn status(&self) -> DevToolsStatus {
        self.revs.stamp(|rev| self.compose(rev), |_| ())
    }

    fn compose(&self, rev: u64) -> DevToolsStatus {
        let perms = self.permissions();
        let cli = Self::cli_path();
        let cli_dir = cli
            .as_ref()
            .and_then(|p| p.parent())
            .map(|d| d.to_string_lossy().into_owned());
        DevToolsStatus {
            rev,
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

    /// Compose, number and emit under one lock, so racing updates reach the window in order.
    fn emit_status(&self) {
        self.revs.stamp(
            |rev| self.compose(rev),
            |s| {
                if let Err(e) = self.app.emit(STATUS_EVENT, s) {
                    log::warn!("emit {STATUS_EVENT} failed: {e}");
                }
            },
        );
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
            self.cancel_pending(|_| true);
            self.stop_sending_files();
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
            self.cancel_pending(|a| !a.is_files());
        }
        if p == Permission::TugboatSend && !on {
            self.cancel_pending(Ask::is_files);
            self.stop_sending_files();
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
        self.cancel_pending(|_| true);
        self.stop_sending_files();
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

    /// Every file held for the phone is withdrawn: a `tug boat` still waiting notices and answers
    /// Cancelled, and one that already returned (`--no-wait`) loses its files too, so turning the
    /// switch off really stops what it allowed.
    fn stop_sending_files(&self) {
        self.files_epoch.fetch_add(1, Ordering::SeqCst);
        self.tugboat.cancel_all_holds();
    }

    /// Drop the card on screen if `which` says it's affected.
    fn cancel_pending(&self, which: impl Fn(&Ask) -> bool) {
        let cancelled = lock(&self.confirms).cancel_if(which);
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

    /// Ask the person, and wait (up to 2 minutes) for Send or Don't send. Another card already
    /// showing means "busy", never a queue.
    async fn ask(&self, client: &ClientInfo, ask: Ask) -> Result<Resolution, BridgeError> {
        let opened = lock(&self.confirms).open(client.display_name(), ask, now_ms());
        let Ok(req) = opened else {
            return Err(BridgeError::new(
                ErrorCode::Busy,
                "tug is already asking you about something else. Try again once that's answered.",
            ));
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
        Ok(resolution)
    }

    /// `send_text` after the recipient is known: ask, then send only on Send.
    async fn confirm_and_send(
        &self,
        client: &ClientInfo,
        to_name: String,
        address: String,
        message: String,
    ) -> Result<SendResult, BridgeError> {
        let ask = Ask::Text {
            to_name: to_name.clone(),
            to_address: address.clone(),
            message: message.clone(),
        };
        let resolution = self.ask(client, ask).await?;
        let (outcome, detail) = match resolution {
            Resolution::Declined => (SendOutcome::Declined, Some("They chose Don't send.".to_string())),
            Resolution::TimedOut => (
                SendOutcome::TimedOut,
                Some("Nobody answered within 2 minutes, so it wasn't sent.".to_string()),
            ),
            Resolution::Cancelled => (SendOutcome::Cancelled, Some("It was cancelled in tug.".to_string())),
            Resolution::Approved => {
                // The same path as tug's own composer (#99): saved, shown as "Sending…" and queued
                // before anything waits, and a send the phone didn't take comes back as a failed
                // row, not an error. Running out of SEND_WAIT only stops this wait: the queued text
                // still goes (or fails with Retry) in tug.
                let sent = tokio::time::timeout(
                    SEND_WAIT,
                    crate::map::service::send_text(&self.shared, &address, &message),
                )
                .await;
                approved_outcome(match sent {
                    Ok(Ok(m)) => Delivery::Stored(m.status),
                    Ok(Err(e)) => Delivery::NotSaved(e),
                    Err(_) => {
                        log::info!("devtools: send_text still waiting on the phone; answered queued");
                        Delivery::StillQueued
                    }
                })
            }
        };
        Ok(SendResult {
            outcome,
            to: to_name,
            detail,
        })
    }

    /// `offer_files` once its files have been checked: ask, and only on Send open Tugboat (with
    /// its code on screen if the phone's page isn't open), hold the files for the phone, and wait
    /// for it to download them unless told not to.
    async fn confirm_and_offer(
        &self,
        client: &ClientInfo,
        picked: Vec<outgoing::Picked>,
        hold_secs: u32,
        no_wait: bool,
        progress: &ProgressSink,
    ) -> Result<OfferResult, BridgeError> {
        let names: Vec<String> = picked.iter().map(|p| p.name.clone()).collect();
        let total_bytes: u64 = picked.iter().map(|p| p.size).sum();
        let answer = |outcome, detail| OfferResult {
            outcome,
            names: names.clone(),
            total_bytes,
            hold_secs,
            detail,
        };
        // Counts and sizes only, never names or paths.
        log::info!("devtools: asking to send {} files ({total_bytes} bytes)", picked.len());
        let files = picked.iter().map(outgoing::Picked::card).collect();
        progress.send(Progress::Confirming);
        let resolution = self.ask(client, Ask::Files { files }).await?;
        if let Some((outcome, detail)) = unapproved(resolution) {
            return Ok(answer(outcome, detail));
        }
        // Sending files may have been switched off while the card was up.
        let epoch = self.files_epoch.load(Ordering::SeqCst);
        if !self.permissions()[&Permission::TugboatSend] || !self.is_enabled() {
            return Ok(answer(
                OfferOutcome::Cancelled,
                Some("sending files was switched off in tug.".into()),
            ));
        }
        let to_offer = tokio::task::spawn_blocking(move || outgoing::to_offer(&picked))
            .await
            .map_err(|e| {
                log::warn!("devtools: reading files to send failed: {e}");
                BridgeError::new(ErrorCode::Internal, "Something went wrong in tug. Try again.")
            })??;
        let window = Duration::from_secs(u64::from(hold_secs));
        let (session, hold) = self
            .tugboat
            .hold(to_offer, window)
            .await
            .map_err(|e| BridgeError::new(ErrorCode::Unavailable, e))?;
        // No Tugboat page open on the phone: show the code to scan.
        if !session.phone_active() {
            crate::tray::show(&self.app);
            self.tugboat.show_panel();
        }
        if no_wait {
            // Switched off between the card and here: what was just held goes again.
            if self.files_epoch.load(Ordering::SeqCst) != epoch {
                session.cancel_hold(hold);
                return Ok(answer(
                    OfferOutcome::Cancelled,
                    Some("sending files was switched off in tug.".into()),
                ));
            }
            return Ok(answer(OfferOutcome::Offered, None));
        }
        // The hold settles itself within its window plus the grace; this is only a backstop.
        let overdue_at = tokio::time::Instant::now() + window + held::GRACE + Duration::from_secs(5);
        let mut said: Option<Progress> = None;
        loop {
            let step = wait_step(
                session.held(hold),
                session.is_closed(),
                self.files_epoch.load(Ordering::SeqCst) != epoch,
                tokio::time::Instant::now() >= overdue_at,
            );
            match step {
                Step::Wait { fetched, total } => {
                    let now = Progress::Waiting {
                        names: names.clone(),
                        phone_open: session.phone_active(),
                        fetched,
                        total,
                    };
                    if said.as_ref() != Some(&now) {
                        progress.send(now.clone());
                        said = Some(now);
                    }
                }
                Step::Answer(outcome, detail) => return Ok(answer(outcome, detail)),
                Step::Withdraw(outcome, detail) => {
                    session.cancel_hold(hold);
                    return Ok(answer(outcome, detail));
                }
            }
            tokio::time::sleep(HOLD_POLL).await;
        }
    }
}

/// The request's switch must be on (checked in Rust before anything runs, whatever the caller
/// is). Pure, so "off means refused" is tested for every switch.
fn switched_on(request: &Request, permissions: &BTreeMap<Permission, bool>) -> Result<(), BridgeError> {
    match request.permission() {
        Some(p) if !permissions.get(&p).copied().unwrap_or(false) => Err(BridgeError::new(
            ErrorCode::ToolOff,
            format!("\"{}\" is switched off in tug › Settings › Developer tools.", p.label()),
        )),
        _ => Ok(()),
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
            Request::SendText { .. } => send_text_time_limit(),
            Request::OfferFiles { hold_secs, .. } => offer_time_limit(*hold_secs),
            _ => Duration::from_secs(15),
        }
    }

    fn rejected(&self, client: &ClientInfo, why: ErrorCode) {
        log::warn!("devtools: refused a call from {:?} ({why:?})", client.display_name());
    }

    fn handle(
        &self,
        client: ClientInfo,
        request: Request,
        progress: ProgressSink,
    ) -> BoxFuture<'_, Result<serde_json::Value, BridgeError>> {
        Box::pin(async move {
            let tool = request.tool();
            let who = format!("{:?} {:?}", client.kind, client.display_name());
            if let Err(e) = switched_on(&request, &self.permissions()) {
                log::info!("devtools: {tool} by {who}: switched off");
                return Err(e);
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
            let result = tools::run(self, &client, request, &progress).await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::Status;

    #[test]
    fn a_text_still_waiting_on_the_phone_is_queued_never_failed() {
        let (outcome, detail) = approved_outcome(Delivery::StillQueued);
        assert_eq!(outcome, SendOutcome::Queued);
        let detail = detail.unwrap();
        assert!(detail.contains("Don't send it again"), "{detail}");
        // A row the worker left pending reads the same way.
        assert_eq!(
            approved_outcome(Delivery::Stored(Status::Pending)).0,
            SendOutcome::Queued
        );
    }

    #[test]
    fn the_phones_answer_maps_to_the_outcome() {
        assert_eq!(
            approved_outcome(Delivery::Stored(Status::Accepted)),
            (SendOutcome::Sent, None)
        );
        assert_eq!(
            approved_outcome(Delivery::Stored(Status::Sent)),
            (SendOutcome::Sent, None)
        );
        let (o, d) = approved_outcome(Delivery::Stored(Status::Unconfirmed));
        assert_eq!(o, SendOutcome::Sent);
        assert!(d.unwrap().contains("Don't send it again"));
        let (o, d) = approved_outcome(Delivery::Stored(Status::Failed));
        assert_eq!(o, SendOutcome::Failed);
        assert!(d.unwrap().contains("Retry"));
    }

    #[test]
    fn nothing_saved_is_failed_with_the_reason() {
        assert_eq!(
            approved_outcome(Delivery::NotSaved("Message service isn't running".into())),
            (SendOutcome::Failed, Some("Message service isn't running".into()))
        );
    }

    #[test]
    fn sending_files_is_refused_until_its_switch_is_on() {
        let offer = tug_bridge::protocol::Call::OfferFiles {
            paths: vec![r"C:\Users\me\report.pdf".into()],
            stdin: None,
            wait_secs: None,
            progress: true,
        }
        .validate(now_ms())
        .unwrap();
        // Never touched: off by default, refused before any card or file check.
        let defaults = effective_permissions(&BTreeMap::new());
        let e = switched_on(&offer, &defaults).unwrap_err();
        assert_eq!(e.code, ErrorCode::ToolOff);
        assert!(
            e.message.contains("\"Send files to your iPhone\" is switched off"),
            "{e}"
        );
        // Turned on in Settings: let through (to the card).
        let on = effective_permissions(&BTreeMap::from([("tugboat_send".to_string(), true)]));
        assert!(switched_on(&offer, &on).is_ok());
        // Turned off again.
        let off = effective_permissions(&BTreeMap::from([("tugboat_send".to_string(), false)]));
        assert!(switched_on(&offer, &off).is_err());
        // `status` needs only the master switch.
        assert!(switched_on(&Request::Status, &off).is_ok());
    }

    #[test]
    fn only_send_on_the_card_sends_files() {
        assert_eq!(unapproved(Resolution::Approved), None);
        assert_eq!(unapproved(Resolution::Declined).unwrap().0, OfferOutcome::Declined);
        assert_eq!(unapproved(Resolution::TimedOut).unwrap().0, OfferOutcome::NotConfirmed);
        assert_eq!(unapproved(Resolution::Cancelled).unwrap().0, OfferOutcome::Cancelled);
    }

    #[test]
    fn every_way_the_wait_for_the_phone_ends() {
        let waiting = Verdict::Waiting { fetched: 1, total: 2 };
        assert_eq!(
            wait_step(waiting, false, false, false),
            Step::Wait { fetched: 1, total: 2 }
        );
        assert_eq!(
            wait_step(Verdict::Delivered, false, false, false),
            Step::Answer(OfferOutcome::Delivered, None)
        );
        // The phone got everything: delivered, even if Tugboat closed or the switch went off since.
        assert_eq!(
            wait_step(Verdict::Delivered, true, true, true),
            Step::Answer(OfferOutcome::Delivered, None)
        );
        assert_eq!(
            wait_step(Verdict::TimedOut, false, false, false),
            Step::Answer(OfferOutcome::TimedOut, None)
        );
        assert!(matches!(
            wait_step(Verdict::Withdrawn, false, false, false),
            Step::Answer(OfferOutcome::Cancelled, Some(_))
        ));
        assert!(matches!(
            wait_step(waiting, true, false, false),
            Step::Answer(OfferOutcome::Cancelled, Some(d)) if d.contains("closed")
        ));
        // Switched off: the files are taken back, not left on offer.
        assert!(matches!(
            wait_step(waiting, false, true, false),
            Step::Withdraw(OfferOutcome::Cancelled, Some(d)) if d.contains("switched off")
        ));
        assert_eq!(
            wait_step(waiting, false, false, true),
            Step::Withdraw(OfferOutcome::TimedOut, None)
        );
    }

    #[test]
    fn a_wait_for_files_ends_before_the_bridge_and_the_client_give_up() {
        use tug_bridge::protocol::{DEFAULT_WAIT_SECS, FINISH_GRACE_SECS, MAX_WAIT_SECS};
        assert_eq!(held::GRACE.as_secs(), FINISH_GRACE_SECS);
        for hold in [1, DEFAULT_WAIT_SECS, MAX_WAIT_SECS] {
            let in_tug = Duration::from_millis(CONFIRM_TIMEOUT_MS as u64)
                + Duration::from_secs(u64::from(hold))
                + held::GRACE
                + Duration::from_secs(5);
            assert!(offer_time_limit(hold) > in_tug, "{hold}");
            let call = tug_bridge::protocol::Call::OfferFiles {
                paths: vec![r"C:\a".into()],
                stdin: None,
                wait_secs: Some(hold),
                progress: false,
            };
            assert!(offer_time_limit(hold) < tug_bridge::client::time_limit(&call), "{hold}");
        }
    }

    #[test]
    fn the_wait_ends_before_the_bridge_and_the_client_give_up() {
        let card_and_wait = Duration::from_millis(CONFIRM_TIMEOUT_MS as u64) + SEND_WAIT;
        assert!(send_text_time_limit() > card_and_wait);
        assert!(send_text_time_limit() < tug_bridge::client::SEND_TEXT_TIMEOUT);
    }
}
