//! Automatic updates, the way Teams does them: tug checks GitHub in the background, downloads a
//! new release quietly (its signature checked by `tauri-plugin-updater` against the public key in
//! tauri.conf.json), and then the window shows a small Update button. One click, a confirm, and
//! tug runs the NSIS installer in update mode and comes back on the same screen.
//!
//! - **Off until set up.** Without a real public key in tauri.conf.json (`policy::pubkey_configured`)
//!   nothing here touches the network, and Settings › About says updates aren't set up. Debug
//!   builds never check either.
//! - **Never interrupts.** tug never restarts on its own; Restart now waits while a call rings, a
//!   Tugboat transfer runs or voice typing is busy. Quitting from the tray with an update ready
//!   applies it without starting tug again (the tray item says so).
//! - **Never bricks.** A failed check, download or signature leaves tug running as it was; it's
//!   logged and tried again later. Releases that aren't newer are refused.
//! - **Portable exe:** it can't run an installer, so it only says a new version is out and opens
//!   the release page.
//! - **Only GitHub.** The check and the download go to github.com (and GitHub's download servers),
//!   from Rust, so the window's CSP doesn't change. The webview has no updater permissions.
//!
//! The decisions are in `policy` (pure, tested); this file is the bookkeeping and the plugin calls.

pub mod policy;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::state::{now_ms, RevCounter};
use policy::{Blocker, Busy, Failure, Phase, Refusal};

/// The event carrying `UpdateStatus` to the window.
pub const EVENT: &str = "update-status";
/// "Download updates automatically" (Settings › About): on unless switched off.
const AUTO_SETTING: &str = "update.auto";
/// The screen to reopen after an update restart; set just before the installer runs, taken once
/// by the window when it loads.
const RESUME_SETTING: &str = "update.resume";
/// How long a check may take before it counts as failed.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the download may take (an installer is ~10 MB; slow links get plenty of time).
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// How often the schedule looks at the clock. Cheap: it only compares times.
const TICK: Duration = Duration::from_secs(60);

/// What the window shows: the About section and the Update button read this.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub rev: u64,
    pub phase: Phase,
    /// The version running now.
    pub current: String,
    /// The newer release found (Available, Downloading, Ready, Applying).
    pub version: Option<String>,
    /// The release notes' first paragraph, for the confirm.
    pub headline: Option<String>,
    /// Download progress in percent, when GitHub says the size.
    pub progress: Option<u8>,
    /// What failed, while `phase` is Failed.
    pub failure: Option<Failure>,
    /// The portable exe: it can only point at the download.
    pub portable: bool,
    /// "Download updates automatically".
    pub auto: bool,
    /// When the last check finished (ms since the epoch), if one has this run.
    pub checked_at: Option<i64>,
}

/// Restart now's answer: it either restarts (and this never arrives) or says why not.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyAnswer {
    /// Something an update restart would cut off; the button stays.
    pub blocked: Option<Blocker>,
    pub status: UpdateStatus,
}

struct State {
    phase: Phase,
    version: Option<String>,
    headline: Option<String>,
    progress: Option<u8>,
    failure: Option<Failure>,
    checked_at: Option<i64>,
    /// A newer release that was found but not downloaded (automatic downloads off).
    found: Option<Update>,
    /// The downloaded, verified installer and the release it belongs to.
    ready: Option<(Update, Vec<u8>)>,
    last_attempt_ms: Option<i64>,
    last_failed: bool,
}

struct Inner {
    app: AppHandle,
    configured: bool,
    portable: bool,
    current: String,
    started_ms: i64,
    state: Mutex<State>,
    revs: RevCounter,
    /// Check for updates and switching automatic updates on wake the schedule early.
    wake: tokio::sync::Notify,
}

#[derive(Clone)]
pub struct Updater {
    inner: Arc<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The public key tauri.conf.json carries for the updater plugin (empty when not set up).
fn configured_pubkey(app: &AppHandle) -> String {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .unwrap_or_default()
        .to_string()
}

/// Whether this exe is the installed copy (NSIS's uninstaller lives beside it) or the portable one.
fn running_portable() -> bool {
    let uninstaller = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("uninstall.exe")))
        .is_some_and(|p| p.is_file());
    policy::is_portable(uninstaller)
}

/// The plugin, registered in lib.rs. Its own default comparator is tug's (never a downgrade), so
/// even its JavaScript commands, which the webview has no permission to call, couldn't downgrade.
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry, tauri_plugin_updater::Config> {
    tauri_plugin_updater::Builder::new()
        .default_version_comparator(|current, release| policy::is_newer_version(&current, &release.version))
        .build()
}

