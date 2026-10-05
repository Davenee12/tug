//! The Spotify connector.
//!
//! AMS gives tug play/pause/skip for whatever the iPhone is playing, but Spotify exposes no
//! repeat/shuffle commands and no album art over AMS. When the owner connects their own Spotify
//! app (Authorization Code + PKCE, no client secret), tug augments the Now Playing card for the
//! Spotify player with working repeat/shuffle, a Like button, album art, and their playlists —
//! all through the Spotify Web API, called from here (never the web view).
//!
//! AMS stays the source of truth for what's playing and for play/pause; this only adds what AMS
//! can't do, and only while the Spotify player is active. All HTTP is in `http` (WinRT), the
//! refresh token is kept in Windows Credential Manager (`creds`), and the pure logic (PKCE,
//! callback parsing, device choice, error mapping) lives in `auth`/`model`, unit-tested.

mod auth;
mod creds;
mod http;
pub mod listener;
mod model;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use serde_json::Value;

use crate::ams::RepeatMode;
use crate::state::now_ms;
use crate::store::Store;
use http::Method;
use model::ApiError;
pub use model::Playlist;

const API_BASE: &str = "https://api.spotify.com/v1";
/// Settings keys (UI-prefixed, so they ride the normal settings channel; neither is secret —
/// the refresh token is the only secret, and it lives in Credential Manager).
const CLIENT_ID_KEY: &str = "ui.spotifyClientId";
const ACCOUNT_KEY: &str = "ui.spotifyAccount";
/// Refresh the access token this long before it actually expires.
const EXPIRY_SKEW_MS: i64 = 60_000;

/// What the Spotify Settings section shows. Mirrored in `src/types/protocol.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotifyStatus {
    pub connected: bool,
    pub account: Option<String>,
    pub client_id: Option<String>,
    /// The exact Redirect URI to register in the Spotify dashboard.
    pub redirect_uri: String,
}

/// The Spotify-specific Now Playing augmentation. Mirrored in `src/types/protocol.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotifyPlayer {
    pub is_playing: bool,
    pub shuffle: bool,
    pub repeat: Option<RepeatMode>,
    /// Whether the current track is in the user's library (for the Like button); `None` unknown.
    pub saved: Option<bool>,
    /// Album art as a `data:` URI (cached per URL), when available.
    pub album_art: Option<String>,
    /// The playing track's URI (what Like saves/removes).
    pub track_uri: Option<String>,
    pub device_name: Option<String>,
}

#[derive(Default)]
struct Session {
    access_token: Option<String>,
    expires_at_ms: i64,
}

pub struct Spotify {
    store: Arc<Store>,
    /// tug's app-data dir; album art is cached under `<dir>/spotify_art`.
    cache_dir: PathBuf,
    session: Mutex<Session>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn pe(s: &str) -> String {
    auth::percent_encode(s)
}

impl Spotify {
    pub fn new(store: Arc<Store>, cache_dir: PathBuf) -> Self {
        Self {
            store,
            cache_dir,
            session: Mutex::default(),
        }
    }

    // ---- Settings / status ----------------------------------------------------------------

