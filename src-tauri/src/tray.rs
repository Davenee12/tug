//! tug in the system tray, and unread texts on the taskbar: the tray tooltip counts them
//! and the taskbar button gets a small dot, so tug is noticeable while it's minimized.
//! Closing the window hides tug to the tray (it keeps mirroring the phone); Quit is in the
//! tray menu.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::state::events;

const TRAY_ID: &str = "tug";
/// Settings key: the "still running in the tray" hint has been shown once.
const HINT_SHOWN: &str = "tray.hint_shown";

/// Set once the tray icon exists. Without it, hiding on close would leave tug running
/// with no way back or out, so closing quits as before.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Unread texts the UI last reported (see `set_unread`). The backend doesn't know about
/// threads, so it uses this only to decide whether a tray click should also open the
/// newest conversation; the frontend picks which one.
static UNREAD: AtomicU32 = AtomicU32::new(0);

pub fn installed() -> bool {
    INSTALLED.load(Ordering::Relaxed)
}

/// Left-click (and the menu's Open) bring tug forward and, with unread texts, open the
/// newest unread conversation; right-click offers Open, Settings and Quit.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open tug", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit tug", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &settings, &quit])?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("tug")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_and_open_latest(app),
            "settings" => {
                show(app);
                let _ = app.emit(events::OPEN_SETTINGS, ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_and_open_latest(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    INSTALLED.store(true, Ordering::Relaxed);
    Ok(())
}

/// The window was closed and tug went to the tray: say so the first time, so it doesn't
/// look like tug vanished (or quit and stopped mirroring the phone).
pub fn hint_once<R: Runtime>(app: &AppHandle<R>, store: &crate::store::Store) {
    if store.setting(HINT_SHOWN).ok().flatten().is_some() {
        return;
    }
    use tauri_plugin_notification::NotificationExt;
    let shown = app
        .notification()
        .builder()
        .title("tug is still running")
        .body("It's in the tray by the clock, still mirroring your iPhone. Right-click it to quit.")
        .show();
    match shown {
        Ok(()) => {
            let _ = store.set_setting(HINT_SHOWN, "1");
        }
        Err(e) => log::info!("tray hint not shown: {e}"),
    }
}

/// Bring the main window to the front, restoring it if minimized.
pub fn show<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Bring tug forward and, if there are unread texts, ask the frontend to open the newest
/// unread conversation (as if it were clicked, so it's read and cleared the usual way).
/// With nothing unread this is just `show`, leaving the user on whatever view they left.
fn show_and_open_latest<R: Runtime>(app: &AppHandle<R>) {
    show(app);
    if UNREAD.load(Ordering::Relaxed) > 0 {
        let _ = app.emit(events::OPEN_LATEST_CONVERSATION, ());
    }
}

/// Unread texts: the count in the tray tooltip, and a dot on the taskbar button.
pub fn set_unread<R: Runtime>(app: &AppHandle<R>, count: u32) -> tauri::Result<()> {
    UNREAD.store(count, Ordering::Relaxed);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_tooltip(Some(tooltip(count)))?;
    }
    #[cfg(windows)]
    if let Some(window) = app.get_webview_window("main") {
        if count == 0 {
            window.set_overlay_icon(None)?;
        } else {
            let dot = dot_rgba();
            window.set_overlay_icon(Some(tauri::image::Image::new(&dot, DOT, DOT)))?;
        }
    }
    Ok(())
}

fn tooltip(count: u32) -> String {
    match count {
        0 => "tug".into(),
        1 => "tug · 1 unread".into(),
        n => format!("tug · {n} unread"),
    }
}

/// Overlay size: Windows draws taskbar overlays at 16×16.
const DOT: u32 = 16;
/// tug's coral (`primary` in style.css) and a cream ring so it reads on dark and light taskbars.
const CORAL: [u8; 3] = [0xcc, 0x78, 0x5c];
const CREAM: [u8; 3] = [0xfa, 0xf9, 0xf5];

/// A filled coral circle with a thin cream ring, anti-aliased, as RGBA.
fn dot_rgba() -> Vec<u8> {
    let size = DOT as f32;
    let center = size / 2.0;
    let outer = size / 2.0 - 0.5;
    let ring = 1.5;
    let mut out = Vec::with_capacity((DOT * DOT * 4) as usize);
    for y in 0..DOT {
        for x in 0..DOT {
            let d = ((x as f32 + 0.5 - center).powi(2) + (y as f32 + 0.5 - center).powi(2)).sqrt();
            let alpha = (outer + 0.5 - d).clamp(0.0, 1.0);
            let rgb = if d < outer - ring { CORAL } else { CREAM };
            out.extend_from_slice(&rgb);
            out.push((alpha * 255.0).round() as u8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_counts_unread() {
        assert_eq!(tooltip(0), "tug");
        assert_eq!(tooltip(1), "tug · 1 unread");
        assert_eq!(tooltip(12), "tug · 12 unread");
    }

    #[test]
    fn dot_is_a_coral_circle_on_a_clear_square() {
        let px = dot_rgba();
        assert_eq!(px.len(), (DOT * DOT * 4) as usize);
        let at = |x: u32, y: u32| {
            let i = ((y * DOT + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        assert_eq!(at(8, 8), [0xcc, 0x78, 0x5c, 255], "centre is solid coral");
        assert_eq!(at(0, 0)[3], 0, "corners are transparent");
        assert_eq!(&at(8, 0)[..3], &CREAM, "the rim is the cream ring");
    }
}
