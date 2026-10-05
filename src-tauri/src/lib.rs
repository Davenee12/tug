mod ams;
mod ancs;
mod ancs_queue;
mod ble;
mod clipboard;
mod commands;
mod location;
pub mod map;
mod messages;
#[cfg(test)]
mod perf;
mod state;
mod store;

use std::sync::Arc;

use tauri::Manager;

use commands::AppState;
use state::Shared;
use store::Store;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be first: a second launch focuses the running window instead of
        // starting a rival that can't advertise (Windows allows one provider per service).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            // Logs land in %LOCALAPPDATA%\dev.davejames.tug\logs for hardware debugging.
            tauri_plugin_log::Builder::new()
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
            app.manage(AppState { shared, ble });
            Ok(())
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
            commands::list_messages,
            commands::get_contacts,
            commands::send_message,
            commands::refresh_messages,
            commands::copy_text,
            commands::set_hidden,
            commands::locate,
            commands::mark_read,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
