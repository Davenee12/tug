mod album_art;
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
mod forgotten;
mod frontend_log;
pub mod hfp;
mod link_policy;
mod location;
pub mod map;
mod media_keys;
mod messages;
mod pc_audio;
#[cfg(test)]
mod perf;
mod pin_confirm;
mod quick_actions;
mod session_end;
mod share_privacy;
mod spotify;
mod startup;
mod state;
mod store;
mod text;
pub mod toast;
mod tray;
mod tugboat;
mod updater;
mod voice;
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

/// This launch's command line (the program path first), never panicking: `std::env::args()`
/// panics on an argument that isn't valid Unicode (a lone UTF-16 surrogate from a shortcut or
/// another program), and the release build aborts on a panic, so tug would vanish at launch. Such
/// an argument is read lossily (U+FFFD for what can't be decoded); none of tug's own flags can
/// contain one, so it simply matches nothing.
pub fn launch_args() -> impl Iterator<Item = String> {
    lossy_args(std::env::args_os())
}

fn lossy_args(args: impl IntoIterator<Item = std::ffi::OsString>) -> impl Iterator<Item = String> {
    args.into_iter().map(|a| a.to_string_lossy().into_owned())
}

/// Whether this launch asked to start minimized (autostart adds `MINIMIZED_ARG`).
fn launched_minimized() -> bool {
    launch_args().any(|a| a == MINIMIZED_ARG)
}

/// Run by the uninstaller (windows/hooks.nsh) before tug's files are removed: takes this tug's
/// own `bin` folder off the user's PATH, however long PATH is (NSIS can only handle 1023
/// characters). It takes no path from the caller, touches only the user's own PATH, and does
/// nothing when the folder isn't on it, so running it by hand can't do anything Settings ›
/// Developer tools › Remove from PATH couldn't.
pub const REMOVE_FROM_PATH_ARG: &str = "--remove-from-path";

/// The uninstaller's step, if that's what this launch is for: `Some(exit code)`, and the app
/// doesn't start. `None` for every other launch.
pub fn run_uninstall_step(args: impl Iterator<Item = String>) -> Option<i32> {
    if !is_uninstall_step(args) {
        return None;
    }
    let dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("bin")));
    // Never fall through to starting the app from the uninstaller.
    let Some(dir) = dir else { return Some(1) };
    Some(match devtools::path_env::remove(&dir.to_string_lossy()) {
        Ok(()) => 0,
        Err(_) => 1,
    })
}