impl Updater {
    pub fn new(app: AppHandle) -> Updater {
        let configured = policy::updates_enabled(cfg!(debug_assertions), &configured_pubkey(&app));
        let current = app.package_info().version.to_string();
        let portable = running_portable();
        log::info!(
            "updates: {}",
            match (configured, portable) {
                (false, _) => "not set up for this build",
                (true, true) => "portable copy: will point at new releases",
                (true, false) => "set up",
            }
        );
        Updater {
            inner: Arc::new(Inner {
                app,
                configured,
                portable,
                current,
                started_ms: now_ms(),
                state: Mutex::new(State {
                    phase: if configured { Phase::Idle } else { Phase::NotConfigured },
                    version: None,
                    headline: None,
                    progress: None,
                    failure: None,
                    checked_at: None,
                    found: None,
                    ready: None,
                    last_attempt_ms: None,
                    last_failed: false,
                }),
                revs: RevCounter::default(),
                wake: tokio::sync::Notify::new(),
            }),
        }
    }

    fn auto(&self) -> bool {
        auto_from_setting(
            self.store()
                .and_then(|s| s.setting(AUTO_SETTING).ok().flatten())
                .as_deref(),
        )
    }

    fn store(&self) -> Option<Arc<crate::store::Store>> {
        self.inner
            .app
            .try_state::<crate::commands::AppState>()
            .map(|s| s.shared.store.clone())
    }

    fn compose(&self, rev: u64) -> UpdateStatus {
        let s = lock(&self.inner.state);
        UpdateStatus {
            rev,
            phase: s.phase,
            current: self.inner.current.clone(),
            version: s.version.clone(),
            headline: s.headline.clone(),
            progress: s.progress,
            failure: s.failure,
            portable: self.inner.portable,
            auto: self.auto(),
            checked_at: s.checked_at,
        }
    }

    pub fn status(&self) -> UpdateStatus {
        self.inner.revs.stamp(|rev| self.compose(rev), |_| ())
    }

    fn emit(&self) -> UpdateStatus {
        self.inner.revs.stamp(
            |rev| self.compose(rev),
            |s| {
                if let Err(e) = self.inner.app.emit(EVENT, s) {
                    log::warn!("emit {EVENT} failed: {e}");
                }
            },
        )
    }

    /// Change the state and tell the window.
    fn set(&self, f: impl FnOnce(&mut State)) -> UpdateStatus {
        f(&mut lock(&self.inner.state));
        let ready = lock(&self.inner.state).phase == Phase::Ready;
        crate::tray::set_update_ready(ready && !self.inner.portable);
        self.emit()
    }

