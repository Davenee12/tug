//! Tauri commands invoked from the Vue frontend (see `src/lib/ipc.ts`).

use std::collections::HashMap;
use std::sync::Arc;

use tauri::State;

use crate::ams::{NowPlaying, RemoteCommand};
use crate::ble::{BleHandle, Command};
use crate::messages::{Contact, StoredMessage};
use crate::state::{DeviceStatus, Shared};
use crate::store::StoredNotification;

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
