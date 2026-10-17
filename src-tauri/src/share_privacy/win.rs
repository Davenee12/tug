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
//! **Failing closed.** Every look runs on a short-lived `tug-share-look` helper and nothing waits
//! on it for more than 500 ms. No answer in time, a look stuck for 2 s (a fresh one is started;
//! at most three can be stuck), or a failed call with nothing found all mean "couldn't tell":
//! with the setting on, the pop-up hides its preview and the window blurs with a banner that
//! says why, until a look can tell.
//!
//! The decisions are in `mod.rs`; this file only asks Windows and reports.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use windows::core::BOOL;
use windows::core::PWSTR;
use windows::Win32::Devices::Display::{
    GetDisplayConfigBufferSizes, QueryDisplayConfig, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_TOPOLOGY_CLONE, DISPLAYCONFIG_TOPOLOGY_ID, QDC_DATABASE_CURRENT,
};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, HWND, LPARAM, RECT, WIN32_ERROR,
};
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
    class_of_interest, covers_monitor, enabled_from_setting, hide_preview, hiding_started, judge, needs_title,
    plan_look, share_state, transition, Answer, LookPlan, Rect, ShareState, Signal, Snapshot, TopWindow, EVENT,
    QUNS_APP, QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN, SETTING,
};
use crate::store::Store;

/// How often to look while the window is on screen or previews are hidden.
const POLL: Duration = Duration::from_secs(2);
/// The longest anything waits for a fresh look: past it the answer is unknown, and a pop-up hides
/// its preview rather than trusting an old answer.
const CHECK_LIMIT: Duration = Duration::from_millis(500);

struct Inner {
    /// Settings › Notifications › Hide previews while sharing or presenting.
    enabled: bool,
    /// tug's window is on screen (the window says so).
    watched: bool,
    /// The last finished look's strongest signal.
    signal: Option<Signal>,
    /// The last look couldn't tell: previews are hidden until one can.
    unknown: bool,
}

struct Monitor {
    app: AppHandle,
    inner: Mutex<Inner>,
    wake: Condvar,
}

static MONITOR: OnceLock<Monitor> = OnceLock::new();

impl Monitor {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn state(&self) -> ShareState {
        let g = self.lock();
        share_state(g.enabled, g.signal, g.unknown)
    }

    /// Record a look's answer: log what changed (the signal's name only) and tell the window if
    /// hiding started or stopped. With the setting off, nothing is hidden whatever the look said.
    /// Not knowing keeps the last signal for the log, and hides previews until a look can tell.
    fn apply(&self, answer: Answer) {
        let (before, after, was, now, was_unknown, now_unknown) = {
            let mut g = self.lock();
            let was = share_state(g.enabled, g.signal, g.unknown);
            let (before, was_unknown) = (g.signal, g.unknown);
            match answer {
                _ if !g.enabled => {
                    g.signal = None;
                    g.unknown = false;
                }
                Answer::Known(found) => {
                    g.signal = found;
                    g.unknown = false;
                }
                Answer::Unknown => g.unknown = true,
            }
            let now = share_state(g.enabled, g.signal, g.unknown);
            (before, g.signal, was, now, was_unknown, g.unknown)
        };
        // Once per episode, not once per pop-up.
        if now_unknown && !was_unknown {
            log::warn!("share privacy: couldn't check the screen in time; hiding previews until tug can");
        } else if was_unknown && !now_unknown {
            log::info!("share privacy: checking the screen again");
        }
        if let Some(line) = transition(before, after) {
            log::info!("share privacy: {line}");
        }
        if hiding_started(was, now) {
            // Before anything new pops up (a pop-up's own look lands here first, then shows).
            crate::toast::withdraw_while_hiding(&self.app);
        }
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
            unknown: false,
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
            while !(g.enabled && (g.watched || g.signal.is_some() || g.unknown)) {
                g = m.wake.wait(g).unwrap_or_else(|e| e.into_inner());
            }
        }
        m.apply(look_now());
        let g = m.lock();
        let _ = m.wake.wait_timeout(g, POLL);
    }
}

