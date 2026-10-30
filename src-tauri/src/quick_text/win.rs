//! The Windows side of quick text: what had the keyboard when the shortcut was pressed, and giving
//! it back when the composer closes. The press itself arrives on code fill's `tug-hotkey` thread.
//!
//! The decisions live in `mod.rs`; this file only asks Windows and reports.

use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsWindow, SetForegroundWindow,
};

use super::{note_press, tug_window, Front, GiveBack, TugWindow};
use crate::quick_actions::{self, QuickAction};

/// The shortcut was pressed. What's in front is read here, at once, before tug comes forward;
/// the rest runs off the hotkey thread (reading tug's window waits on the UI thread).
pub(crate) fn pressed(app: &AppHandle) {
    let front = front();
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("tug-quick-text".into())
        .spawn(move || {
            let tug = app.get_webview_window("main").map_or(TugWindow::Hidden, |w| {
                tug_window(w.is_visible().unwrap_or(false), w.is_minimized().unwrap_or(false))
            });
            note_press(front, tug);
            log::info!("quick text shortcut pressed");
            // Brings tug forward and has the UI open the composer, once, even if it's still loading.
            quick_actions::receive(&app, QuickAction::QuickText);
        });
    if let Err(e) = spawned {
        log::warn!("quick text: couldn't open the composer: {e}");
    }
}

/// The app the person was in gets the keyboard back, then tug's window goes back to how it was.
/// In that order: tug is in front now, so Windows lets it hand the keyboard on; once tug is hidden
/// it may not.
pub(super) fn give_back(app: &AppHandle, back: GiveBack) {
    if let Some(raw) = back.window {
        let hwnd = HWND(raw as *mut core::ffi::c_void);
        // SAFETY: IsWindow only checks the handle (that app may have closed meanwhile) and
        // SetForegroundWindow takes a handle IsWindow just accepted.
        let returned = unsafe { IsWindow(Some(hwnd)).as_bool() && SetForegroundWindow(hwnd).as_bool() };
        if !returned {
            log::info!("quick text: the app the shortcut was pressed in is gone or didn't come back");
        }
    }
    if let Some(window) = app.get_webview_window("main") {
        let put_back = match back.tug {
            TugWindow::Hidden => window.hide(),
            TugWindow::Minimized => window.minimize(),
            TugWindow::Behind => Ok(()),
        };
        if let Err(e) = put_back {
            log::warn!("quick text: couldn't put tug's window back: {e}");
        }
    }
}

/// What has the keyboard: tug's own window, another app's, or nothing.
fn front() -> Front {
    // SAFETY: read-only Win32 queries with a valid out-pointer.
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.is_invalid() {
            return Front::Nothing;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == GetCurrentProcessId() {
            Front::Tug
        } else {
            Front::Other(hwnd.0 as isize)
        }
    }
}
