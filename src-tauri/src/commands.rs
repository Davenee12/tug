//! Tauri commands invoked from the Vue frontend (see `src/lib/ipc.ts`).

use std::collections::HashMap;
use std::sync::Arc;

use tauri::State;

use crate::ams::{NowPlaying, RemoteCommand};
use crate::ble::{BleHandle, Command};
use crate::map::calls::CallRecord;
use crate::messages::{Contact, StoredMessage};
use crate::state::{DeviceStatus, Shared};
use crate::store::StoredNotification;
use serde::Serialize;

pub struct AppState {
    pub shared: Arc<Shared>,
    pub ble: BleHandle,
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

#[tauri::command]
pub async fn send_message(state: State<'_, AppState>, address: String, text: String) -> Result<StoredMessage> {
    let map = state.shared.map.get().cloned().ok_or("Message service isn't running")?;
    map.send(address, text).await
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
    std::process::Command::new("explorer.exe")
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Unread texts, for the tray tooltip and the taskbar dot.
#[tauri::command]
pub fn set_unread(app: tauri::AppHandle, count: u32) -> Result<()> {
    crate::tray::set_unread(&app, count).map_err(|e| e.to_string())
}

/// Copy text to the clipboard. Sync on purpose: Tauri runs sync commands on the main
/// (STA) thread, which the WinRT clipboard requires.
#[tauri::command]
pub fn copy_text(text: String) -> Result<()> {
    crate::clipboard::set_text(&text)
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
