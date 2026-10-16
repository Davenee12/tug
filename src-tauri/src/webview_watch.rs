//! Watching WebView2 for a failed process, naming it truthfully in the log, and bringing tug's
//! window back when it can be brought back.
//!
//! Tauri doesn't surface WebView2's `ProcessFailed` event, but it hands back the raw controller via
//! `with_webview`, so we register the handler ourselves. The event's `kind` says *which* process
//! failed, and that decides what (if anything) tug has to do:
//!
//! - the page's own process exited or hung → reload the page;
//! - the browser process exited → every WebView2 object in the window is dead (anything sent to it
//!   fails with 0x8007139F), so the window has to be destroyed and built again;
//! - a GPU, utility or other helper process exited → WebView2 restarts it on its own; log only.
//!
//! Both kind-0 (browser process exited) events in the owner's log so far (2026-10-10, 2026-10-13)
//! came seconds after Windows started a restart, when Windows ends WebView2 before tug. Rebuilding
//! the window then would only fight the shutdown, so recovery is skipped while Windows is shutting
//! down, and the log says so rather than calling it a crash.

use std::time::{Duration, Instant};

/// WebView2's `COREWEBVIEW2_PROCESS_FAILED_KIND` values.
mod kind {
    pub const BROWSER_PROCESS_EXITED: i32 = 0;
    pub const RENDER_PROCESS_EXITED: i32 = 1;
    pub const RENDER_PROCESS_UNRESPONSIVE: i32 = 2;
    pub const FRAME_RENDER_PROCESS_EXITED: i32 = 3;
    pub const UTILITY_PROCESS_EXITED: i32 = 4;
    pub const SANDBOX_HELPER_PROCESS_EXITED: i32 = 5;
    pub const GPU_PROCESS_EXITED: i32 = 6;
    pub const PPAPI_PLUGIN_PROCESS_EXITED: i32 = 7;
    pub const PPAPI_BROKER_PROCESS_EXITED: i32 = 8;
    pub const UNKNOWN_PROCESS_EXITED: i32 = 9;
}

/// Which process failed, in words, for a WebView2 failure kind.
fn kind_text(kind: i32) -> &'static str {
    match kind {
        kind::BROWSER_PROCESS_EXITED => "browser process exited",
        kind::RENDER_PROCESS_EXITED => "page (render) process exited",
        kind::RENDER_PROCESS_UNRESPONSIVE => "page (render) process stopped responding",
        kind::FRAME_RENDER_PROCESS_EXITED => "an embedded frame's render process exited",
        kind::UTILITY_PROCESS_EXITED => "utility process exited",
        kind::SANDBOX_HELPER_PROCESS_EXITED => "sandbox helper process exited",
        kind::GPU_PROCESS_EXITED => "GPU process exited",
        kind::PPAPI_PLUGIN_PROCESS_EXITED => "plugin process exited",
        kind::PPAPI_BROKER_PROCESS_EXITED => "plugin broker process exited",
        kind::UNKNOWN_PROCESS_EXITED => "helper process exited",
        _ => "process failed (unrecognised kind)",
    }
}

/// WebView2's `COREWEBVIEW2_PROCESS_FAILED_REASON`, in words (only on runtimes that report it).
fn reason_text(reason: i32) -> &'static str {
    match reason {
        0 => "unexpected",
        1 => "unresponsive",
        2 => "terminated",
        3 => "crashed",
        4 => "failed to launch",
        5 => "out of memory",
        6 => "profile deleted",
        _ => "unrecognised reason",
    }
}

/// What tug does about one failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// The page process is gone or hung: reload the page in the same webview.
    Reload,
    /// The browser process is gone: destroy the window and build it again.
    RecreateWindow,
    /// A hang reported once may still clear by itself: wait for the next report.
    WaitForHang,
    /// WebView2 recovers by itself (helper processes), or it isn't tug's page (frames).
    Nothing,
    /// Windows is shutting down and ended WebView2 on the way: leave it.
    ShuttingDown,
    /// Recovered too often in a short time: stop rather than loop.
    GiveUp,
}

/// How many recoveries are allowed in `WINDOW` before tug stops trying (a page that crashes every
/// time it loads must not reload forever).
const MAX_RECOVERIES: usize = 3;
const WINDOW: Duration = Duration::from_secs(10 * 60);
/// A second hang report within this long means the page really is stuck, not just busy.
const HANG_CONFIRM: Duration = Duration::from_secs(30);

