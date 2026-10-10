mod ams;
mod ancs;
mod ancs_queue;
mod app_icons;
mod autostart;
mod ble;
mod bt_inventory;
mod cache_trim;
mod clipboard;
mod code_fill;
mod codes;
mod commands;
mod contact_photos;
mod device_info;
mod device_kind;
mod devtools;
mod diagnostics;
mod frontend_log;
pub mod hfp;
mod link_policy;
mod location;
pub mod map;
mod media_keys;
mod messages;
#[cfg(test)]
mod perf;
mod spotify;
mod startup;
mod state;
mod store;
mod text;
pub mod toast;
mod tray;
mod tugboat;
mod wake;
mod webview_watch;
mod wedge;

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

/// Open tug.db. When it won't open (corrupt, locked by a sync app, read-only, a failed upgrade),
/// say so on screen and offer to set it aside and start fresh, or quit — never just vanish.
fn open_store(dir: &std::path::Path, log_dir: Option<&std::path::Path>) -> Store {
    let path = dir.join("tug.db");
    loop {
        let err = match Store::open(&path) {
            Ok(store) => return store,
            Err(e) => e.to_string(),
        };
        log::error!("couldn't open {}: {err}", path.display());
        if !startup::ask_start_fresh(&path, &err, log_dir) {
            log::logger().flush();
            std::process::exit(1);
        }
        match startup::set_aside(&path) {
            Ok(to) => log::warn!("set the database aside as {}; starting fresh", to.display()),
            // Shown again with the open error on the next try (the file is still there).
            Err(e) => log::error!("couldn't set the database aside: {e}"),
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let context = tauri::generate_context!();
    let log_dir = startup::log_dir(&context.config().identifier);
    startup::install_panic_hook(log_dir.clone());
    let setup_log_dir = log_dir.clone();
    let built = tauri::Builder::default()
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
        // Tugboat's "Choose files" picker (opened from Rust; the page never sees it).
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let store = Arc::new(open_store(&dir, setup_log_dir.as_deref()));
            let shared = Arc::new(Shared::new(app.handle().clone(), store.clone()));
            let ble = ble::start(shared.clone());
            let _ = shared.map.set(map::service::start(shared.clone()));
            media_keys::start(shared.clone(), ble.clone());
            // The Spotify connector: optional, set up by the owner in Settings. Album art is
            // cached under the app-data dir; the refresh token goes to Credential Manager.
            let spotify = Arc::new(spotify::Spotify::new(store, dir.clone()));
            // Tugboat: idle until the panel opens it.
            let tugboat = tugboat::TugboatService::new(app.handle().clone());
            // Developer tools: off unless switched on in Settings. The bridge only waits for a
            // connection (no thread, no polling); while off it answers "off" and nothing else.
            let devtools = devtools::DevTools::new(app.handle().clone(), shared.clone(), ble.clone(), tugboat.clone());
            devtools.start();
            // Code fill's type-the-code shortcut: registered now (on unless switched off), so it
            // works from a tray-only start too.
            code_fill::start(app.handle(), &shared.store);
            app.manage(AppState {
                shared,
                ble,
                spotify,
                tugboat,
                devtools,
            });
            // Keep the purely-cached image folders (album art/covers, app icons) from growing without
            // limit: drop the least-recently-used beyond the cap. Off the main thread so a big folder
            // scan never delays the window. Contact photos aren't capped here — they can't be
            // re-fetched, and are pruned by reference instead.
            let icons_dir = dir.join("icons");
            let art_dir = dir.join("spotify_art");
            std::thread::spawn(move || {
                cache_trim::APP_ICONS_TRIM.trim_now(&icons_dir, cache_trim::APP_ICONS_CAP);
                cache_trim::SPOTIFY_ART_TRIM.trim_now(&art_dir, cache_trim::SPOTIFY_ART_CAP);
            });
            // Nice to have, never a reason not to start.
            if let Err(e) = tray::install(app.handle()) {
                log::warn!("tray icon unavailable: {e}");
            }
            // Log WebView2 process-failed events (crash/hang of the web content), so the next one
            // isn't a mystery. Registered whether or not the window is shown now.
            if let Some(window) = app.get_webview_window("main") {
                webview_watch::watch(&window);
            }
            // The window is created hidden (see tauri.conf.json). Start in the tray only when
            // asked to (autostart adds `--minimized`) AND there's a tray to come back from;
            // otherwise show normally, so a `--minimized` launch with no tray isn't stranded.
            // Fit the window on screens smaller than its default size (a 1366×768 laptop, or a
            // small screen at 150%) before it's first shown.
            if let Some(window) = app.get_webview_window("main") {
                startup::fit_window(&window);
            }
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
            // Tugboat: files dropped onto tug's window are offered to the phone from here, so the
            // page script never gets to name a path to offer.
            if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    if !paths.is_empty() {
                        state.tugboat.offer_dropped(paths.clone());
                    }
                }
                return;
            }
            if let tauri::WindowEvent::Focused(true) = event {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    state.tugboat.window_shown();
                }
                return;
            }
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
                        // Nothing on screen would show Tugboat still listening: stop it (after any
                        // transfer in progress).
                        state.tugboat.window_hidden();
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
            commands::rescan_discovery,
            commands::pair_device,
            commands::remove_pairing,
            commands::confirm_pairing,
            commands::use_device,
            commands::forget_device,
            commands::pair_texts,
            commands::set_advertising,
            commands::get_settings,
            commands::set_setting,
            commands::get_autostart,
            commands::set_autostart,
            commands::pc_region,
            commands::list_messages,
            commands::get_contacts,
            commands::send_message,
            commands::retry_message,
            commands::refresh_messages,
            commands::copy_text,
            commands::copy_code,
            commands::code_hotkey_status,
            commands::set_code_hotkey,
            commands::set_conversation_hidden,
            commands::set_unread,
            commands::open_windows_settings,
            commands::popups_blocked,
            commands::open_url,
            commands::locate,
            commands::place_lookup,
            commands::app_icon,
            commands::app_website,
            commands::contact_photo,
            commands::mark_read,
            commands::set_watching,
            commands::check_switches,
            commands::get_calls,
            commands::refresh_calls,
            commands::dial,
            commands::show_toast,
            commands::copy_diagnostics,
            commands::bt_inventory,
            commands::open_logs_folder,
            commands::spotify_status,
            commands::spotify_connect,
            commands::spotify_disconnect,
            commands::spotify_playlists,
            commands::spotify_cover,
            commands::spotify_play_context,
            commands::spotify_play_track,
            commands::spotify_player,
            commands::spotify_set_repeat,
            commands::spotify_set_shuffle,
            commands::spotify_set_saved,
            commands::spotify_search,
            commands::spotify_queue,
            commands::spotify_add_to_queue,
            commands::spotify_recently_played,
            commands::spotify_top_tracks,
            commands::spotify_top_artists,
            commands::spotify_devices,
            commands::spotify_transfer,
            commands::spotify_seek,
            commands::spotify_album,
            commands::spotify_artist,
            commands::spotify_playlist_items,
            commands::spotify_add_to_playlist,
            commands::log_frontend_error,
            commands::tugboat_start,
            commands::tugboat_stop,
            commands::tugboat_status,
            commands::tugboat_user_is_admin,
            commands::tugboat_copy_link,
            commands::tugboat_pick_files,
            commands::tugboat_remove_offer,
            commands::tugboat_send_text,
            commands::tugboat_open_folder,
            commands::devtools_status,
            commands::devtools_set_enabled,
            commands::devtools_set_permission,
            commands::devtools_revoke,
            commands::devtools_confirm,
            commands::devtools_set_on_path,
        ])
        .build(context);
    // A setup or WebView failure: say what failed and where the log is, instead of exiting
    // with no window.
    let app = match built {
        Ok(app) => app,
        Err(e) => {
            log::error!("tug couldn't start: {e}");
            log::logger().flush();
            startup::show_fatal(&e.to_string(), log_dir.as_deref());
            std::process::exit(1);
        }
    };
    app.run(|app, event| {
        // Pop-up buttons only work while tug runs: don't leave dead ones in Action Center.
        if let tauri::RunEvent::Exit = event {
            toast::withdraw_all(app);
            // Tugboat never outlives tug: stop listening and remove unfinished uploads.
            if let Some(state) = app.try_state::<AppState>() {
                state.tugboat.shutdown_now();
            }
        }
    });
}
