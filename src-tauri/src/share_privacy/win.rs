//! The Windows side of hiding previews: one look at the screen ([`snapshot`]), and the
//! `tug-share-watch` thread that keeps the window's banner true.
//!
//! **Cost.** A look is one shell call, one display-config read and one pass over the top-level
//! windows reading class names (a program name only for the handful of classes a rule names):
//! well under a millisecond, with no messages sent to other programs. It runs:
//! - right before each pop-up (so a pop-up is never judged on an old answer);
//! - every 2 s while tug's window is on screen (the window tells us, `watch`);
//! - every 2 s while previews are hidden, so hiding clears as soon as sharing stops, even with
//!   tug in the tray.
//!
//! Otherwise (tug in the tray, nothing hidden) the thread sleeps on a condition variable and
//! never wakes, so idle CPU is unchanged.
//!
//! The decisions are in `mod.rs`; this file only asks Windows and reports.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use windows::core::BOOL;
use windows::core::PWSTR;
use windows::Win32::Devices::Display::{
    GetDisplayConfigBufferSizes, QueryDisplayConfig, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_TOPOLOGY_CLONE, DISPLAYCONFIG_TOPOLOGY_ID, QDC_DATABASE_CURRENT,
};
use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONULL};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::SHQueryUserNotificationState;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible,
};

use super::{
    class_of_interest, covers_monitor, enabled_from_setting, evaluate, needs_title, transition, Rect, ShareState,
    Signal, Snapshot, TopWindow, EVENT, QUNS_APP, QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN, SETTING,
};
use crate::store::Store;

/// How often to look while the window is on screen or previews are hidden.
const POLL: Duration = Duration::from_secs(2);
/// How long a pop-up waits for a fresh look before going with the last answer.
const CHECK_LIMIT: Duration = Duration::from_millis(500);

struct Inner {
    /// Settings › Notifications › Hide previews while sharing or presenting.
    enabled: bool,
    /// tug's window is on screen (the window says so).
    watched: bool,
    /// The last look's strongest signal.
    signal: Option<Signal>,
}

struct Monitor {
    app: AppHandle,
    inner: Mutex<Inner>,
    wake: Condvar,
}

static MONITOR: OnceLock<Monitor> = OnceLock::new();
/// A time-limited look is still running (Windows didn't answer): don't start another.
static HELPER_BUSY: AtomicBool = AtomicBool::new(false);

impl Monitor {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn state(&self) -> ShareState {
        let g = self.lock();
        ShareState::from_signal(if g.enabled { g.signal } else { None })
    }

    /// Record a look's answer: log what changed (the signal's name only) and tell the window if
    /// hiding started or stopped. With the setting off, nothing is hidden whatever the look said.
    fn apply(&self, found: Option<Signal>) {
        let (before, after) = {
            let mut g = self.lock();
            let found = if g.enabled { found } else { None };
            let before = g.signal;
            g.signal = found;
            (before, found)
        };
        if let Some(line) = transition(before, after) {
            log::info!("share privacy: {line}");
        }
        let (was, now) = (ShareState::from_signal(before), ShareState::from_signal(after));
        if was != now {
            if let Err(e) = self.app.emit(EVENT, now) {
                log::warn!("emit {EVENT} failed: {e}");
            }
            // Hiding started: keep looking until it clears, even with the window hidden.
            self.wake.notify_all();
        }
    }
}

/// Start watching (once): read the setting and start the `tug-share-watch` thread, which sleeps
/// until the window is shown or a pop-up finds something to hide.
pub fn start(app: &AppHandle, store: &Store) {
    let enabled = enabled_from_setting(store.setting(SETTING).ok().flatten().as_deref());
    let monitor = Monitor {
        app: app.clone(),
        inner: Mutex::new(Inner {
            enabled,
            watched: false,
            signal: None,
        }),
        wake: Condvar::new(),
    };
    if MONITOR.set(monitor).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new().name("tug-share-watch".into()).spawn(run);
    if let Err(e) = spawned {
        log::warn!("share privacy: watcher unavailable ({e}); pop-ups still check before showing");
    }
}

fn run() {
    let Some(m) = MONITOR.get() else { return };
    loop {
        {
            let mut g = m.lock();
            // Nothing could show a preview right now: sleep until that changes.
            while !(g.enabled && (g.watched || g.signal.is_some())) {
                g = m.wake.wait(g).unwrap_or_else(|e| e.into_inner());
            }
        }
        m.apply(evaluate(&snapshot()));
        let g = m.lock();
        let _ = m.wake.wait_timeout(g, POLL);
    }
}

/// A fresh look, limited in time: a pop-up never waits more than [`CHECK_LIMIT`] on Windows.
/// `None` when Windows didn't answer in time (the last answer stands).
fn look_now() -> Option<Option<Signal>> {
    if HELPER_BUSY.swap(true, Ordering::AcqRel) {
        return None;
    }
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("tug-share-look".into())
        .spawn(move || {
            let found = evaluate(&snapshot());
            HELPER_BUSY.store(false, Ordering::Release);
            let _ = tx.send(found);
        });
    if spawned.is_err() {
        HELPER_BUSY.store(false, Ordering::Release);
        return None;
    }
    match rx.recv_timeout(CHECK_LIMIT) {
        Ok(found) => Some(found),
        Err(_) => {
            log::warn!("share privacy: Windows didn't answer in time; going with the last check");
            None
        }
    }
}

/// Right before a pop-up: should it hide its preview? Looks again rather than trusting the last
/// answer, since tug is usually in the tray when pop-ups matter and nothing was watching.
pub fn previews_hidden_now() -> bool {
    let Some(m) = MONITOR.get() else { return false };
    if !m.lock().enabled {
        return false;
    }
    if let Some(found) = look_now() {
        m.apply(found);
    }
    m.state().active
}