/// Recovery bookkeeping for the main window: recent recoveries (for the cap) and the last hang
/// report (a single report can be a long task that finishes; a second one in a row is a hang).
#[derive(Debug)]
struct Recovery {
    recent: Vec<Instant>,
    last_hang: Option<Instant>,
}

impl Recovery {
    const fn new() -> Self {
        Recovery {
            recent: Vec::new(),
            last_hang: None,
        }
    }

    /// Decide what to do about a failure of `kind` at `now`.
    fn plan(&mut self, kind: i32, shutting_down: bool, now: Instant) -> Action {
        let wanted = match kind {
            kind::BROWSER_PROCESS_EXITED if shutting_down => return Action::ShuttingDown,
            kind::BROWSER_PROCESS_EXITED => Action::RecreateWindow,
            kind::RENDER_PROCESS_EXITED => Action::Reload,
            kind::RENDER_PROCESS_UNRESPONSIVE => {
                let confirmed = self
                    .last_hang
                    .is_some_and(|t| now.saturating_duration_since(t) <= HANG_CONFIRM);
                if !confirmed {
                    self.last_hang = Some(now);
                    return Action::WaitForHang;
                }
                Action::Reload
            }
            _ => return Action::Nothing,
        };
        // Any page or browser recovery starts the hang check afresh.
        self.last_hang = None;
        self.recent.retain(|&t| now.saturating_duration_since(t) < WINDOW);
        if self.recent.len() >= MAX_RECOVERIES {
            return Action::GiveUp;
        }
        self.recent.push(now);
        wanted
    }
}

/// The line logged for one failure: which process, the reason and exit code when the runtime
/// reports them, and what tug is doing about it.
fn describe(kind: i32, reason: Option<i32>, exit_code: Option<i32>, action: Action) -> String {
    let mut detail = format!("kind {kind}");
    if let Some(r) = reason {
        detail.push_str(&format!(", reason: {}", reason_text(r)));
    }
    if let Some(c) = exit_code {
        detail.push_str(&format!(", exit code {c:#x}"));
    }
    let doing = match action {
        Action::Reload => "reloading the window's page",
        Action::RecreateWindow => "rebuilding the window",
        Action::WaitForHang => "waiting to see if it recovers",
        Action::Nothing => "WebView2 restarts it by itself",
        Action::ShuttingDown => "Windows is shutting down, so leaving it",
        Action::GiveUp => "it keeps failing, so not recovering again for now (restart tug to bring the window back)",
    };
    format!("WebView2 {} ({detail}); {doing}", kind_text(kind))
}