    /// The background schedule: a first check shortly after startup, then every 6 hours (an hour
    /// after a failure), only while automatic updates are on. Nothing at all when not set up.
    pub fn start(&self) {
        if !self.inner.configured {
            return;
        }
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(TICK) => {}
                    _ = me.inner.wake.notified() => {}
                }
                let (phase, last, failed) = {
                    let s = lock(&me.inner.state);
                    (s.phase, s.last_attempt_ms, s.last_failed)
                };
                let due = policy::check_due(now_ms(), me.inner.started_ms, last, failed);
                if policy::background_check(phase, me.auto(), due) {
                    me.check().await;
                }
            }
        });
    }

    /// Check for updates now (Settings › About). Answers at once with Checking; the outcome comes
    /// as `update-status`.
    pub fn check_now(&self) -> UpdateStatus {
        if !self.inner.configured || !policy::manual_check(lock(&self.inner.state).phase) {
            return self.status();
        }
        let status = self.set(|s| {
            s.phase = Phase::Checking;
            s.failure = None;
        });
        let me = self.clone();
        tauri::async_runtime::spawn(async move { me.check().await });
        status
    }

    /// One check, and the download it leads to.
    async fn check(&self) {
        {
            let mut s = lock(&self.inner.state);
            // A manual check already set Checking; the schedule only gets here from a quiet phase.
            if !matches!(s.phase, Phase::Checking) && !policy::manual_check(s.phase) {
                return;
            }
            s.phase = Phase::Checking;
            s.failure = None;
        }
        self.emit();
        let result = match self.updater() {
            Ok(u) => u.check().await.map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        let now = now_ms();
        let found = match result {
            Ok(found) => found,
            Err(e) => {
                log::warn!("update check failed: {e}");
                self.set(|s| {
                    s.phase = Phase::Failed;
                    s.failure = Some(Failure::Check);
                    s.last_attempt_ms = Some(now);
                    s.last_failed = true;
                });
                return;
            }
        };
        // The comparator already refused anything not newer; checked again here so a downgrade
        // can't get through whatever the plugin does.
        let found = found.filter(|u| {
            let newer = policy::is_newer(&self.inner.current, &u.version);
            if !newer {
                log::warn!("update: refused {} (not newer than {})", u.version, self.inner.current);
            }
            newer
        });
        // The installer must come from tug's own releases on GitHub, whatever latest.json says.
        if let Some(u) = found
            .as_ref()
            .filter(|u| !policy::trusted_download(u.download_url.as_str()))
        {
            log::warn!(
                "update: refused {}: its download isn't on tug's GitHub releases",
                u.version
            );
            self.set(|s| {
                s.phase = Phase::Failed;
                s.failure = Some(Failure::Check);
                s.last_attempt_ms = Some(now);
                s.last_failed = true;
            });
            return;
        }
        let next = policy::after_check(found.is_some(), self.auto(), self.inner.portable);
        if let Some(u) = &found {
            log::info!("update: tug {} is available", u.version);
        }
        self.set(|s| {
            s.phase = next.phase;
            s.checked_at = Some(now);
            s.last_attempt_ms = Some(now);
            s.last_failed = false;
            s.version = found.as_ref().map(|u| u.version.clone());
            s.headline = policy::headline(found.as_ref().and_then(|u| u.body.as_deref()));
            s.found = if next.download { None } else { found.clone() };
        });
        if let (true, Some(update)) = (next.download, found) {
            self.download(update).await;
        }
    }

    /// Download (Settings › About, with automatic downloads off): the release Check found.
    pub fn download_now(&self) -> UpdateStatus {
        let update = {
            let mut s = lock(&self.inner.state);
            if self.inner.portable || s.phase != Phase::Available {
                None
            } else {
                s.found.take()
            }
        };
        let Some(update) = update else { return self.status() };
        let status = self.set(|s| {
            s.phase = Phase::Downloading;
            s.progress = None;
        });
        let me = self.clone();
        tauri::async_runtime::spawn(async move { me.download(update).await });
        status
    }

    async fn download(&self, mut update: Update) {
        self.set(|s| {
            s.phase = Phase::Downloading;
            s.progress = None;
            s.failure = None;
        });
        update.timeout = Some(DOWNLOAD_TIMEOUT);
        let mut done: u64 = 0;
        let mut shown: Option<u8> = None;
        // `download` checks the signature against tauri.conf.json's public key (and that it was
        // signed for this version) before it hands the bytes back.
        let result = update
            .download(
                |chunk, total| {
                    done += chunk as u64;
                    let pct = policy::percent(done, total);
                    if pct != shown {
                        shown = pct;
                        self.set(|s| s.progress = pct);
                    }
                },
                || {},
            )
            .await;
        match result {
            Ok(bytes) => {
                log::info!(
                    "update: tug {} downloaded and verified ({} bytes)",
                    update.version,
                    bytes.len()
                );
                self.set(|s| {
                    s.phase = Phase::Ready;
                    s.progress = None;
                    s.ready = Some((update, bytes));
                });
            }
            Err(e) => {
                log::warn!("update download failed: {e}");
                self.set(|s| {
                    s.phase = Phase::Failed;
                    s.failure = Some(Failure::Download);
                    s.progress = None;
                    s.last_attempt_ms = Some(now_ms());
                    s.last_failed = true;
                });
            }
        }
    }

    /// "Download updates automatically".
    pub fn set_auto(&self, on: bool) -> Result<UpdateStatus, String> {
        let store = self.store().ok_or("tug isn't ready yet")?;
        store
            .set_setting(AUTO_SETTING, if on { "true" } else { "false" })
            .map_err(|e| e.to_string())?;
        if on {
            self.inner.wake.notify_one();
        }
        Ok(self.emit())
    }

    /// Restart now. Refused (with the reason) while something would be cut off; otherwise saves
    /// the screen to come back to, tidies up like Quit, starts the installer and exits.
    pub fn apply(&self, resume: Option<String>) -> Result<ApplyAnswer, String> {
        let (phase, version) = {
            let s = lock(&self.inner.state);
            (s.phase, s.version.clone())
        };
        let busy = busy(&self.inner.app);
        match policy::can_apply(
            phase,
            self.inner.portable,
            &self.inner.current,
            version.as_deref(),
            busy,
        ) {
            Ok(()) => {}
            Err(Refusal::Busy(b)) => {
                log::info!("update: restart held ({b:?})");
                return Ok(ApplyAnswer {
                    blocked: Some(b),
                    status: self.status(),
                });
            }
            Err(Refusal::Downgrade) => {
                log::warn!("update: refused to apply {version:?} over {}", self.inner.current);
                self.set(|s| {
                    s.ready = None;
                    s.phase = Phase::UpToDate;
                    s.version = None;
                });
                return Err("This update isn't newer than the tug you have.".into());
            }
            Err(Refusal::NotReady | Refusal::Portable) => return Err("There's no update ready to install.".into()),
        }
        let Some((update, bytes)) = lock(&self.inner.state).ready.take() else {
            return Err("There's no update ready to install.".into());
        };
        let store = self.store();
        if let (Some(store), Some(screen)) = (&store, resume.filter(|r| policy::valid_resume(r))) {
            let _ = store.set_setting(RESUME_SETTING, &screen);
        }
        self.set(|s| s.phase = Phase::Applying);
        log::info!("update: installing tug {} and restarting", update.version);
        log::logger().flush();
        // Exits the process once the installer has started (after `on_before_exit` tidies up).
        let err = match update.install(&bytes) {
            Ok(()) => {
                return Ok(ApplyAnswer {
                    blocked: None,
                    status: self.status(),
                })
            }
            Err(e) => e.to_string(),
        };
        log::warn!("update install failed: {err}");
        if policy::after_failed_install(EXIT_STARTED.load(Ordering::SeqCst)) == policy::AfterFailedInstall::Restart {
            // The tidy-up for the installer already ran (tray icon gone, windows hidden, tidy-up
            // latched): staying would leave tug running with no way to reach it. Start again; the
            // saved screen stays, so the window comes back where it was, and a later check retries.
            log::warn!("update: the installer didn't start after tug had tidied up; starting tug again");
            log::logger().flush();
            self.inner.app.restart();
        }
        if let Some(store) = &store {
            let _ = store.delete_setting(RESUME_SETTING);
        }
        self.set(|s| {
            s.phase = Phase::Failed;
            s.failure = Some(Failure::Apply);
            s.last_attempt_ms = Some(now_ms());
            s.last_failed = true;
        });
        Err("tug couldn't start the update. It will try again later.".into())
    }

    /// Quitting from the tray with an update downloaded: install it now, without starting tug again
    /// (the tray item said "Quit and update tug"). Called from the exit handler after tidying up.
    pub fn apply_on_quit(&self, session_ending: bool) {
        let ready = {
            let mut s = lock(&self.inner.state);
            if !policy::apply_on_quit(s.phase, self.inner.portable, session_ending) {
                return;
            }
            s.ready.take()
        };
        let Some((update, bytes)) = ready else { return };
        if !policy::is_newer(&self.inner.current, &update.version) {
            return;
        }
        log::info!("update: installing tug {} on quit", update.version);
        log::logger().flush();
        if let Err(e) = update.restart_after_install(false).install(&bytes) {
            log::warn!("update on quit failed: {e}");
        }
    }

    /// The release page: the new version's when there is one, else this version's.
    pub fn release_page(&self) -> Option<String> {
        let version = lock(&self.inner.state).version.clone();
        policy::release_page(version.as_deref().unwrap_or(&self.inner.current))
    }

    fn updater(&self) -> Result<tauri_plugin_updater::Updater, String> {
        let app = self.inner.app.clone();
        // GitHub only, whatever else a config might carry.
        let endpoint: tauri::Url = policy::ENDPOINT.parse().map_err(|e| format!("{e}"))?;
        self.inner
            .app
            .updater_builder()
            .endpoints(vec![endpoint])
            .map_err(|e| e.to_string())?
            .version_comparator(|current, release| policy::is_newer_version(&current, &release.version))
            .timeout(CHECK_TIMEOUT)
            // Right before the installer starts and tug exits: the same tidy-up as Quit (pop-ups,
            // Tugboat, PC audio), then Tauri's own.
            .on_before_exit(move || {
                EXIT_STARTED.store(true, Ordering::SeqCst);
                crate::tidy_before_exit(&app, false);
                app.cleanup_before_exit();
            })
            .build()
            .map_err(|e| e.to_string())
    }
}

