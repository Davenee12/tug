//! Windows signing out, restarting or shutting down. tao runs tug's `RunEvent::Exit` from inside
//! WM_ENDSESSION, and Windows waits for it (showing "tug is preventing shutdown" after a few
//! seconds). On a normal Quit tug tidies up properly; when the session is ending it does only what
//! can't wait and leaves the rest to the next start, so it never holds up shutdown.
//!
//! How it knows: the main window is subclassed to notice WM_QUERYENDSESSION / WM_ENDSESSION, and
//! `GetSystemMetrics(SM_SHUTTINGDOWN)` backs that up. What to skip is `ExitPlan` (pure, tested).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Set once Windows says the session is ending.
static ENDING: AtomicBool = AtomicBool::new(false);

/// How long a normal Quit waits for Windows to hand the phone's audio back to the phone.
pub const AUDIO_QUIT_WAIT: Duration = Duration::from_secs(2);

/// What tug's exit does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitPlan {
    /// Take tug's pop-ups out of Action Center now (a synchronous WinRT call). When the session
    /// ends they go at the next start instead (`toast::withdraw_all` runs then too).
    pub withdraw_toasts: bool,
    /// How long to wait for Windows to close PC audio. The close is always started.
    pub audio_wait: Duration,
    /// Delete Tugboat's unfinished uploads now (can be gigabytes). Otherwise the next start
    /// removes them (`TugboatService::new`).
    pub clean_tugboat: bool,
}

impl ExitPlan {
    pub fn for_exit(session_ending: bool) -> ExitPlan {
        if session_ending {
            ExitPlan {
                withdraw_toasts: false,
                audio_wait: Duration::ZERO,
                clean_tugboat: false,
            }
        } else {
            ExitPlan {
                withdraw_toasts: true,
                audio_wait: AUDIO_QUIT_WAIT,
                clean_tugboat: true,
            }
        }
    }
}

/// Whether Windows is ending the session (signing out, restarting, shutting down).
pub fn ending() -> bool {
    ENDING.load(Ordering::SeqCst) || shutting_down()
}

#[cfg(windows)]
fn shutting_down() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SHUTTINGDOWN};
    // SAFETY: a plain query with no pointers.
    unsafe { GetSystemMetrics(SM_SHUTTINGDOWN) != 0 }
}

#[cfg(not(windows))]
fn shutting_down() -> bool {
    false
}

/// Listen for the session ending on tug's main window. Nothing else about the window changes:
/// every message still goes on to tao.
#[cfg(windows)]
pub fn watch(window: &tauri::WebviewWindow) {
    // A window can only be subclassed from the thread that owns it (the main thread); the window
    // rebuilt after a WebView2 failure is built from a helper thread.
    let w = window.clone();
    if let Err(e) = window.run_on_main_thread(move || subclass(&w)) {
        log::debug!("session end: couldn't watch the window ({e})");
    }
}

#[cfg(windows)]
fn subclass(window: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::SetWindowSubclass;
    let Ok(hwnd) = window.hwnd() else {
        log::debug!("session end: no window handle to watch");
        return;
    };
    let hwnd = HWND(hwnd.0);
    // SAFETY: the subclass proc is a plain function with no state; it stays installed for the
    // window's lifetime (tug has one main window, destroyed only at exit).
    let ok = unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0) };
    if !ok.as_bool() {
        log::debug!("session end: couldn't watch the window");
    }
}

#[cfg(not(windows))]
pub fn watch(_window: &tauri::WebviewWindow) {}

#[cfg(windows)]
const SUBCLASS_ID: usize = 0x7475_6765; // "tuge"

#[cfg(windows)]
unsafe extern "system" fn subclass_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::Shell::DefSubclassProc;
    use windows::Win32::UI::WindowsAndMessaging::WM_ENDSESSION;
    if is_session_end(msg, wparam.0) {
        ENDING.store(true, Ordering::SeqCst);
    } else if msg == WM_ENDSESSION {
        // wParam FALSE: another app cancelled the shutdown; tug carries on as normal.
        ENDING.store(false, Ordering::SeqCst);
    }
    // SAFETY: forwarding the message unchanged to the next handler (tao's).
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// WM_QUERYENDSESSION, or WM_ENDSESSION saying the session really is ending (wParam TRUE).
#[cfg_attr(not(windows), allow(dead_code))]
fn is_session_end(msg: u32, wparam: usize) -> bool {
    const WM_QUERYENDSESSION: u32 = 0x0011;
    const WM_ENDSESSION: u32 = 0x0016;
    msg == WM_QUERYENDSESSION || (msg == WM_ENDSESSION && wparam != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normal_quit_tidies_up_properly() {
        let plan = ExitPlan::for_exit(false);
        assert!(plan.withdraw_toasts);
        assert!(plan.clean_tugboat);
        assert_eq!(plan.audio_wait, AUDIO_QUIT_WAIT);
    }

    #[test]
    fn a_session_end_never_waits() {
        let plan = ExitPlan::for_exit(true);
        assert!(!plan.withdraw_toasts, "pop-ups go at the next start instead");
        assert!(!plan.clean_tugboat, "unfinished uploads go at the next start instead");
        assert_eq!(plan.audio_wait, Duration::ZERO);
    }

    #[test]
    fn recognises_the_session_ending_messages() {
        assert!(is_session_end(0x0011, 0)); // WM_QUERYENDSESSION
        assert!(is_session_end(0x0016, 1)); // WM_ENDSESSION, ending
        assert!(!is_session_end(0x0016, 0)); // WM_ENDSESSION, cancelled
        assert!(!is_session_end(0x0010, 0)); // WM_CLOSE
    }
}
