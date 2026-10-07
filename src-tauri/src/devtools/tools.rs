//! What each bridge call does, over tug's own history, phone link and Tugboat. Arguments have
//! already been checked (`Call::validate`), the switch and rate limit too (`mod.rs`).

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use tauri::{Emitter, Manager};
use tug_bridge::protocol::{
    BridgeError, ClientInfo, CodeResult, ErrorCode, HitKind, MediaAction, MessageHit, NowPlayingInfo, OfferResult,
    PhoneStatus, Request, SkippedFile, StatusResult,
};
use tug_bridge::time::iso_utc;

use super::recipient::{resolve, Recipient};
use super::{dev_apps, files, phone_model, DevTools};
use crate::ams::{PlaybackState, RemoteCommand};
use crate::ble::Command;
use crate::messages::{Direction, StoredMessage};
use crate::state::{now_ms, ConnectionState};
use crate::store::StoredNotification;
use crate::tugboat::session::SkipReason;

/// Codes older than this aren't handed out (the Feed's Ctrl+Shift+C uses the same window).
const CODE_MAX_AGE_MS: i64 = 10 * 60 * 1000;
/// How much recent history the code and developer-notification tools look through.
const SCAN: u32 = 500;
const MEDIA_TIMEOUT: Duration = Duration::from_secs(10);

fn internal(e: impl std::fmt::Display) -> BridgeError {
    log::warn!("devtools: {e}");
    BridgeError::new(ErrorCode::Internal, "Something went wrong in tug. Try again.")
}

fn json<T: Serialize>(v: T) -> Result<serde_json::Value, BridgeError> {
    serde_json::to_value(v).map_err(internal)
}

fn notification_app(n: &StoredNotification) -> String {
    n.app_name.clone().unwrap_or_else(|| n.app_id.clone())
}

/// A notification's text the way the Feed reads it: subtitle and message.
fn notification_text(n: &StoredNotification) -> String {
    match (n.subtitle.trim(), n.message.trim()) {
        ("", m) => m.to_string(),
        (s, "") => s.to_string(),
        (s, m) => format!("{s} — {m}"),
    }
}

fn message_from(m: &StoredMessage) -> String {
    match m.direction {
        Direction::Out => "You".into(),
        Direction::In => m.contact_name.clone().unwrap_or_else(|| m.address.clone()),
    }
}

fn message_hit(m: &StoredMessage) -> (i64, MessageHit) {
    (
        m.received_at,
        MessageHit {
            kind: HitKind::Text,
            from: message_from(m),
            app: "Messages".into(),
            time: iso_utc(m.received_at),
            text: m.body.clone(),
        },
    )
}

fn notification_hit(n: &StoredNotification) -> (i64, MessageHit) {
    (
        n.received_at,
        MessageHit {
            kind: HitKind::Notification,
            from: n.title.clone(),
            app: notification_app(n),
            time: iso_utc(n.received_at),
            text: notification_text(n),
        },
    )
}

pub(super) async fn run(
    dt: &DevTools,
    client: &ClientInfo,
    request: Request,
) -> Result<serde_json::Value, BridgeError> {
    match request {
        Request::LatestCode { copy } => latest_code(dt, copy).await,
        Request::Search { query, limit, since_ms } => search(dt, &query, limit, since_ms),
        Request::DevNotifications { since_ms, limit } => dev_notifications(dt, since_ms, limit),
        Request::ListFiles { limit, since_ms } => {
            let folder = tugboat_folder(dt)?;
            let list = tokio::task::spawn_blocking(move || files::list(&folder, limit as usize, since_ms))
                .await
                .map_err(internal)?;
            json(list)
        }
        Request::GetFile { name } => {
            let folder = tugboat_folder(dt)?;
            let got = tokio::task::spawn_blocking(move || files::read(&folder, &name))
                .await
                .map_err(internal)?;
            json(got.map_err(|e| BridgeError::new(ErrorCode::NotFound, e))?)
        }
        Request::PhoneStatus => json(phone_status(dt)),
        Request::Media(action) => media(dt, action).await,
        Request::SendText { to, message } => send_text(dt, client, &to, message).await,
        Request::OfferFiles { paths } => offer_files(dt, paths).await,
        Request::Status => json(StatusResult {
            version: dt.app.package_info().version.to_string(),
            permissions: dt.permissions(),
            phone: phone_status(dt),
        }),
    }
}