/// "update.auto" → on unless it says "false".
fn auto_from_setting(value: Option<&str>) -> bool {
    value != Some("false")
}

/// What an update restart would cut off right now.
fn busy(app: &AppHandle) -> Busy {
    let state = app.try_state::<crate::commands::AppState>();
    let call = state.as_ref().is_some_and(|s| {
        let live = s.shared.live_session();
        s.shared
            .store
            .recent(25, None, live.as_deref())
            .is_ok_and(|rows| rows.iter().any(|n| n.category == "incomingCall" && n.live))
    });
    Busy {
        call,
        transfer: state.as_ref().is_some_and(|s| s.tugboat.transferring()),
        voice: crate::voice::busy(),
    }
}

/// Whether the last run saved a screen to come back to (an update restart): the window shows even
/// if tug was started minimized.
pub fn resume_pending(store: &crate::store::Store) -> bool {
    store.setting(RESUME_SETTING).ok().flatten().is_some()
}

/// Run once per exit, whichever way tug leaves (Quit, Windows ending the session, an update).
static TIDIED: AtomicBool = AtomicBool::new(false);

/// The updater's `on_before_exit` ran: tug tidied up for the installer and expects to exit.
static EXIT_STARTED: AtomicBool = AtomicBool::new(false);

/// Whether this is the first call (the caller should tidy up).
pub fn first_tidy() -> bool {
    !TIDIED.swap(true, Ordering::SeqCst)
}

