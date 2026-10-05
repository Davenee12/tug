mod ams;
mod ancs;
mod ancs_queue;
mod app_icons;
mod ble;
mod clipboard;
mod commands;
mod device_kind;
pub mod hfp;
mod location;
pub mod map;
mod media_keys;
mod messages;
#[cfg(test)]
mod perf;
mod state;
mod store;
mod tray;

use std::sync::Arc;

use tauri::Manager;

use commands::AppState;
use state::Shared;
use store::Store;

/// Passed to tug by the autostart entry (and usable by hand). It means "come up hidden in
/// the tray", so starting with Windows doesn't throw the window in the user's face at login.
pub const MINIMIZED_ARG: &str = "--minimized";

/// Whether this launch asked to start minimized (autostart adds `MINIMIZED_ARG`).
fn launched_minimized() -> bool {
    std::env::args().any(|a| a == MINIMIZED_ARG)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be first: a second launch focuses the running window instead of
        // starting a rival that can't advertise (Windows allows one provider per service).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show(app)))
        // Start with Windows (Settings › General). The autostart entry carries `--minimized`
        // so an automatic launch comes up hidden in the tray; the switch reads the registry
        // (via `autolaunch().is_enabled()`), so that state is the single source of truth.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![MINIMIZED_ARG]),
        ))
        .plugin(
            // Logs land in %LOCALAPPDATA%\dev.davejames.tug\logs for hardware debugging.
            // Keep a few rotated files: the default (one 40 KB file) had already thrown away
            // the minutes before a failure by the time anyone looked.
            tauri_plugin_log::Builder::new()
                .max_file_size(2_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(5))
                .level(log::LevelFilter::Info)
                .level_for("tug_lib", log::LevelFilter::Debug)
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let store = Arc::new(Store::open(&dir.join("tug.db"))?);
            let shared = Arc::new(Shared::new(app.handle().clone(), store));
            let ble = ble::start(shared.clone());
            let _ = shared.map.set(map::service::start(shared.clone()));
            media_keys::start(shared.clone(), ble.clone());
            app.manage(AppState { shared, ble });
            // Nice to have, never a reason not to start.
            if let Err(e) = tray::install(app.handle()) {
                log::warn!("tray icon unavailable: {e}");
            }
            // The window is created hidden (see tauri.conf.json). Start in the tray only when
            // asked to (autostart adds `--minimized`) AND there's a tray to come back from;
            // otherwise show normally, so a `--minimized` launch with no tray isn't stranded.
            if !(launched_minimized() && tray::installed()) {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            Ok(())
        })
        // Closing the window hides tug to the tray instead of quitting (Quit is in the tray
        // menu), so the phone stays mirrored and notifications keep arriving.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() != "main" || !tray::installed() {
                    return;
                }
                let state = window.app_handle().try_state::<AppState>();
                // Settings › General › Keep running when closed (on unless switched off).
                let keep = state
                    .as_ref()
                    .and_then(|s| s.shared.store.setting("ui.closeToTray").ok().flatten())
                    .as_deref()
                    != Some("false");
                if keep {
                    api.prevent_close();
                    let _ = window.hide();
                    if let Some(state) = state {
                        tray::hint_once(window.app_handle(), &state.shared.store);
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_now_playing,
            commands::list_notifications,
            commands::search_notifications,
            commands::search_all,
            commands::clear_history,
            commands::perform_action,
            commands::media_command,
            commands::start_discovery,
            commands::stop_discovery,
            commands::pair_device,
            commands::confirm_pairing,
            commands::use_device,
            commands::forget_device,
            commands::set_advertising,
            commands::get_settings,
            commands::set_setting,
            commands::get_autostart,
            commands::set_autostart,
            commands::list_messages,
            commands::get_contacts,
            commands::send_message,
            commands::refresh_messages,
            commands::copy_text,
            commands::set_hidden,
            commands::set_unread,
            commands::open_windows_settings,
            commands::locate,
            commands::place_lookup,
            commands::app_icon,
            commands::mark_read,
            commands::set_watching,
            commands::get_calls,
            commands::refresh_calls,
            commands::dial,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
