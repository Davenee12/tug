//! tug in the system tray, and unread texts on the taskbar: the tray icon gets a coral badge
//! and its tooltip counts them, and the taskbar button gets a small dot, so tug is noticeable
//! while it's minimized or hidden in the tray.
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
/// The tray icon currently shows the unread badge (so it's only swapped when that changes).
static BADGED: AtomicBool = AtomicBool::new(false);

pub fn installed() -> bool {
    INSTALLED.load(Ordering::Relaxed)
}

/// Left-click (and the menu's Open) bring tug forward and, with unread texts, open the
/// newest unread conversation; right-click offers Open and Quit.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open tug", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit tug", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("tug")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_and_open_latest(app),
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
    // Hiding to the tray can recreate the taskbar button, which drops its overlay: put the
    // unread dot back.
    #[cfg(windows)]
    let _ = set_overlay(app, UNREAD.load(Ordering::Relaxed));
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

/// Unread texts: a badge on the tray icon and the count in its tooltip (the cue that shows
/// while tug is hidden in the tray, where there's no taskbar button), and a dot on the taskbar
/// button.
pub fn set_unread<R: Runtime>(app: &AppHandle<R>, count: u32) -> tauri::Result<()> {
    UNREAD.store(count, Ordering::Relaxed);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_tooltip(Some(tooltip(count)))?;
        let badge = count > 0;
        if BADGED.swap(badge, Ordering::Relaxed) != badge {
            if let Some(icon) = app.default_window_icon() {
                let image = if badge {
                    let (w, h) = (icon.width(), icon.height());
                    tauri::image::Image::new_owned(badged_rgba(icon.rgba(), w, h), w, h)
                } else {
                    icon.clone().to_owned()
                };
                tray.set_icon(Some(image))?;
            }
        }
    }
    #[cfg(windows)]
    set_overlay(app, count)?;
    Ok(())
}

/// The taskbar button's unread dot (none at zero).
#[cfg(windows)]
fn set_overlay<R: Runtime>(app: &AppHandle<R>, count: u32) -> tauri::Result<()> {
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

/// tug's icon (RGBA, `w`×`h`) with a coral badge, cream-ringed like the taskbar dot, over its
/// top-right corner: the tray's unread cue. Anti-aliased and blended over the icon.
fn badged_rgba(icon: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut out = icon.to_vec();
    let side = w.min(h) as f32;
    let radius = side * 0.28;
    let ring = (side * 0.07).max(1.0);
    let (cx, cy) = (w as f32 - radius, radius);
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let cover = (radius + 0.5 - d).clamp(0.0, 1.0);
            if cover == 0.0 {
                continue;
            }
            let rgb = if d < radius - ring { CORAL } else { CREAM };
            let i = ((y * w + x) * 4) as usize;
            for c in 0..3 {
                out[i + c] = (rgb[c] as f32 * cover + out[i + c] as f32 * (1.0 - cover)).round() as u8;
            }
            out[i + 3] = (255.0 * cover + out[i + 3] as f32 * (1.0 - cover)).round() as u8;
        }
    }
    out
}

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
    fn tray_badge_sits_on_the_top_right_and_leaves_the_rest_of_the_icon() {
        let (w, h) = (32, 32);
        // A plain opaque grey icon.
        let icon: Vec<u8> = (0..w * h).flat_map(|_| [0x40, 0x40, 0x40, 0xff]).collect();
        let px = badged_rgba(&icon, w, h);
        assert_eq!(px.len(), icon.len());
        let at = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        let r = (32.0 * 0.28) as u32;
        assert_eq!(at(w - r, r), [0xcc, 0x78, 0x5c, 255], "badge centre is coral");
        assert_eq!(at(0, h - 1), [0x40, 0x40, 0x40, 0xff], "bottom-left untouched");
        assert_eq!(at(0, 0), [0x40, 0x40, 0x40, 0xff], "top-left untouched");
        // On a transparent corner the badge is still opaque.
        let clear = vec![0u8; (w * h * 4) as usize];
        let px = badged_rgba(&clear, w, h);
        assert_eq!(px[(((r * w) + (w - r)) * 4 + 3) as usize], 255);
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