    fn client_id(&self) -> Option<String> {
        self.store
            .setting(CLIENT_ID_KEY)
            .ok()
            .flatten()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Whether a refresh token is stored (tug considers itself connected then).
    fn connected(&self) -> bool {
        creds::load(creds::TARGET).ok().flatten().is_some()
    }

    pub fn status(&self) -> SpotifyStatus {
        SpotifyStatus {
            connected: self.connected(),
            account: self.store.setting(ACCOUNT_KEY).ok().flatten(),
            client_id: self.client_id(),
            redirect_uri: listener::REGISTERED_REDIRECT.to_string(),
        }
    }

    /// Save the Client ID. Changing it (or clearing it) disconnects, since the stored tokens
    /// belong to the old app.
    pub fn set_client_id(&self, id: &str) -> Result<SpotifyStatus, String> {
        let id = id.trim();
        if self.client_id().as_deref() != Some(id) {
            self.disconnect()?;
        }
        if id.is_empty() {
            self.store.delete_setting(CLIENT_ID_KEY).map_err(|e| e.to_string())?;
        } else {
            self.store.set_setting(CLIENT_ID_KEY, id).map_err(|e| e.to_string())?;
        }
        Ok(self.status())
    }

    // ---- OAuth connect / disconnect -------------------------------------------------------

    /// Run the full Authorization Code + PKCE flow: bind a loopback port, open the browser, wait
    /// for the redirect, exchange the code, store the refresh token, and record the account name.
    pub fn connect(&self) -> Result<SpotifyStatus, String> {
        let client_id = self
            .client_id()
            .ok_or("Add your Spotify Client ID first, then connect.")?;

        let tcp = listener::bind()?;
        let redirect = listener::REGISTERED_REDIRECT;
        let verifier = auth::code_verifier(&http::random_bytes(32)?);
        let challenge = auth::code_challenge(&verifier);
        let state = auth::oauth_state(&http::random_bytes(16)?);
        let url = auth::authorize_url(&client_id, redirect, &challenge, &state);

        crate::commands::open_in_browser(&url)?;
        let query = listener::wait_for_callback(tcp, listener::CALLBACK_TIMEOUT)?;
        let callback = auth::parse_callback(&query).ok_or("Spotify didn't return an authorization code.")?;
        let code = auth::verify(&callback, &state)?;

        let body = format!(
            "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
            pe(&code),
            pe(redirect),
            pe(&client_id),
            pe(&verifier),
        );
        let resp = http::request(
            Method::Post,
            auth::TOKEN_URL,
            None,
            Some(("application/x-www-form-urlencoded", body)),
        )?;
        if !resp.is_success() {
            return Err(model::classify(resp.status, &resp.body, resp.retry_after).user_message());
        }
        let tokens = parse_tokens(&resp.body).ok_or("Spotify's sign-in response was unreadable.")?;
        let refresh = tokens
            .refresh_token
            .ok_or("Spotify didn't return a refresh token; please try connecting again.")?;
        creds::store(creds::TARGET, &refresh)?;
        {
            let mut s = lock(&self.session);
            s.access_token = Some(tokens.access_token);
            s.expires_at_ms = now_ms() + tokens.expires_in_ms;
        }

        // Record the account name to show in Settings (best-effort; never logged).
        if let Ok(resp) = self.api(Method::Get, "/me", None) {
            if let Some(name) = model::account_name(&resp.body) {
                let _ = self.store.set_setting(ACCOUNT_KEY, &name);
            }
        }
        Ok(self.status())
    }

    pub fn disconnect(&self) -> Result<(), String> {
        creds::delete(creds::TARGET)?;
        let _ = self.store.delete_setting(ACCOUNT_KEY);
        let mut s = lock(&self.session);
        s.access_token = None;
        s.expires_at_ms = 0;
        Ok(())
    }

    // ---- Token management -----------------------------------------------------------------

    /// A valid access token, refreshing first if the current one is missing or near expiry.
    fn access_token(&self) -> Result<String, ApiError> {
        {
            let s = lock(&self.session);
            if let Some(token) = &s.access_token {
                if !model::token_expired(s.expires_at_ms, now_ms(), EXPIRY_SKEW_MS) {
                    return Ok(token.clone());
                }
            }
        }
        self.refresh()
    }

    /// Exchange the stored refresh token for a fresh access token (and a rotated refresh token,
    /// when Spotify sends one). A failed refresh means the connection is dead.
    fn refresh(&self) -> Result<String, ApiError> {
        let client_id = self.client_id().ok_or(ApiError::Unauthorized)?;
        let refresh = creds::load(creds::TARGET)
            .map_err(|e| ApiError::Other { status: 0, message: e })?
            .ok_or(ApiError::Unauthorized)?;
        let body = format!(
            "grant_type=refresh_token&refresh_token={}&client_id={}",
            pe(&refresh),
            pe(&client_id),
        );
        let resp = http::request(
            Method::Post,
            auth::TOKEN_URL,
            None,
            Some(("application/x-www-form-urlencoded", body)),
        )
        .map_err(|e| ApiError::Other { status: 0, message: e })?;
        if !resp.is_success() {
            return Err(model::classify(resp.status, &resp.body, resp.retry_after));
        }
        let tokens = parse_tokens(&resp.body).ok_or(ApiError::Other {
            status: 0,
            message: "unreadable token response".into(),
        })?;
        if let Some(new_refresh) = tokens.refresh_token {
            let _ = creds::store(creds::TARGET, &new_refresh);
        }
        let mut s = lock(&self.session);
        s.access_token = Some(tokens.access_token.clone());
        s.expires_at_ms = now_ms() + tokens.expires_in_ms;
        Ok(tokens.access_token)
    }

    // ---- Authenticated requests -----------------------------------------------------------

    /// One authenticated Web API request to `path` (relative to `API_BASE`). On a 401 the token
    /// is refreshed once and the request retried, so an expired token recovers transparently.
    fn api(
        &self,
        method: Method,
        path: &str,
        body: Option<(&'static str, String)>,
    ) -> Result<http::Response, ApiError> {
        let url = format!("{API_BASE}{path}");
        let token = self.access_token()?;
        let send = |tok: &str| {
            http::request(method, &url, Some(tok), body.clone()).map_err(|e| ApiError::Other { status: 0, message: e })
        };
        let resp = send(&token)?;
        let resp = if resp.status == 401 {
            let token = self.refresh()?;
            send(&token)?
        } else {
            resp
        };
        if resp.is_success() {
            Ok(resp)
        } else {
            Err(model::classify(resp.status, &resp.body, resp.retry_after))
        }
    }

    // ---- Features -------------------------------------------------------------------------

    /// All of the user's own and followed playlists (paginated).
    pub fn playlists(&self) -> Result<Vec<Playlist>, String> {
        let mut out = Vec::new();
        let mut offset = 0u32;
        loop {
            let path = format!("/me/playlists?limit=50&offset={offset}");
            let resp = self.api(Method::Get, &path, None).map_err(|e| e.user_message())?;
            let (items, next) = model::parse_playlists(&resp.body);
            let got = items.len();
            out.extend(items);
            // Follow pages until Spotify stops handing out a `next`, with a hard cap for safety.
            if next.is_none() || got == 0 || out.len() >= 1000 {
                break;
            }
            offset += 50;
        }
        Ok(out)
    }

    /// Start a playlist on the iPhone. Picks the iPhone from the device list (preferring the
    /// paired phone's name); if no phone is listed, says to open Spotify on it first.
    pub fn play_playlist(&self, uri: &str, phone_name: Option<&str>) -> Result<(), String> {
        if !uri.starts_with("spotify:playlist:") {
            return Err("That doesn't look like a playlist.".into());
        }
        let devices = self
            .api(Method::Get, "/me/player/devices", None)
            .map_err(|e| e.user_message())?;
        let device = model::choose_device(&devices.body, phone_name)
            .ok_or("Open Spotify on your iPhone once, then try again.")?;
        let path = format!("/me/player/play?device_id={}", pe(&device.id));
        let body = serde_json::json!({ "context_uri": uri }).to_string();
        self.api(Method::Put, &path, Some(("application/json", body)))
            .map(|_| ())
            .map_err(|e| e.user_message())
    }

    /// The Spotify playback snapshot for the Now Playing card: repeat/shuffle state, saved state
    /// and album art for the current track. `None` when nothing is playing.
    pub fn player(&self) -> Result<Option<SpotifyPlayer>, String> {
        let resp = self
            .api(Method::Get, "/me/player", None)
            .map_err(|e| e.user_message())?;
        let snap = match model::parse_player(&resp.body) {
            Some(s) => s,
            None => return Ok(None),
        };
        let saved = match &snap.track_uri {
            Some(uri) => self.saved_state(uri),
            None => None,
        };
        let album_art = snap.art_url.as_deref().and_then(|u| self.art_data_uri(u));
        Ok(Some(SpotifyPlayer {
            is_playing: snap.is_playing,
            shuffle: snap.shuffle,
            repeat: snap.repeat,
            saved,
            album_art,
            track_uri: snap.track_uri,
            device_name: snap.device_name,
        }))
    }

    fn saved_state(&self, uri: &str) -> Option<bool> {
        let path = format!("/me/library/contains?uris={}", pe(uri));
        let resp = self.api(Method::Get, &path, None).ok()?;
        model::parse_contains(&resp.body)
    }

    pub fn set_repeat(&self, mode: RepeatMode) -> Result<(), String> {
        let path = format!("/me/player/repeat?state={}", model::repeat_to_spotify(mode));
        self.api(Method::Put, &path, None)
            .map(|_| ())
            .map_err(|e| e.user_message())
    }

    pub fn set_shuffle(&self, on: bool) -> Result<(), String> {
        let path = format!("/me/player/shuffle?state={on}");
        self.api(Method::Put, &path, None)
            .map(|_| ())
            .map_err(|e| e.user_message())
    }

    /// Like (save) or un-like (remove) a track in the user's library (Feb 2026 `/me/library`).
    pub fn set_saved(&self, uri: &str, saved: bool) -> Result<(), String> {
        let method = if saved { Method::Put } else { Method::Delete };
        // `uris` is a query parameter (comma-separated); a JSON body gets "Missing required
        // field: uris" (Dave's account, 2026-10-05).
        let path = format!("/me/library?uris={}", pe(uri));
        self.api(method, &path, None).map(|_| ()).map_err(|e| e.user_message())
    }

    /// A playlist cover as a `data:` URI (same cache as album art). Only Spotify image hosts.
    pub fn cover(&self, url: &str) -> Option<String> {
        model::is_spotify_image_url(url)
            .then(|| self.art_data_uri(url))
            .flatten()
    }

    /// Album art as a `data:` URI, cached per URL under `<cache_dir>/spotify_art`.
    fn art_data_uri(&self, url: &str) -> Option<String> {
        let dir = self.cache_dir.join("spotify_art");
        let file = dir.join(model::art_cache_name(url));
        if let Ok(bytes) = std::fs::read(&file) {
            return crate::app_icons::data_uri(&bytes);
        }
        let bytes = http::get_bytes(url).ok()?;
        let uri = crate::app_icons::data_uri(&bytes)?;
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&file, &bytes);
        Some(uri)
    }
}

struct Tokens {
    access_token: String,
    refresh_token: Option<String>,
    expires_in_ms: i64,
}

/// Parse an OAuth token response (`access_token`, optional `refresh_token`, `expires_in`).
fn parse_tokens(json: &str) -> Option<Tokens> {
    let v: Value = serde_json::from_str(json).ok()?;
    Some(Tokens {
        access_token: v.get("access_token")?.as_str()?.to_string(),
        refresh_token: v.get("refresh_token").and_then(Value::as_str).map(str::to_string),
        expires_in_ms: v.get("expires_in").and_then(Value::as_i64).unwrap_or(3600) * 1000,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_token_responses() {
        let t = parse_tokens(r#"{"access_token":"AT","refresh_token":"RT","expires_in":3600,"token_type":"Bearer"}"#)
            .unwrap();
        assert_eq!(t.access_token, "AT");
        assert_eq!(t.refresh_token.as_deref(), Some("RT"));
        assert_eq!(t.expires_in_ms, 3_600_000);
        // A refresh response often omits the refresh token (reuse the old one).
        let t = parse_tokens(r#"{"access_token":"AT2","expires_in":3600}"#).unwrap();
        assert_eq!(t.refresh_token, None);
        assert!(parse_tokens("not json").is_none());
        assert!(parse_tokens(r#"{"token_type":"Bearer"}"#).is_none());
    }

    #[test]
    fn status_reports_client_id_and_redirect() {
        let store = Arc::new(Store::in_memory().unwrap());
        let sp = Spotify::new(store.clone(), std::env::temp_dir());
        let s = sp.status();
        assert!(!s.connected);
        assert_eq!(s.client_id, None);
        assert_eq!(s.redirect_uri, "http://127.0.0.1:8972/callback");
        // Setting the client id is reflected; clearing it removes it.
        sp.set_client_id("  abc123  ").unwrap();
        assert_eq!(sp.status().client_id.as_deref(), Some("abc123"));
        sp.set_client_id("").unwrap();
        assert_eq!(sp.status().client_id, None);
    }
}