/// The looks running on `tug-share-look` helpers, so callers share one rather than each starting
/// their own, and a stuck one can't block the rest.
struct Looks {
    /// The newest look still running: its number and when it started.
    running: Option<(u64, Instant)>,
    /// The number the next look gets.
    next: u64,
    /// The newest finished look: its number and answer.
    finished: Option<(u64, Answer)>,
    /// Look threads that haven't finished, stuck ones included.
    threads: usize,
}

static LOOKS: Mutex<Looks> = Mutex::new(Looks {
    running: None,
    next: 0,
    finished: None,
    threads: 0,
});
static LOOK_DONE: Condvar = Condvar::new();
/// Said once: helper threads can't start.
static SPAWN_WARNED: AtomicBool = AtomicBool::new(false);

fn looks() -> MutexGuard<'static, Looks> {
    LOOKS.lock().unwrap_or_else(|e| e.into_inner())
}

/// A fresh look, limited in time: nothing waits more than [`CHECK_LIMIT`] on Windows. Joins a
/// look started moments ago; starts a fresh one when the last has been stuck for
/// [`super::LOOK_STUCK_AFTER`]. [`Answer::Unknown`] when no fresh answer came in time.
fn look_now() -> Answer {
    let deadline = Instant::now() + CHECK_LIMIT;
    let mut g = looks();
    let plan = plan_look(g.running.map(|(_, at)| at.elapsed()), g.threads);
    let id = match (plan, g.running) {
        (LookPlan::Join, Some((id, _))) => id,
        (LookPlan::GiveUp, _) => return Answer::Unknown,
        _ => {
            let id = g.next;
            g.next += 1;
            let spawned = std::thread::Builder::new()
                .name("tug-share-look".into())
                .spawn(move || finish_look(id, judge(&snapshot())));
            if let Err(e) = spawned {
                if !SPAWN_WARNED.swap(true, Ordering::Relaxed) {
                    log::warn!("share privacy: couldn't start a look ({e})");
                }
                return Answer::Unknown;
            }
            g.running = Some((id, Instant::now()));
            g.threads += 1;
            id
        }
    };
    loop {
        if let Some((done, answer)) = g.finished {
            // This look's answer, or a newer one's.
            if done >= id {
                return answer;
            }
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Answer::Unknown;
        }
        g = LOOK_DONE.wait_timeout(g, left).unwrap_or_else(|e| e.into_inner()).0;
    }
}

/// A look finished (however late): keep its answer if nothing newer has finished.
fn finish_look(id: u64, answer: Answer) {
    let mut g = looks();
    g.threads = g.threads.saturating_sub(1);
    if g.finished.is_none_or(|(done, _)| done < id) {
        g.finished = Some((id, answer));
    }
    if g.running.is_some_and(|(running, _)| running == id) {
        g.running = None;
    }
    LOOK_DONE.notify_all();
}

/// Right before a pop-up: should it hide its preview? Looks again rather than trusting the last
/// answer, since tug is usually in the tray when pop-ups matter and nothing was watching. With
/// the setting on and no fresh answer, the preview is hidden (`hide_preview`).
pub fn previews_hidden_now() -> bool {
    let Some(m) = MONITOR.get() else { return false };
    if !m.lock().enabled {
        return false;
    }
    let answer = look_now();
    m.apply(answer);
    hide_preview(m.lock().enabled, answer)
}

/// tug's window came on screen (`on`) or went away. On screen, look now and every 2 s after, so
/// the banner is right; returns the answer so the window can show it at once (hidden, saying
/// so, when tug couldn't tell).
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
        m.apply(look_now());
    }
    m.wake.notify_all();
    m.state()
}

/// The Settings switch changed. Off: stop looking and show everything again at once.
pub fn set_enabled(on: bool) {
    let Some(m) = MONITOR.get() else { return };
    m.lock().enabled = on;
    if !on {
        m.apply(Answer::Known(None));
    }
    m.wake.notify_all();
}