async fn latest_code(dt: &DevTools, copy: bool) -> Result<serde_json::Value, BridgeError> {
    let store = &dt.shared.store;
    let live = dt.shared.live_session();
    let now = now_ms();
    let since = now - CODE_MAX_AGE_MS;
    let mut best: Option<(i64, String, String, String)> = None;
    let mut consider = |at: i64, text: &str, from: String, app: String| {
        if at < since || best.as_ref().is_some_and(|b| b.0 >= at) {
            return;
        }
        if let Some(found) = crate::codes::find_code(text) {
            best = Some((at, found.code, from, app));
        }
    };
    for n in store.recent(SCAN, None, live.as_deref()).map_err(internal)? {
        let text = if n.message.is_empty() { &n.subtitle } else { &n.message };
        consider(n.received_at, text, n.title.clone(), notification_app(&n));
    }
    for m in store.recent_messages(SCAN).map_err(internal)? {
        if m.direction == Direction::In {
            consider(m.received_at, &m.body, message_from(&m), "Messages".into());
        }
    }
    let Some((at, code, from, app)) = best else {
        return Err(BridgeError::new(
            ErrorCode::NotFound,
            "No verification code in the last 10 minutes.",
        ));
    };
    let copied = if copy {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let c = code.clone();
        // The WinRT clipboard wants the main thread. Kept out of clipboard history and sync.
        dt.app
            .run_on_main_thread(move || {
                let _ = tx.send(crate::clipboard::set_text_private(&c));
            })
            .map_err(internal)?;
        matches!(tokio::time::timeout(Duration::from_secs(5), rx).await, Ok(Ok(Ok(()))))
    } else {
        false
    };
    json(CodeResult {
        code,
        from,
        app,
        received_at: iso_utc(at),
        age_seconds: ((now - at) / 1000).max(0),
        copied,
    })
}

fn search(dt: &DevTools, query: &str, limit: u32, since_ms: Option<i64>) -> Result<serde_json::Value, BridgeError> {
    let store = &dt.shared.store;
    let live = dt.shared.live_session();
    let since = since_ms.unwrap_or(i64::MIN);
    let mut hits: Vec<(i64, MessageHit)> = store
        .search_messages(query, limit)
        .map_err(internal)?
        .iter()
        .filter(|m| m.received_at >= since)
        .map(message_hit)
        .collect();
    hits.extend(
        store
            .search(query, limit, live.as_deref())
            .map_err(internal)?
            .iter()
            .filter(|n| n.received_at >= since)
            .map(notification_hit),
    );
    hits.sort_by_key(|h| std::cmp::Reverse(h.0));
    hits.truncate(limit as usize);
    json(hits.into_iter().map(|(_, h)| h).collect::<Vec<_>>())
}

fn dev_notifications(dt: &DevTools, since_ms: i64, limit: u32) -> Result<serde_json::Value, BridgeError> {
    let live = dt.shared.live_session();
    let hits: Vec<MessageHit> = dt
        .shared
        .store
        .recent(SCAN, None, live.as_deref())
        .map_err(internal)?
        .iter()
        .filter(|n| n.received_at >= since_ms)
        .filter_map(|n| {
            let app = dev_apps::dev_app(&n.app_id, n.app_name.as_deref())?;
            let (_, mut hit) = notification_hit(n);
            hit.app = app.to_string();
            Some(hit)
        })
        .take(limit as usize)
        .collect();
    json(hits)
}

fn tugboat_folder(dt: &DevTools) -> Result<PathBuf, BridgeError> {
    dt.app
        .path()
        .picture_dir()
        .map(|p| p.join("Tugboat"))
        .map_err(|_| BridgeError::new(ErrorCode::NotFound, "Couldn't find your Pictures folder."))
}

fn phone_status(dt: &DevTools) -> PhoneStatus {
    let s = dt.shared.status();
    let np = dt.shared.now_playing();
    let connection = match s.connection {
        ConnectionState::NoDevice => "no_phone",
        ConnectionState::Disconnected => "disconnected",
        ConnectionState::Connecting => "connecting",
        ConnectionState::Connected => "connected",
    };
    let connected = s.connection == ConnectionState::Connected;
    let state = match np.state {
        PlaybackState::Playing | PlaybackState::FastForwarding | PlaybackState::Rewinding => "playing",
        PlaybackState::Paused => "paused",
        PlaybackState::Unknown => "stopped",
    };
    let now_playing =
        (connected && s.services.media && np.title.as_deref().is_some_and(|t| !t.is_empty())).then(|| NowPlayingInfo {
            title: np.title.clone(),
            artist: np.artist.clone(),
            album: np.album.clone(),
            state: state.into(),
            app: np.player.clone(),
        });
    PhoneStatus {
        connected,
        connection: connection.into(),
        model: s
            .device
            .as_ref()
            .and_then(|d| d.model.as_deref())
            .and_then(phone_model::model_name)
            .map(str::to_string),
        battery_percent: if connected { s.battery } else { None },
        notifications: connected && s.services.notifications,
        texts: s.services.messages,
        now_playing,
    }
}