#[cfg(windows)]
mod imp {
    use super::{describe, Action, Recovery};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    use tauri::{AppHandle, Manager};
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2ProcessFailedEventArgs, ICoreWebView2ProcessFailedEventArgs2,
        COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_REASON,
    };
    use webview2_com::ProcessFailedEventHandler;
    use windows::core::Interface;

    /// One main window, so one set of recovery bookkeeping.
    static RECOVERY: Mutex<Recovery> = Mutex::new(Recovery::new());
    /// Set while the main window is being rebuilt, so destroying the old one (the only window)
    /// doesn't end tug. Read from the app's run loop.
    static RECREATING: AtomicBool = AtomicBool::new(false);

    pub fn recreating_window() -> bool {
        RECREATING.load(Ordering::SeqCst)
    }

    /// Whether this Windows session is shutting down or restarting.
    fn windows_shutting_down() -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SHUTTINGDOWN};
        // SAFETY: GetSystemMetrics only reads a system value; no pointers are involved.
        unsafe { GetSystemMetrics(SM_SHUTTINGDOWN) != 0 }
    }

    pub fn watch(window: &tauri::WebviewWindow) {
        let app = window.app_handle().clone();
        let result = window.with_webview(move |webview| {
            if let Err(e) = install(&webview, app) {
                log::warn!("WebView2 crash watch unavailable: {}", e.message());
            }
        });
        if let Err(e) = result {
            log::warn!("WebView2 crash watch unavailable: {e}");
        }
    }

    fn install(webview: &tauri::webview::PlatformWebview, app: AppHandle) -> windows::core::Result<()> {
        // SAFETY: the controller and core WebView2 come straight from Tauri's live webview. The
        // handler reads the event args through COM out-param getters and, for a page failure,
        // calls Reload on the sender it was handed; nothing COM is kept across calls.
        unsafe {
            let core: ICoreWebView2 = webview.controller().CoreWebView2()?;
            let handler = ProcessFailedEventHandler::create(Box::new(
                move |sender: Option<ICoreWebView2>, args: Option<ICoreWebView2ProcessFailedEventArgs>| {
                    let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND(-1);
                    let (mut reason, mut exit_code) = (None, None);
                    if let Some(a) = args.as_ref() {
                        let _ = a.ProcessFailedKind(&mut kind);
                        // Reason and exit code tell a crash from a kill or out-of-memory; older
                        // runtimes don't have them.
                        if let Ok(a2) = a.cast::<ICoreWebView2ProcessFailedEventArgs2>() {
                            let mut r = COREWEBVIEW2_PROCESS_FAILED_REASON(-1);
                            if a2.Reason(&mut r).is_ok() {
                                reason = Some(r.0);
                            }
                            let mut code = 0i32;
                            if a2.ExitCode(&mut code).is_ok() {
                                exit_code = Some(code);
                            }
                        }
                    }
                    let shutting_down = windows_shutting_down();
                    let action = RECOVERY
                        .lock()
                        .map(|mut r| r.plan(kind.0, shutting_down, Instant::now()))
                        .unwrap_or(Action::Nothing);
                    let line = describe(kind.0, reason, exit_code, action);
                    match action {
                        Action::ShuttingDown | Action::Nothing | Action::WaitForHang => log::info!("{line}"),
                        Action::GiveUp => log::error!("{line}"),
                        Action::Reload | Action::RecreateWindow => log::warn!("{line}"),
                    }
                    match action {
                        Action::Reload => {
                            if let Some(core) = sender {
                                if let Err(e) = core.Reload() {
                                    log::warn!("WebView2 reload failed: {}", e.message());
                                }
                            }
                        }
                        Action::RecreateWindow => {
                            let app = app.clone();
                            // Off the WebView2 callback (and off the main thread, which has to
                            // process the old window's teardown before the new one can be built).
                            std::thread::spawn(move || recreate_window(app));
                        }
                        _ => {}
                    }
                    Ok(())
                },
            ));
            // The registration token is only needed to unsubscribe, which we never do (the handler
            // lives as long as the webview); keep it on the stack.
            let mut token: i64 = 0;
            core.add_ProcessFailed(&handler, &mut token)?;
        }
        Ok(())
    }

    /// Destroy the main window (its WebView2 is dead) and build it again from the app config, in
    /// the same place and showing if it was showing.
    fn recreate_window(app: AppHandle) {
        // Windows can end WebView2 a moment before it marks the session as shutting down: give it
        // that moment, then check again.
        std::thread::sleep(Duration::from_secs(2));
        if windows_shutting_down() {
            log::info!("Windows is shutting down; not rebuilding the window");
            return;
        }
        let Some(old) = app.get_webview_window("main") else {
            return;
        };
        let Some(config) = app.config().app.windows.iter().find(|w| w.label == "main").cloned() else {
            log::error!("can't rebuild the window: no \"main\" window in the app config");
            return;
        };
        // The native window is fine; only its web content is gone, so these still answer.
        let visible = old.is_visible().unwrap_or(false);
        let maximized = old.is_maximized().unwrap_or(false);
        let position = old.outer_position().ok();
        let size = old.inner_size().ok();

        RECREATING.store(true, Ordering::SeqCst);
        if let Err(e) = old.destroy() {
            RECREATING.store(false, Ordering::SeqCst);
            log::error!("can't rebuild the window: closing the broken one failed: {e}");
            return;
        }
        drop(old);
        // The old window's label is freed once the run loop has processed the teardown.
        for _ in 0..50 {
            if app.get_webview_window("main").is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let built = tauri::WebviewWindowBuilder::from_config(&app, &config).and_then(|b| b.build());
        RECREATING.store(false, Ordering::SeqCst);
        let window = match built {
            Ok(w) => w,
            Err(e) => {
                log::error!("can't rebuild the window: {e}");
                return;
            }
        };
        watch(&window);
        if let Some(size) = size {
            let _ = window.set_size(size);
        }
        if let Some(position) = position {
            let _ = window.set_position(position);
        }
        if maximized {
            let _ = window.maximize();
        }
        if visible {
            crate::tray::show(&app);
        }
        log::info!("window rebuilt after WebView2's browser process exited");
    }
}

/// Register WebView2's `ProcessFailed` handler on the main window: log each failure truthfully and
/// recover the window where that's possible. Best-effort: if the controller can't be reached, that
/// is logged once and tug runs on without the watch.
#[cfg(windows)]
pub use imp::watch;