// ---- Commands ----

type CmdResult<T> = std::result::Result<T, String>;

fn service(app: &AppHandle) -> CmdResult<Updater> {
    app.try_state::<Updater>()
        .map(|u| u.inner().clone())
        .ok_or_else(|| "tug isn't ready yet".to_string())
}

#[tauri::command]
pub fn update_status(app: AppHandle) -> CmdResult<UpdateStatus> {
    Ok(service(&app)?.status())
}

#[tauri::command]
pub fn update_check(app: AppHandle) -> CmdResult<UpdateStatus> {
    Ok(service(&app)?.check_now())
}

#[tauri::command]
pub fn update_download(app: AppHandle) -> CmdResult<UpdateStatus> {
    Ok(service(&app)?.download_now())
}

#[tauri::command]
pub fn update_set_auto(app: AppHandle, on: bool) -> CmdResult<UpdateStatus> {
    service(&app)?.set_auto(on)
}

/// `resume` is the screen to come back to ("messages", "settings:about"); anything else is dropped.
#[tauri::command]
pub fn update_apply(app: AppHandle, resume: Option<String>) -> CmdResult<ApplyAnswer> {
    service(&app)?.apply(resume)
}

#[tauri::command]
pub fn update_open_release_page(app: AppHandle) -> CmdResult<()> {
    let url = service(&app)?
        .release_page()
        .ok_or("No release page for this version")?;
    crate::commands::open_in_browser(&url)
}

/// The screen saved before an update restart, once (it's cleared as it's read).
#[tauri::command]
pub fn update_take_resume(app: AppHandle) -> CmdResult<Option<String>> {
    let state = app
        .try_state::<crate::commands::AppState>()
        .ok_or("tug isn't ready yet")?;
    let store = &state.shared.store;
    let screen = store.setting(RESUME_SETTING).map_err(|e| e.to_string())?;
    if screen.is_some() {
        store.delete_setting(RESUME_SETTING).map_err(|e| e.to_string())?;
    }
    Ok(screen.filter(|s| policy::valid_resume(s)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_updates_are_on_unless_switched_off() {
        assert!(auto_from_setting(None));
        assert!(auto_from_setting(Some("true")));
        assert!(auto_from_setting(Some("")));
        assert!(!auto_from_setting(Some("false")));
    }

    #[test]
    fn tauri_conf_carries_the_updater_but_no_key_yet_means_off() {
        // The shipped config: the endpoint and settings are there; until the owner pastes the
        // public key (docs/RELEASING.md) the pubkey is empty and tug never checks.
        let conf: serde_json::Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let updater = &conf["plugins"]["updater"];
        assert_eq!(updater["endpoints"][0], policy::ENDPOINT);
        assert_eq!(
            updater["requireSignedVersion"], true,
            "a signature must bind its version"
        );
        assert_ne!(updater["allowDowngrades"], true);
        assert_eq!(updater["windows"]["installMode"], "passive");
        let key = updater["pubkey"].as_str().expect("pubkey is a string");
        assert!(
            key.is_empty() || policy::pubkey_configured(key),
            "pubkey is empty or a real public key"
        );
        // The webview gets no updater permission: the work is in Rust.
        let caps = include_str!("../../capabilities/default.json");
        assert!(!caps.contains("updater"));
    }

    #[test]
    fn the_tidy_up_runs_once() {
        // An update's exit tidies up in `on_before_exit` and the exit handler may run after it.
        first_tidy();
        assert!(!first_tidy());
    }
}