/// Only exactly `tug.exe --remove-from-path` (nothing before it) is the uninstaller's step.
fn is_uninstall_step(mut args: impl Iterator<Item = String>) -> bool {
    args.next().as_deref() == Some(REMOVE_FROM_PATH_ARG)
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
    // Before any window exists, so tug's taskbar button carries the id its jump list is under.
    quick_actions::init(&context.config().identifier);
    let built = tauri::Builder::default()
        // Must be first: a second launch focuses the running window instead of
        // starting a rival that can't advertise (Windows allows one provider per service).
        // A jump-list task is such a launch: its `--action` is carried out here.
        .plugin(tauri_plugin_single_instance::init(
            |app, args, _cwd| match quick_actions::parse_args(&args) {
                Some(action) => quick_actions::receive(app, action),
                None => tray::show(app),
            },
        ))
        // Start with Windows (Settings › General) is tug's own HKCU Run entry (autostart.rs),
        // carrying `--minimized` so an automatic launch comes up hidden in the tray.
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
        // Automatic updates (updater/): the check, download and signature check run in Rust. It
        // does nothing until tauri.conf.json carries a real public key (docs/RELEASING.md).
        .plugin(updater::plugin())
        .setup(move |app| {
            // Pop-ups left by the last run can't be answered (nothing hears a press while tug is
            // closed). A normal Quit withdraws them; when Windows shut down it was left to now.
            // Off the main thread and time-limited; this run's own pop-ups wait for it (at most
            // `toast::STALE_WITHDRAW_LIMIT`) so it can't take them back. Before the BLE actor and
            // the MAP worker start, so nothing can pop up ahead of the gate.
            toast::withdraw_stale_at_startup(app.handle());
            // Running elevated: move an old all-users Start with Windows entry back to this user.
            std::thread::spawn(autostart::tidy_at_startup);
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let store = Arc::new(open_store(&dir, setup_log_dir.as_deref()));
            // Coming back from an update restart: show the window (even from a `--minimized`
            // start) so the person sees tug back on the screen they left.
            let resuming = updater::resume_pending(&store);
            // Taskbar quick actions: a task clicked while tug wasn't running started this launch;
            // the UI carries it out once it has loaded. Then (re)build the jump list itself.
            if let Some(action) = quick_actions::parse_args(launch_args()) {
                quick_actions::queue(action);
            }
            let show_games = store.setting(quick_actions::SHOW_GAMES_SETTING).ok().flatten();
            quick_actions::install(quick_actions::show_games(show_games.as_deref()));
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
            // Play iPhone audio on this PC: off until asked for (or the owner's automatic switch).
            let pc_audio = pc_audio::PcAudio::new(shared.clone());
            pc_audio.start();
            // Code fill's type-the-code shortcut: registered now (on unless switched off), so it
            // works from a tray-only start too.
            code_fill::start(app.handle(), &shared.store);
            // Hide previews while sharing or presenting: a thread that sleeps until the window is
            // on screen or a pop-up finds something to hide.
            #[cfg(windows)]
            share_privacy::start(app.handle(), &shared.store);
            // Voice typing: off by default; when the owner turned it on, its shortcut is back now.
            voice::start(app.handle(), &shared.store);
            app.manage(AppState {
                shared,
                ble,
                spotify,
                tugboat,
                devtools,
                pc_audio,
            });
            // Automatic updates: a first check shortly after startup, then every 6 hours, only
            // when set up (a public key in tauri.conf.json) and switched on.
            let updates = updater::Updater::new(app.handle().clone());
            app.manage(updates.clone());
            updates.start();
            // Keep the purely-cached image folders (album art/covers, app icons) from growing without
            // limit: drop the least-recently-used beyond the cap. Off the main thread so a big folder
            // scan never delays the window. Contact photos aren't capped here — they can't be
            // re-fetched, and are pruned by reference instead.
            let icons_dir = dir.join("icons");
            let art_dir = dir.join("spotify_art");
            let album_dir = dir.join(album_art::DIR);
            std::thread::spawn(move || {
                cache_trim::APP_ICONS_TRIM.trim_now(&icons_dir, cache_trim::APP_ICONS_CAP);
                cache_trim::SPOTIFY_ART_TRIM.trim_now(&art_dir, cache_trim::SPOTIFY_ART_CAP);
                cache_trim::ALBUM_ART_TRIM.trim_now(&album_dir, cache_trim::ALBUM_ART_CAP);
                album_art::sweep_misses(&album_dir, std::time::SystemTime::now());
            });
            // Nice to have, never a reason not to start.
            if let Err(e) = tray::install(app.handle()) {
                log::warn!("tray icon unavailable: {e}");
            }
            // Watch WebView2 for a failed process: log which one truthfully, and reload the page or
            // rebuild the window when that brings it back. Registered whether or not the window is
            // shown now.
            if let Some(window) = app.get_webview_window("main") {
                webview_watch::watch(&window);
                // Notice Windows signing out or shutting down, so exit doesn't hold it up.
                session_end::watch(&window);
            }
            // The window is created hidden (see tauri.conf.json). Start in the tray only when
            // asked to (autostart adds `--minimized`) AND there's a tray to come back from;
            // otherwise show normally, so a `--minimized` launch with no tray isn't stranded.
            // Fit the window on screens smaller than its default size (a 1366×768 laptop, or a
            // small screen at 150%) before it's first shown.
            if let Some(window) = app.get_webview_window("main") {
                startup::fit_window(&window);
            }
            if resuming || !(launched_minimized() && tray::installed()) {
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
            commands::pc_audio_status,
            commands::pc_audio_set,
            commands::pc_audio_set_auto,
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
            commands::take_quick_actions,
            commands::set_code_hotkey,
            commands::voice_status,
            commands::set_voice_typing,
            commands::set_conversation_hidden,
            commands::set_unread,
            commands::open_windows_settings,
            commands::popups_blocked,
            commands::open_url,
            commands::locate,
            commands::place_lookup,
            commands::app_icon,
            commands::app_website,
            commands::album_art,
            commands::contact_photo,
            commands::mark_read,
            commands::set_watching,
            commands::check_switches,
            commands::set_switches_visible,
            commands::warm_texts,
            commands::get_calls,
            commands::refresh_calls,
            commands::dial,
            commands::show_toast,
            commands::screen_share_watch,
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
            commands::tugboat_send_clipboard,
            commands::tugboat_open_folder,
            commands::game_pad_open,
            commands::game_pad_close,
            commands::game_pad_feedback,
            commands::devtools_status,
            commands::devtools_set_enabled,
            commands::devtools_set_permission,
            commands::devtools_revoke,
            commands::devtools_confirm,
            commands::devtools_set_on_path,
            updater::update_status,
            updater::update_check,
            updater::update_download,
            updater::update_set_auto,
            updater::update_apply,
            updater::update_open_release_page,
            updater::update_take_resume,
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
        // Rebuilding the main window after WebView2's browser process died briefly leaves no
        // window at all; that isn't the user closing tug.
        if let tauri::RunEvent::ExitRequested { code: None, api, .. } = &event {
            if webview_watch::recreating_window() {
                api.prevent_exit();
                return;
            }
        }
        if let tauri::RunEvent::Exit = event {
            let ending = session_end::ending();
            tidy_before_exit(app, ending);
            // Quit with an update downloaded: install it now, without starting tug again (the
            // tray's item said "Quit and update tug"). Never while Windows is ending the session.
            if let Some(updates) = app.try_state::<updater::Updater>() {
                updates.apply_on_quit(ending);
            }
        }
    });
}

/// What tug does on its way out, once, however it leaves: Quit, Windows ending the session, or an
/// update's installer about to start (`updater`, from the plugin's `on_before_exit`).
/// Windows signing out or shutting down runs this from WM_ENDSESSION and waits for it: then only
/// what can't wait is done, and the rest is left to the next start.
pub(crate) fn tidy_before_exit(app: &tauri::AppHandle, ending: bool) {
    if !updater::first_tidy() {
        return;
    }
    let started = std::time::Instant::now();
    let plan = session_end::ExitPlan::for_exit(ending);
    // Pop-up buttons only work while tug runs: don't leave dead ones in Action Center.
    if plan.withdraw_toasts {
        toast::withdraw_all(app);
    }
    // Tugboat never outlives tug: stop listening (and remove unfinished uploads).
    if let Some(state) = app.try_state::<AppState>() {
        state.tugboat.shutdown_now(plan.clean_tugboat);
        // Hand the phone's audio back to the phone rather than leave it routed to a PC
        // that stopped listening.
        state.pc_audio.shutdown_now(plan.audio_wait);
    }
    log::info!(
        "exit: tidied up in {} ms{}",
        started.elapsed().as_millis(),
        if ending { " (Windows is ending the session)" } else { "" }
    );
}

#[cfg(test)]
mod uninstall_step_tests {
    use super::*;

    #[test]
    fn only_the_exact_flag_is_the_uninstall_step() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>().into_iter();
        assert!(is_uninstall_step(args(&["--remove-from-path"])));
        assert!(!is_uninstall_step(args(&[])));
        assert!(!is_uninstall_step(args(&[MINIMIZED_ARG])));
        assert!(!is_uninstall_step(args(&[MINIMIZED_ARG, "--remove-from-path"])));
        assert!(!is_uninstall_step(args(&["--REMOVE-FROM-PATH"])));
        // Every ordinary launch starts the app.
        assert_eq!(run_uninstall_step(args(&[MINIMIZED_ARG])), None);
    }
}