/// One look at the screen. See the module docs for what each part costs. A call that fails
/// marks the look incomplete, so finding nothing then means "couldn't tell" (`judge`).
fn snapshot() -> Snapshot {
    // SAFETY: plain FFI call with no arguments.
    let quns = unsafe { SHQueryUserNotificationState() }.ok().map(|s| s.0);
    let full_screen_answer = matches!(quns, Some(QUNS_BUSY | QUNS_APP | QUNS_RUNNING_D3D_FULL_SCREEN));
    let duplicating = display_duplicated();
    let windows = windows_of_interest();
    Snapshot {
        quns,
        foreground_full_screen: full_screen_answer && foreground_covers_monitor(),
        duplicating: duplicating.unwrap_or(false),
        incomplete: quns.is_none() || duplicating.is_none() || windows.is_none(),
        windows: windows.unwrap_or_default(),
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
    // Clamped: this runs inside EnumWindows' callback, where a panic must not happen.
    String::from_utf16_lossy(&buf[..(n.max(0) as usize).min(buf.len())])
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

/// Windows+P "Duplicate": the current display topology is a clone. `None` when Windows couldn't
/// say. Access denied means a remote session or the secure desktop (UAC, lock screen), where this
/// session has no local display to duplicate: that's an answer, not a failure.
/// <https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-querydisplayconfig>
fn display_duplicated() -> Option<bool> {
    // The display set-up can change between the size query and the read (a projector plugged in):
    // Windows then says the buffer is too small, so ask again.
    for _ in 0..3 {
        match read_topology() {
            Ok(topology) => return Some(topology == DISPLAYCONFIG_TOPOLOGY_CLONE),
            Err(e) if e == ERROR_ACCESS_DENIED => return Some(false),
            Err(e) if e == ERROR_INSUFFICIENT_BUFFER => continue,
            Err(_) => return None,
        }
    }
    None
}

fn read_topology() -> Result<DISPLAYCONFIG_TOPOLOGY_ID, WIN32_ERROR> {
    let (mut paths, mut modes) = (0u32, 0u32);
    // SAFETY: out-pointers are locals; the arrays are sized from GetDisplayConfigBufferSizes and
    // their lengths are passed alongside.
    unsafe {
        let sized = GetDisplayConfigBufferSizes(QDC_DATABASE_CURRENT, &mut paths, &mut modes);
        if sized != ERROR_SUCCESS {
            return Err(sized);
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
        if result == ERROR_SUCCESS {
            Ok(topology)
        } else {
            Err(result)
        }
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
/// their title only where a rule matches by title). `None` if the pass over windows failed
/// (`visit` never stops it early, so a failure is Windows').
fn windows_of_interest() -> Option<Vec<TopWindow>> {
    let mut scan = Scan {
        found: Vec::new(),
        exes: HashMap::new(),
    };
    // SAFETY: `scan` outlives EnumWindows, which calls `visit` synchronously on this thread.
    let walked = unsafe { EnumWindows(Some(visit), LPARAM(&mut scan as *mut Scan as isize)) };
    walked.ok().map(|()| scan.found)
}

unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // Nothing may unwind into Windows from here (`crate::callback`): a panic skips the window and
    // the walk carries on.
    crate::callback::guarded(
        "share privacy window scan",
        // SAFETY: as `visit_one` requires, straight from EnumWindows.
        || unsafe { visit_one(hwnd, lparam) },
        || BOOL(1),
    )
}

/// SAFETY: `lparam` must be the `Scan` passed by `windows_of_interest`, during its EnumWindows.
unsafe fn visit_one(hwnd: HWND, lparam: LPARAM) -> BOOL {
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
            String::from_utf16_lossy(&buf[..(n.max(0) as usize).min(buf.len())])
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
            "one look: {each:?} (quns {:?}, duplicating {}, {} windows of interest, answer {:?})",
            s.quns,
            s.duplicating,
            s.windows.len(),
            super::judge(&s)
        );
    }
}