async fn media(dt: &DevTools, action: MediaAction) -> Result<serde_json::Value, BridgeError> {
    let s = dt.shared.status();
    if s.connection != ConnectionState::Connected || !s.services.media {
        return Err(BridgeError::new(
            ErrorCode::Unavailable,
            "Music controls need your iPhone connected to tug.",
        ));
    }
    let command = match action {
        MediaAction::Play => RemoteCommand::Play,
        MediaAction::Pause => RemoteCommand::Pause,
        MediaAction::Toggle => RemoteCommand::TogglePlayPause,
        MediaAction::Next => RemoteCommand::NextTrack,
        MediaAction::Previous => RemoteCommand::PreviousTrack,
    };
    let sent = tokio::time::timeout(MEDIA_TIMEOUT, dt.ble.request(|reply| Command::Media { command, reply })).await;
    match sent {
        Ok(Ok(())) => json(serde_json::json!({ "done": command.as_str() })),
        Ok(Err(e)) => Err(BridgeError::new(
            ErrorCode::Unavailable,
            format!("The phone didn't take it: {e}"),
        )),
        Err(_) => Err(BridgeError::new(
            ErrorCode::Unavailable,
            "The phone didn't answer in time.",
        )),
    }
}

async fn send_text(
    dt: &DevTools,
    client: &ClientInfo,
    to: &str,
    message: String,
) -> Result<serde_json::Value, BridgeError> {
    if !dt.shared.status().services.messages || dt.shared.map.get().is_none() {
        return Err(BridgeError::new(
            ErrorCode::Unavailable,
            "Texts aren't connected in tug right now, so nothing can be sent.",
        ));
    }
    let store = &dt.shared.store;
    let contacts = store.contacts().map_err(internal)?;
    // Addresses with texts, newest first: between one person's numbers, the one last texted.
    let mut recent: Vec<String> = Vec::new();
    for m in store.recent_messages(SCAN).map_err(internal)?.iter().rev() {
        if !recent.contains(&m.address) {
            recent.push(m.address.clone());
        }
    }
    let (name, address) = match resolve(to, &contacts, &recent) {
        Recipient::One { name, address } => (name, address),
        Recipient::Several(names) => {
            return Err(BridgeError::new(
                ErrorCode::Invalid,
                format!(
                    "More than one contact matches \"{to}\": {}. Say which one.",
                    names.join(", ")
                ),
            ))
        }
        Recipient::NoMatch => {
            return Err(BridgeError::new(
                ErrorCode::NotFound,
                format!("No contact matches \"{to}\". Use their name as it's saved on the phone, or a phone number."),
            ))
        }
    };
    json(dt.confirm_and_send(client, name, address, message).await?)
}

fn skip_reason(r: SkipReason) -> &'static str {
    match r {
        SkipReason::TooBig => "too big",
        SkipReason::Folder => "a folder (send the files inside it)",
        SkipReason::Unreadable => "couldn't be read",
        SkipReason::TooMany => "too many files offered at once",
    }
}

async fn offer_files(dt: &DevTools, paths: Vec<String>) -> Result<serde_json::Value, BridgeError> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let mut skipped: Vec<SkippedFile> = Vec::new();
    let mut existing = Vec::new();
    for p in paths {
        if p.exists() {
            existing.push(p);
        } else {
            skipped.push(SkippedFile {
                name: p
                    .file_name()
                    .map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()),
                reason: "not found".into(),
            });
        }
    }
    if existing.is_empty() {
        return json(OfferResult { offered: 0, skipped });
    }
    // Tugboat stops when tug's window hides, so bring tug forward first; the panel opens on the
    // same event as files dropped onto the window.
    crate::tray::show(&dt.app);
    let n = existing.len();
    let tug_skipped = dt
        .tugboat
        .offer(existing)
        .await
        .map_err(|e| BridgeError::new(ErrorCode::Unavailable, e))?;
    let _ = dt.app.emit(crate::tugboat::DROPPED_EVENT, tug_skipped.clone());
    let offered = n - tug_skipped.len();
    skipped.extend(tug_skipped.into_iter().map(|s| SkippedFile {
        name: s.name,
        reason: skip_reason(s.reason).into(),
    }));
    json(OfferResult { offered, skipped })
}
