//! Tauri commands invoked from the Vue frontend (see `src/lib/ipc.ts`).

use std::collections::HashMap;
use std::sync::Arc;

use tauri::State;

use crate::ams::{NowPlaying, RemoteCommand, RepeatMode};
use crate::ble::{BleHandle, Command};
use crate::map::calls::CallRecord;
use crate::messages::{Contact, StoredMessage};
use crate::spotify::{
    AlbumDetail, Artist, ArtistDetail, Device, Playlist, Queue, Spotify, SpotifyPlayer, SpotifySearch, SpotifyStatus,
    Track,
};
use crate::state::{DeviceStatus, Shared};
use crate::store::StoredNotification;
use crate::tugboat::session::Skipped;
use crate::tugboat::{TugboatService, TugboatStatus};
use serde::Serialize;

pub struct AppState {
    pub shared: Arc<Shared>,
    pub ble: BleHandle,
    pub spotify: Arc<Spotify>,
    pub tugboat: TugboatService,
}

type Result<T> = std::result::Result<T, String>;

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> DeviceStatus {
    state.shared.status()
}

#[tauri::command]
pub fn get_now_playing(state: State<'_, AppState>) -> NowPlaying {
    state.shared.now_playing()
}

#[tauri::command]
pub fn list_notifications(
    state: State<'_, AppState>,
    before_id: Option<i64>,
    limit: u32,
) -> Result<Vec<StoredNotification>> {
    state
        .shared
        .store
        .recent(limit.min(500), before_id, state.shared.live_session().as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn search_notifications(state: State<'_, AppState>, query: String, limit: u32) -> Result<Vec<StoredNotification>> {
    state
        .shared
        .store
        .search(&query, limit.min(500), state.shared.live_session().as_deref())
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    pub people: Vec<Contact>,
    pub messages: Vec<StoredMessage>,
    pub notifications: Vec<StoredNotification>,
}

/// One search across people, texts and notifications (universal search).
#[tauri::command]
pub fn search_all(state: State<'_, AppState>, query: String, limit: u32) -> Result<SearchResults> {
    let limit = limit.clamp(1, 200);
    let store = &state.shared.store;
    let live = state.shared.live_session();
    let has_text = !query.trim().is_empty();
    Ok(SearchResults {
        people: store.search_contacts(&query, limit).map_err(|e| e.to_string())?,
        messages: store.search_messages(&query, limit).map_err(|e| e.to_string())?,
        notifications: if has_text {
            store
                .search(&query, limit, live.as_deref())
                .map_err(|e| e.to_string())?
        } else {
            Vec::new()
        },
    })
}

#[tauri::command]
pub fn clear_history(state: State<'_, AppState>) -> Result<()> {
    state.shared.store.clear_history().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn perform_action(state: State<'_, AppState>, id: i64, positive: bool) -> Result<()> {
    state
        .ble
        .request(|reply| Command::PerformAction { id, positive, reply })
        .await
}

#[tauri::command]
pub async fn media_command(state: State<'_, AppState>, command: String) -> Result<()> {
    let command = RemoteCommand::parse(&command).ok_or_else(|| format!("unknown media command {command}"))?;
    state.ble.request(|reply| Command::Media { command, reply }).await
}

#[tauri::command]
pub fn start_discovery(state: State<'_, AppState>) {
    state.ble.send(Command::StartDiscovery);
}

#[tauri::command]
pub fn stop_discovery(state: State<'_, AppState>) {
    state.ble.send(Command::StopDiscovery);
}

/// Re-run the iPhone inquiry (the find step calls this every ~15 s) so a phone made discoverable
/// after scanning began still appears, without disturbing the LE watcher or the rows already listed.
#[tauri::command]
pub fn rescan_discovery(state: State<'_, AppState>) {
    state.ble.send(Command::RescanDiscovery);
}

/// Unpair a leftover phone's LE and Classic bonds (both, matched by name), then keep scanning so it
/// reappears unpaired and ready to pair fresh.
#[tauri::command]
pub async fn remove_pairing(state: State<'_, AppState>, id: String) -> Result<()> {
    state.ble.request(|reply| Command::RemovePairing { id, reply }).await
}

#[tauri::command]
pub async fn pair_device(state: State<'_, AppState>, id: String) -> Result<()> {
    state.ble.request(|reply| Command::Pair { id, reply }).await
}

#[tauri::command]
pub fn confirm_pairing(state: State<'_, AppState>, accept: bool) {
    if let Some(tx) = state.shared.take_pairing_confirm() {
        let _ = tx.send(accept);
    }
}

#[tauri::command]
pub async fn use_device(state: State<'_, AppState>, id: String) -> Result<()> {
    state.ble.request(|reply| Command::UseDevice { id, reply }).await
}

#[tauri::command]
pub async fn forget_device(state: State<'_, AppState>) -> Result<()> {
    state.ble.request(|reply| Command::Forget { reply }).await
}

/// Pair the iPhone's Classic (texts) side from inside tug (setup's Texts step). The PIN shows in
/// tug via the pairing-request event, the same as notifications pairing.
#[tauri::command]
pub async fn pair_texts(state: State<'_, AppState>) -> Result<()> {
    state.ble.request(|reply| Command::PairTexts { reply }).await
}

#[tauri::command]
pub async fn set_advertising(state: State<'_, AppState>, enabled: bool) -> Result<()> {
    state
        .ble
        .request(|reply| Command::SetAdvertising { enabled, reply })
        .await
}

#[tauri::command]
pub fn list_messages(state: State<'_, AppState>, limit: u32) -> Result<Vec<StoredMessage>> {
    state
        .shared
        .store
        .recent_messages(limit.min(5000))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_contacts(state: State<'_, AppState>) -> Result<Vec<Contact>> {
    state.shared.store.contacts().map_err(|e| e.to_string())
}

/// Send a reply through the iPhone. Resolves with the stored message even when the phone didn't
/// take it (status failed, shown with Retry); an error means nothing was saved, so the composer
/// keeps the text.
#[tauri::command]
pub async fn send_message(state: State<'_, AppState>, address: String, text: String) -> Result<StoredMessage> {
    crate::map::service::send_text(&state.shared, &address, &text).await
}

/// Send a failed message again: the same message, to the same number.
#[tauri::command]
pub async fn retry_message(state: State<'_, AppState>, id: i64) -> Result<StoredMessage> {
    crate::map::service::retry_text(&state.shared, id).await
}

/// The phone's recent calls (PBAP call history), newest first.
#[tauri::command]
pub fn get_calls(state: State<'_, AppState>) -> Vec<CallRecord> {
    state.shared.calls()
}

/// Recent calls are on screen: pull them again soon (throttled in the service).
#[tauri::command]
pub fn refresh_calls(state: State<'_, AppState>) {
    if let Some(map) = state.shared.map.get() {
        map.refresh_calls(std::time::Duration::ZERO);
    }
}

/// Experimental: place a call on the iPhone over its hands-free link (see `hfp`). Without a
/// number it only checks that the link can be opened, which Settings does before Call
/// buttons are shown.
#[tauri::command]
pub async fn dial(state: State<'_, AppState>, number: Option<String>) -> Result<()> {
    let map = state.shared.map.get().cloned().ok_or("Message service isn't running")?;
    map.dial(number).await
}

/// The PC's location, for the weather widget. Only called when the user asks.
#[tauri::command]
pub async fn locate() -> Result<crate::location::Position> {
    tauri::async_runtime::spawn_blocking(crate::location::locate)
        .await
        .map_err(|e| e.to_string())?
}

/// An app's real icon (data URI) for the Feed, fetched once from the App Store and cached.
/// Off when the user switched App icons off in Data & privacy.
#[tauri::command]
pub async fn app_icon(app: tauri::AppHandle, state: State<'_, AppState>, app_id: String) -> Result<Option<String>> {
    use tauri::Manager;
    if state.shared.store.setting("ui.appIcons").ok().flatten().as_deref() == Some("false") {
        return Ok(None);
    }
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || crate::app_icons::icon(&dir, &app_id))
        .await
        .map_err(|e| e.to_string())?
}

/// The app's own website (from the same App Store lookup as its icon), for "Open" on apps tug
/// has no page for. Off with App icons, since it sends the app's ID the same way.
#[tauri::command]
pub async fn app_website(app: tauri::AppHandle, state: State<'_, AppState>, app_id: String) -> Result<Option<String>> {
    use tauri::Manager;
    if state.shared.store.setting("ui.appIcons").ok().flatten().as_deref() == Some("false") {
        return Ok(None);
    }
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || crate::app_icons::website(&dir, &app_id))
        .await
        .map_err(|e| e.to_string())?
}

/// A contact's photo (data URI) for their avatar, when the iPhone shared one over PBAP. `key` is a
/// phone number or a name as the avatar shows it; the number is matched exactly, the name only when
/// one contact of that name has a photo. `None` falls the UI back to initials. Photos are local
/// only — nothing is sent anywhere — so, unlike app icons, there's no setting to gate this.
#[tauri::command]
pub async fn contact_photo(app: tauri::AppHandle, state: State<'_, AppState>, key: String) -> Result<Option<String>> {
    use tauri::Manager;
    let number = crate::map::address::normalize(&key);
    let hash = state
        .shared
        .store
        .contact_photo_key(Some(&number), Some(&key))
        .map_err(|e| e.to_string())?;
    let Some(hash) = hash else {
        return Ok(None);
    };
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || crate::contact_photos::photo_data_uri(&dir, &hash))
        .await
        .map_err(|e| e.to_string())
}

/// The reverse-lookup answer for coordinates (JSON), for naming "Use my location".
#[tauri::command]
pub async fn place_lookup(latitude: f64, longitude: f64) -> Result<String> {
    tauri::async_runtime::spawn_blocking(move || crate::location::place_lookup(latitude, longitude))
        .await
        .map_err(|e| e.to_string())?
}

/// The UI is showing the iPhone's switches (or stopped): check them every couple of seconds.
#[tauri::command]
pub fn set_watching(state: State<'_, AppState>, on: bool) {
    if state.shared.set_watching(on) {
        if let Some(map) = state.shared.map.get() {
            map.refresh();
        }
    }
}

/// The user opened these messages in tug: mark them read on the phone too.
#[tauri::command]
pub fn mark_read(state: State<'_, AppState>, message_ids: Vec<i64>) {
    if let Some(map) = state.shared.map.get() {
        map.mark_read(message_ids);
    }
}

/// Delete a conversation from tug (`hidden: true`) or undo that. Local only.
#[tauri::command]
pub fn set_hidden(
    state: State<'_, AppState>,
    notification_ids: Vec<i64>,
    message_ids: Vec<i64>,
    hidden: bool,
) -> Result<()> {
    let at = hidden.then(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    });
    state
        .shared
        .store
        .set_hidden(&notification_ids, &message_ids, at)
        .map_err(|e| e.to_string())
}

