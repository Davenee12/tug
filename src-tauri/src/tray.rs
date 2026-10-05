//! tug in the system tray, and unread texts on the taskbar: the tray tooltip counts them
//! and the taskbar button gets a small dot, so tug is noticeable while it's minimized.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

const TRAY_ID: &str = "tug";

/// Left-click brings tug forward; right-click offers Open and Quit.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open tug", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit tug", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("tug")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show(app),
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
                show(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Bring the main window to the front, restoring it if minimized.
pub fn show<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Unread texts: the count in the tray tooltip, and a dot on the taskbar button.
pub fn set_unread<R: Runtime>(app: &AppHandle<R>, count: u32) -> tauri::Result<()> {
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