#[cfg(test)]
mod launch_args_tests {
    use std::ffi::OsString;

    use super::*;

    /// An argument that isn't valid Unicode: a lone UTF-16 surrogate on Windows, a stray byte
    /// elsewhere. `std::env::args()` panics on one.
    fn not_unicode() -> OsString {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[0x0041, 0xD800, 0x0042])
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![b'A', 0xFF, b'B'])
        }
    }

    #[test]
    fn a_non_unicode_argument_is_read_lossily_not_a_panic() {
        assert!(
            not_unicode().to_str().is_none(),
            "the test argument really isn't Unicode"
        );
        let args: Vec<String> = lossy_args([
            OsString::from(r"C:\tug\tug.exe"),
            not_unicode(),
            OsString::from("--action"),
            OsString::from("settings"),
        ])
        .collect();
        assert_eq!(args[1], "A\u{FFFD}B");
        // tug's own flags still read normally around it.
        assert_eq!(
            quick_actions::parse_args(&args),
            Some(quick_actions::QuickAction::Settings)
        );
        assert!(!args.iter().any(|a| a == MINIMIZED_ARG));
        assert_eq!(run_uninstall_step(args.into_iter().skip(1)), None);
        // This process's own command line reads without panicking.
        assert!(launch_args().count() >= 1);
    }
}