/// Open a page of Windows Settings (setup's "Open Bluetooth settings" and friends). Only
/// known pages: the webview never gets to launch arbitrary URIs.
#[tauri::command]
pub fn open_windows_settings(page: String) -> Result<()> {
    let uri = match page.as_str() {
        "bluetooth" => "ms-settings:bluetooth",
        "location" => "ms-settings:privacy-location",
        "notifications" => "ms-settings:notifications",
        other => return Err(format!("unknown settings page: {other}")),
    };
    std::process::Command::new("explorer.exe")
        .arg(uri)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// True when `url` is a plain http(s) URL with a host: the only thing `open_url` will launch.
/// Pure (no I/O) so it can be unit-tested; see the tests at the bottom of this file. We parse
/// by hand rather than add a URL crate — the check only needs scheme + host and to refuse
/// anything that could be a non-web URI (`file:`, `javascript:`, `ms-settings:`) or carry shell
/// metacharacters to `explorer.exe`.
pub fn is_http_url(url: &str) -> bool {
    // Nothing weird that a shell or the OS could reinterpret.
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let rest = match url.split_once("://") {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") => rest,
        _ => return false,
    };
    // Authority is up to the first path/query/fragment delimiter.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    // Drop any userinfo (after the last '@') and port (after ':'); a host must remain.
    let host_port = authority.rsplit('@').next().unwrap_or("");
    let host = host_port.split(':').next().unwrap_or("");
    !host.is_empty()
}

/// Open an http(s) link in the default browser (the "Open in browser" action on a notification).
/// Validated first so the webview can't ask us to launch a `file:`/`javascript:`/settings URI;
/// uses the same shell launch as `open_windows_settings`.
#[tauri::command]
pub fn open_url(url: String) -> Result<()> {
    if !is_http_url(&url) {
        return Err(format!("refusing to open non-web URL: {url}"));
    }
    open_in_browser(&url)
}

/// Hand a web address to Windows to open in the default browser. Not `explorer.exe <url>`:
/// on the test PC that opened File Explorer for addresses with a query (`?q=…`).
#[cfg(windows)]
pub(crate) fn open_in_browser(url: &str) -> Result<()> {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let file = HSTRING::from(url);
    // SAFETY: plain FFI call; every pointer argument outlives the call.
    let result = unsafe { ShellExecuteW(None, w!("open"), &file, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    // ShellExecute reports success as a value above 32.
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!("Windows couldn't open the link (code {})", result.0 as isize))
    }
}

#[cfg(not(windows))]
pub(crate) fn open_in_browser(_url: &str) -> Result<()> {
    Err("Opening links is only supported on Windows".into())
}

/// Unread texts, for the tray tooltip and the taskbar dot.
#[tauri::command]
pub fn set_unread(app: tauri::AppHandle, count: u32) -> Result<()> {
    crate::tray::set_unread(&app, count).map_err(|e| e.to_string())
}

/// Pop up a phone notification with the actions the frontend found for it (reply, mark
/// read, copy code, call back, clear). The frontend has already decided it should show
/// (toasts on, not do-not-disturb, app not muted, within the rate limit). Falls back to a
/// plain pop-up if Windows won't take the actionable one.
#[tauri::command]
pub fn show_toast(app: tauri::AppHandle, spec: crate::toast::ToastSpec) {
    crate::toast::show(&app, spec);
}

/// Copy text to the clipboard. Sync on purpose: Tauri runs sync commands on the main
/// (STA) thread, which the WinRT clipboard requires.
#[tauri::command]
pub fn copy_text(text: String) -> Result<()> {
    crate::clipboard::set_text(&text)
}

/// Settings safe to print in a support report: scalar on/off flags, never the "seen"
/// conversation map (its keys embed contact names) or anything carrying message content.
const DIAGNOSTIC_SETTINGS: &[&str] = &[
    "advertise",
    "ui.toasts",
    "ui.doNotDisturb",
    "ui.closeToTray",
    "ui.appIcons",
    "ui.dialing",
    "ui.zoom",
    "ui.onboarded",
    "ui.seenSince",
];

/// Build the "Copy diagnostics" report and put it on the clipboard, returning it too so the UI
/// can confirm. Sync for the same STA reason as `copy_text`. Message bodies, phone numbers,
/// contact names and emails never reach it: the status snapshot carries none, the settings are
/// an allowlist, and the log tail is redacted line by line.
#[tauri::command]
pub fn copy_diagnostics(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<String> {
    use crate::diagnostics::{self, Report};
    use tauri::Manager;

    let status = state.shared.status();
    // The phone's name often names a person ("Jordan's iPhone"): keep it out of the report.
    let phone_names: Vec<String> = status
        .device
        .iter()
        .map(|d| d.name.clone())
        .chain(status.texts_device.clone())
        .collect();
    let status_json =
        serde_json::to_string_pretty(&diagnostics::anonymize_status(&status)).map_err(|e| e.to_string())?;
    let bluetooth = diagnostics::bluetooth_summary(&status);
    let windows_version = diagnostics::os_version();

    let raw = state.shared.store.settings().map_err(|e| e.to_string())?;
    let mut settings: Vec<(String, String)> = DIAGNOSTIC_SETTINGS
        .iter()
        .filter_map(|k| raw.get(*k).map(|v| (k.to_string(), v.clone())))
        .collect();
    // Muted apps as a count only (the bundle ids themselves aren't sensitive, but the count is
    // all support needs).
    if let Some(v) = raw.get("ui.mutedApps") {
        let count = serde_json::from_str::<Vec<String>>(v).map(|a| a.len()).unwrap_or(0);
        settings.push(("ui.mutedApps".to_string(), format!("{count} muted")));
    }

    let log_lines = app
        .path()
        .app_log_dir()
        .map(|dir| diagnostics::recent_log_lines(&dir, diagnostics::LOG_TAIL_LINES))
        .unwrap_or_default();

    let app_version = app.package_info().version.to_string();
    let report = Report {
        app_version: &app_version,
        windows_version: &windows_version,
        bluetooth: &bluetooth,
        status_json: &status_json,
        settings: &settings,
        log_lines: &log_lines,
        phone_names: &phone_names,
    }
    .render();
    // The latest Bluetooth inventory (privacy-safe by construction: UUIDs, names, counts).
    let inventory = crate::bt_inventory::diagnostics_section(crate::bt_inventory::last_report().as_ref());
    let report = format!("{report}\n{inventory}");

    crate::clipboard::set_text(&report)?;
    Ok(report)
}

/// The Bluetooth inventory: what the iPhone exposes to tug (GATT services, Device Information,
/// time, battery, AMS/ANCS, PBAP/MAP, SDP, link). Runs it now (also logging it as `bt-inventory:`)
/// or joins the one already running. Privacy-safe: no names, numbers or message text.
#[tauri::command]
pub async fn bt_inventory(state: State<'_, AppState>) -> Result<crate::bt_inventory::BtInventory> {
    let (reply, rx) = tokio::sync::oneshot::channel();
    state.ble.send(Command::Inventory { reply });
    rx.await.map_err(|_| "Bluetooth service stopped".to_string())
}

/// Open tug's log folder in Explorer, so the owner can attach the files to a support message.
#[tauri::command]
pub fn open_logs_folder(app: tauri::AppHandle) -> Result<()> {
    use tauri::Manager;
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::process::Command::new("explorer.exe")
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn refresh_messages(state: State<'_, AppState>) {
    if let Some(map) = state.shared.map.get() {
        map.refresh();
    }
}

/// UI preferences only; Bluetooth settings go through their own commands.
const UI_SETTING_PREFIX: &str = "ui.";

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<HashMap<String, String>> {
    let mut all = state.shared.store.settings().map_err(|e| e.to_string())?;
    all.retain(|k, _| k.starts_with(UI_SETTING_PREFIX) || k == crate::state::keys::ADVERTISE);
    Ok(all)
}

#[tauri::command]
pub fn set_setting(state: State<'_, AppState>, key: String, value: String) -> Result<()> {
    if !key.starts_with(UI_SETTING_PREFIX) {
        return Err(format!("{key} is not a UI setting"));
    }
    state.shared.store.set_setting(&key, &value).map_err(|e| e.to_string())
}

/// Whether tug starts with Windows. The registry entry (via the autostart plugin) is the
/// source of truth, so the Settings switch reads this rather than a stored preference.
#[tauri::command]
pub fn get_autostart(app: tauri::AppHandle) -> Result<bool> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

/// Turn "start with Windows" on or off. On writes an entry that launches tug with
/// `--minimized`, so it comes up hidden in the tray.
#[tauri::command]
pub fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<()> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|e| e.to_string())
    } else {
        manager.disable().map_err(|e| e.to_string())
    }
}