/// Whether the main window is being rebuilt right now. While it is, the run loop must not treat
/// "the last window closed" as a reason to quit.
#[cfg(windows)]
pub fn recreating_window() -> bool {
    imp::recreating_window()
}

/// Off Windows there's no WebView2 to watch; the call is a no-op so the setup path stays identical.
#[cfg(not(windows))]
pub fn watch(_window: &tauri::WebviewWindow) {}

#[cfg(not(windows))]
pub fn recreating_window() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, secs: u64) -> Instant {
        start + Duration::from_secs(secs)
    }

    #[test]
    fn every_documented_kind_is_named_for_the_process_it_is() {
        assert_eq!(kind_text(0), "browser process exited");
        assert_eq!(kind_text(1), "page (render) process exited");
        assert_eq!(kind_text(2), "page (render) process stopped responding");
        assert_eq!(kind_text(3), "an embedded frame's render process exited");
        assert_eq!(kind_text(4), "utility process exited");
        assert_eq!(kind_text(5), "sandbox helper process exited");
        assert_eq!(kind_text(6), "GPU process exited");
        assert_eq!(kind_text(7), "plugin process exited");
        assert_eq!(kind_text(8), "plugin broker process exited");
        assert_eq!(kind_text(9), "helper process exited");
        assert_eq!(kind_text(42), "process failed (unrecognised kind)");
        assert_eq!(kind_text(-1), "process failed (unrecognised kind)");
    }

    #[test]
    fn browser_exit_is_not_called_a_page_crash() {
        let line = describe(0, Some(2), Some(1), Action::ShuttingDown);
        assert!(line.starts_with("WebView2 browser process exited"), "{line}");
        assert!(!line.contains("crashed or hung"), "{line}");
        assert!(line.contains("reason: terminated"), "{line}");
        assert!(line.contains("exit code 0x1"), "{line}");
        assert!(line.contains("Windows is shutting down"), "{line}");
    }

    #[test]
    fn reason_and_exit_code_are_left_out_when_the_runtime_has_none() {
        assert_eq!(
            describe(6, None, None, Action::Nothing),
            "WebView2 GPU process exited (kind 6); WebView2 restarts it by itself"
        );
        assert_eq!(reason_text(5), "out of memory");
        assert_eq!(reason_text(3), "crashed");
        assert_eq!(reason_text(99), "unrecognised reason");
    }

    #[test]
    fn page_crash_reloads_and_browser_exit_rebuilds_the_window() {
        let now = Instant::now();
        assert_eq!(Recovery::new().plan(1, false, now), Action::Reload);
        assert_eq!(Recovery::new().plan(0, false, now), Action::RecreateWindow);
    }

    #[test]
    fn browser_exit_during_windows_shutdown_is_left_alone() {
        let mut r = Recovery::new();
        assert_eq!(r.plan(0, true, Instant::now()), Action::ShuttingDown);
        assert!(r.recent.is_empty(), "a shutdown isn't counted as a recovery");
    }

    #[test]
    fn helper_and_frame_failures_need_nothing_from_tug() {
        let now = Instant::now();
        for k in [3, 4, 5, 6, 7, 8, 9, 42] {
            assert_eq!(Recovery::new().plan(k, false, now), Action::Nothing, "kind {k}");
        }
    }

    #[test]
    fn a_hang_is_only_acted_on_when_reported_twice_in_a_row() {
        let start = Instant::now();
        let mut r = Recovery::new();
        assert_eq!(r.plan(2, false, start), Action::WaitForHang);
        assert_eq!(r.plan(2, false, at(start, 10)), Action::Reload);
        // After a reload, the next hang starts the check afresh.
        assert_eq!(r.plan(2, false, at(start, 20)), Action::WaitForHang);
        // Reports far apart are two separate busy spells, not one hang.
        assert_eq!(r.plan(2, false, at(start, 200)), Action::WaitForHang);
    }

    #[test]
    fn repeated_failures_stop_recovering_instead_of_looping() {
        let start = Instant::now();
        let mut r = Recovery::new();
        for i in 0..MAX_RECOVERIES as u64 {
            assert_eq!(r.plan(1, false, at(start, i)), Action::Reload);
        }
        assert_eq!(r.plan(1, false, at(start, 5)), Action::GiveUp);
        assert_eq!(r.plan(0, false, at(start, 6)), Action::GiveUp);
        // Once the window has passed, recovery is allowed again.
        assert_eq!(r.plan(1, false, at(start, WINDOW.as_secs() + 10)), Action::Reload);
    }
}