/// tug's window came on screen (`on`) or went away. On screen, look now and every 2 s after, so
/// the banner is right; returns the answer so the window can show it at once.
pub fn watch(on: bool) -> ShareState {
    let Some(m) = MONITOR.get() else {
        return ShareState::default();
    };
    let enabled = {
        let mut g = m.lock();
        g.watched = on;
        g.enabled
    };
    if on && enabled {
        if let Some(found) = look_now() {
            m.apply(found);
        }
    }
    m.wake.notify_all();
    m.state()
}

/// The Settings switch changed. Off: stop looking and show everything again at once.
pub fn set_enabled(on: bool) {
    let Some(m) = MONITOR.get() else { return };
    m.lock().enabled = on;
    if !on {
        m.apply(None);
    }
    m.wake.notify_all();
}

/// One look at the screen. See the module docs for what each part costs.
fn snapshot() -> Snapshot {
    // SAFETY: plain FFI call with no arguments.
    let quns = unsafe { SHQueryUserNotificationState() }.ok().map(|s| s.0);
    let full_screen_answer = matches!(quns, Some(QUNS_BUSY | QUNS_APP | QUNS_RUNNING_D3D_FULL_SCREEN));
    Snapshot {
        quns,
        foreground_full_screen: full_screen_answer && foreground_covers_monitor(),
        duplicating: display_duplicated(),
        windows: windows_of_interest(),
    }
}

fn rect(r: RECT) -> Rect {
    Rect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    // SAFETY: the buffer outlives the call and its length is passed with it.
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// The foreground window covers its whole monitor and isn't the desktop or taskbar.
fn foreground_covers_monitor() -> bool {
    // SAFETY: plain FFI calls; every out-pointer is a local that outlives its call.
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() {
            return false;
        }
        let mut wr = RECT::default();
        if GetWindowRect(fg, &mut wr).is_err() {
            return false;
        }
        let monitor = MonitorFromWindow(fg, MONITOR_DEFAULTTONULL);
        if monitor.is_invalid() {
            return false;
        }
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut mi).as_bool() {
            return false;
        }
        covers_monitor(rect(wr), rect(mi.rcMonitor), &class_name(fg))
    }
}

/// Windows+P "Duplicate": the current display topology is a clone.
fn display_duplicated() -> bool {
    let (mut paths, mut modes) = (0u32, 0u32);
    // SAFETY: out-pointers are locals; the arrays are sized from GetDisplayConfigBufferSizes and
    // their lengths are passed alongside.
    unsafe {
        if GetDisplayConfigBufferSizes(QDC_DATABASE_CURRENT, &mut paths, &mut modes) != ERROR_SUCCESS {
            return false;
        }
        let mut path_info = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
        let mut mode_info = vec![DISPLAYCONFIG_MODE_INFO::default(); modes as usize];
        let mut topology = DISPLAYCONFIG_TOPOLOGY_ID::default();
        let result = QueryDisplayConfig(
            QDC_DATABASE_CURRENT,
            &mut paths,
            path_info.as_mut_ptr(),
            &mut modes,
            mode_info.as_mut_ptr(),
            Some(&mut topology),
        );
        result == ERROR_SUCCESS && topology == DISPLAYCONFIG_TOPOLOGY_CLONE
    }
}

/// The owning program's file name, lowercase ("powerpnt.exe"); empty if it can't be read.
fn exe_name(pid: u32) -> String {
    // SAFETY: the handle is closed before returning; the buffer and its length go together.
    unsafe {
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(process);
        if !ok {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit(['\\', '/']).next().unwrap_or_default().to_lowercase()
    }
}

struct Scan {
    found: Vec<TopWindow>,
    exes: HashMap<u32, String>,
}

/// Visible, not minimized top-level windows of a class some rule names, with their program (and
/// their title only where a rule matches by title).
fn windows_of_interest() -> Vec<TopWindow> {
    let mut scan = Scan {
        found: Vec::new(),
        exes: HashMap::new(),
    };
    // SAFETY: `scan` outlives EnumWindows, which calls `visit` synchronously on this thread.
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut scan as *mut Scan as isize)) };
    scan.found
}

unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: lparam is the `Scan` passed by `windows_of_interest`, alive for the whole enumeration.
    let scan = unsafe { &mut *(lparam.0 as *mut Scan) };
    // SAFETY: plain FFI calls on a window handle Windows just gave us.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return BOOL(1);
        }
        let class = class_name(hwnd);
        if !class_of_interest(&class) {
            return BOOL(1);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let exe = scan.exes.entry(pid).or_insert_with(|| exe_name(pid)).clone();
        let title = needs_title(&class, &exe).then(|| {
            // Another program's title is read without sending it a message, so this can't hang.
            let mut buf = [0u16; 256];
            let n = GetWindowTextW(hwnd, &mut buf);
            String::from_utf16_lossy(&buf[..n.max(0) as usize])
        });
        scan.found.push(TopWindow { class, exe, title });
    }
    BOOL(1)
}

#[cfg(test)]
mod tests {
    /// Timing for the module docs' cost claim, on a real desktop: `cargo test -p tug --lib
    /// share_privacy::win::tests::look_cost -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn look_cost() {
        let rounds = 200;
        let start = std::time::Instant::now();
        let mut last = None;
        for _ in 0..rounds {
            last = Some(super::snapshot());
        }
        let each = start.elapsed() / rounds;
        let s = last.unwrap();
        println!(
            "one look: {each:?} (quns {:?}, duplicating {}, {} windows of interest, signal {:?})",
            s.quns,
            s.duplicating,
            s.windows.len(),
            super::evaluate(&s).map(super::Signal::name)
        );
    }
}