// ---- Spotify connector (see src-tauri/src/spotify) ----
//
// AMS can't give Spotify repeat/shuffle, Like or album art; when the owner connects their own
// Spotify app these augment Now Playing. All HTTP runs on a blocking thread (WinRT, like
// `app_icon`); the command layer just hands off and maps join errors.

/// Spotify connection state for Settings (connected, account, Client ID, redirect URI).
#[tauri::command]
pub fn spotify_status(state: State<'_, AppState>) -> SpotifyStatus {
    state.spotify.status()
}

/// Run the OAuth (PKCE) connect flow: opens the browser, waits for the loopback redirect.
#[tauri::command]
pub async fn spotify_connect(state: State<'_, AppState>) -> Result<SpotifyStatus> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.connect())
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn spotify_disconnect(state: State<'_, AppState>) -> Result<SpotifyStatus> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.disconnect().map(|()| sp.status()))
        .await
        .map_err(|e| e.to_string())?
}

/// The user's own and followed playlists (paginated), for the Playlists panel and Ctrl+K.
#[tauri::command]
pub async fn spotify_playlists(state: State<'_, AppState>) -> Result<Vec<Playlist>> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.playlists())
        .await
        .map_err(|e| e.to_string())?
}

/// A playlist cover as a `data:` URI (fetched and cached by tug; only Spotify image hosts).
#[tauri::command]
pub async fn spotify_cover(state: State<'_, AppState>, url: String) -> Result<Option<String>> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.cover(&url))
        .await
        .map_err(|e| e.to_string())
}

/// Play a context (playlist, album or artist) on a device: the one picked, else the iPhone.
#[tauri::command]
pub async fn spotify_play_context(state: State<'_, AppState>, uri: String, device_id: Option<String>) -> Result<()> {
    let sp = state.spotify.clone();
    let phone = state.shared.status().device.map(|d| d.name);
    tauri::async_runtime::spawn_blocking(move || sp.play_context(&uri, device_id.as_deref(), phone.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

/// Play one track, optionally inside a context (its album/playlist) so the queue keeps going.
#[tauri::command]
pub async fn spotify_play_track(
    state: State<'_, AppState>,
    uri: String,
    context_uri: Option<String>,
    device_id: Option<String>,
) -> Result<()> {
    let sp = state.spotify.clone();
    let phone = state.shared.status().device.map(|d| d.name);
    tauri::async_runtime::spawn_blocking(move || {
        sp.play_track(&uri, context_uri.as_deref(), device_id.as_deref(), phone.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Search tracks/albums/artists/playlists. `kinds` picks the types; `offset` pages results.
#[tauri::command]
pub async fn spotify_search(
    state: State<'_, AppState>,
    query: String,
    kinds: Vec<String>,
    offset: u32,
) -> Result<SpotifySearch> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.search(&query, &kinds, offset))
        .await
        .map_err(|e| e.to_string())?
}

/// The current playback queue (what's playing and what's next).
#[tauri::command]
pub async fn spotify_queue(state: State<'_, AppState>) -> Result<Queue> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.queue())
        .await
        .map_err(|e| e.to_string())?
}

/// Add a track to the active device's queue.
#[tauri::command]
pub async fn spotify_add_to_queue(state: State<'_, AppState>, uri: String) -> Result<()> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.add_to_queue(&uri))
        .await
        .map_err(|e| e.to_string())?
}

/// Recently played tracks, newest first.
#[tauri::command]
pub async fn spotify_recently_played(state: State<'_, AppState>) -> Result<Vec<Track>> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.recently_played())
        .await
        .map_err(|e| e.to_string())?
}

/// Top tracks over a time range (short_term / medium_term / long_term).
#[tauri::command]
pub async fn spotify_top_tracks(state: State<'_, AppState>, time_range: String) -> Result<Vec<Track>> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.top_tracks(&time_range))
        .await
        .map_err(|e| e.to_string())?
}

/// Top artists over a time range.
#[tauri::command]
pub async fn spotify_top_artists(state: State<'_, AppState>, time_range: String) -> Result<Vec<Artist>> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.top_artists(&time_range))
        .await
        .map_err(|e| e.to_string())?
}

/// Devices playback can be sent to, for the "Play on" picker.
#[tauri::command]
pub async fn spotify_devices(state: State<'_, AppState>) -> Result<Vec<Device>> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.devices())
        .await
        .map_err(|e| e.to_string())?
}

/// Transfer playback to a device, keeping it playing.
#[tauri::command]
pub async fn spotify_transfer(state: State<'_, AppState>, device_id: String) -> Result<()> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.transfer(&device_id))
        .await
        .map_err(|e| e.to_string())?
}

/// Seek the active device to a position (ms) in the current track.
#[tauri::command]
pub async fn spotify_seek(state: State<'_, AppState>, position_ms: u32) -> Result<()> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.seek(position_ms))
        .await
        .map_err(|e| e.to_string())?
}

/// An album and its songs.
#[tauri::command]
pub async fn spotify_album(state: State<'_, AppState>, id: String) -> Result<AlbumDetail> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.album(&id))
        .await
        .map_err(|e| e.to_string())?
}

/// An artist and their albums.
#[tauri::command]
pub async fn spotify_artist(state: State<'_, AppState>, id: String) -> Result<ArtistDetail> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.artist(&id))
        .await
        .map_err(|e| e.to_string())?
}

/// The songs in a playlist the user owns or collaborates on (empty for followed playlists).
#[tauri::command]
pub async fn spotify_playlist_items(state: State<'_, AppState>, id: String) -> Result<Vec<Track>> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.playlist_items(&id))
        .await
        .map_err(|e| e.to_string())?
}

/// Add a track to a playlist the user owns.
#[tauri::command]
pub async fn spotify_add_to_playlist(state: State<'_, AppState>, playlist_id: String, track_uri: String) -> Result<()> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.add_to_playlist(&playlist_id, &track_uri))
        .await
        .map_err(|e| e.to_string())?
}

/// The Spotify playback snapshot (repeat/shuffle/saved/album art) for the Now Playing card.
#[tauri::command]
pub async fn spotify_player(state: State<'_, AppState>) -> Result<Option<SpotifyPlayer>> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.player())
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn spotify_set_repeat(state: State<'_, AppState>, mode: String) -> Result<()> {
    let mode = match mode.as_str() {
        "off" => RepeatMode::Off,
        "one" => RepeatMode::One,
        "all" => RepeatMode::All,
        other => return Err(format!("unknown repeat mode {other}")),
    };
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.set_repeat(mode))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn spotify_set_shuffle(state: State<'_, AppState>, on: bool) -> Result<()> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.set_shuffle(on))
        .await
        .map_err(|e| e.to_string())?
}

/// Like (save) or un-like (remove) a track in the user's Spotify library.
#[tauri::command]
pub async fn spotify_set_saved(state: State<'_, AppState>, uri: String, saved: bool) -> Result<()> {
    let sp = state.spotify.clone();
    tauri::async_runtime::spawn_blocking(move || sp.set_saved(&uri, saved))
        .await
        .map_err(|e| e.to_string())?
}

/// An uncaught frontend error (window.onerror, an unhandled promise rejection, or a Vue error),
/// forwarded so a window crash leaves a trace in the log. Logged at warn, redacted and rate-limited
/// (see `frontend_log`); the frontend only ever sends the error's name, a trimmed message and the
/// top of the stack, never a message body or a phone number.
#[tauri::command]
pub fn log_frontend_error(kind: String, name: String, message: String, source: String) {
    use std::sync::{Mutex, OnceLock};
    static LIMITER: OnceLock<Mutex<crate::frontend_log::RateLimiter>> = OnceLock::new();
    let limiter = LIMITER.get_or_init(|| Mutex::new(crate::frontend_log::RateLimiter::default()));
    let allowed = limiter
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .allow(std::time::Instant::now());
    if allowed {
        log::warn!("{}", crate::frontend_log::summary(&kind, &name, &message, &source));
    }
}

// --- Tugboat (phone <-> PC over the local Wi-Fi; see tugboat/mod.rs) ---

/// Open Tugboat: a fresh secret and port, and the QR code to scan. Returns the session already
/// open if there is one.
#[tauri::command]
pub async fn tugboat_start(state: State<'_, AppState>) -> Result<TugboatStatus> {
    state.tugboat.start().await
}

/// Close Tugboat: stop listening, invalidate the secret, remove unfinished uploads.
#[tauri::command]
pub async fn tugboat_stop(state: State<'_, AppState>) -> Result<()> {
    state.tugboat.stop(None).await;
    Ok(())
}

#[tauri::command]
pub fn tugboat_status(state: State<'_, AppState>) -> TugboatStatus {
    state.tugboat.status()
}

/// "Copy link" in the Tugboat panel: the QR link, kept out of clipboard history and sync. Sync on
/// purpose, like `copy_text`: the WinRT clipboard needs the main (STA) thread. (Files dropped onto
/// tug are offered from Rust, in lib.rs; no command takes a path from the page.)
#[tauri::command]
pub fn tugboat_copy_link(state: State<'_, AppState>) -> Result<()> {
    state.tugboat.copy_link()
}

/// "Choose files" in the Tugboat panel: Windows' file picker, then offer what was picked.
#[tauri::command]
pub async fn tugboat_pick_files(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<Vec<Skipped>> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_files())
        .await
        .map_err(|e| e.to_string())?;
    let paths: Vec<std::path::PathBuf> = picked
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .collect();
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    state.tugboat.offer(paths).await
}

#[tauri::command]
pub fn tugboat_remove_offer(state: State<'_, AppState>, id: String) {
    state.tugboat.remove_offer(&id);
}

/// Text for the phone to copy (empty clears it).
#[tauri::command]
pub fn tugboat_send_text(state: State<'_, AppState>, text: String) -> Result<()> {
    state.tugboat.send_text(&text)
}

/// Open the Tugboat folder, or select one file Tugboat saved this session.
#[tauri::command]
pub fn tugboat_open_folder(state: State<'_, AppState>, path: Option<String>) -> Result<()> {
    state.tugboat.open_folder(path.as_deref())
}

#[cfg(test)]
mod tests {
    use super::is_http_url;

    #[test]
    fn accepts_http_and_https_with_a_host() {
        assert!(is_http_url("https://mail.google.com/mail/u/0/#inbox"));
        assert!(is_http_url("http://example.com"));
        assert!(is_http_url("https://x.com/notifications"));
        assert!(is_http_url("https://host:8443/path?q=1#frag"));
        assert!(is_http_url("HTTPS://Example.com")); // scheme is case-insensitive
                                                     // A percent-encoded Gmail search is a real query we build.
        assert!(is_http_url(
            "https://mail.google.com/mail/u/0/#search/from%3A%22Jane%20Doe%22"
        ));
    }

    #[test]
    fn rejects_non_web_schemes() {
        assert!(!is_http_url("file:///C:/Windows/System32/calc.exe"));
        assert!(!is_http_url("javascript:alert(1)"));
        assert!(!is_http_url("ms-settings:bluetooth"));
        assert!(!is_http_url("ftp://example.com"));
        assert!(!is_http_url("mailto:someone@example.com"));
        assert!(!is_http_url("example.com")); // no scheme
    }

    #[test]
    fn rejects_missing_host() {
        assert!(!is_http_url("https://"));
        assert!(!is_http_url("http:///just/a/path"));
        assert!(!is_http_url("https://user@"));
    }

    #[test]
    fn rejects_whitespace_and_control_chars() {
        assert!(!is_http_url("https://example.com/a b"));
        assert!(!is_http_url("https://example.com /x")); // would split into a second arg
        assert!(!is_http_url("https://exa\nmple.com"));
        assert!(!is_http_url("https://example.com\t"));
    }
}
