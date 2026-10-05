//! Pure parsing and decisions for the Spotify connector: playlist/player/device/library JSON,
//! repeat-mode mapping, device choice, token-expiry and HTTP error classification. No I/O, so
//! every rule here is unit-tested without the network.

use serde::Serialize;
use serde_json::Value;

use crate::ams::RepeatMode;

/// A playlist as the UI shows it. Mirrored in `src/types/protocol.ts` (`SpotifyPlaylist`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    /// `spotify:playlist:…`, what `PUT /me/player/play` plays as `context_uri`.
    pub uri: String,
    pub name: String,
    /// Owner's display name, when the API gives one.
    pub owner: Option<String>,
    /// Number of tracks, for a subtitle; `None` when Spotify doesn't say (Feb 2026: contents are
    /// only described for playlists the user owns or collaborates on).
    pub track_count: Option<u32>,
    /// A small cover image URL (fetched through tug, never by the webview).
    pub image_url: Option<String>,
}

/// One page of `GET /me/playlists`: the playlists, and the `next` page URL when there is one.
pub fn parse_playlists(json: &str) -> (Vec<Playlist>, Option<String>) {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return (Vec::new(), None),
    };
    let next = v.get("next").and_then(Value::as_str).map(str::to_string);
    let items = v
        .get("items")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(parse_playlist).collect())
        .unwrap_or_default();
    (items, next)
}

fn parse_playlist(v: &Value) -> Option<Playlist> {
    // A `null` can appear in the items array (a playlist that became unavailable); skip it.
    let uri = v.get("uri")?.as_str()?.to_string();
    if !uri.starts_with("spotify:playlist:") {
        return None;
    }
    let name = v.get("name").and_then(Value::as_str).unwrap_or("Untitled").to_string();
    Some(Playlist {
        uri,
        name,
        owner: v
            .get("owner")
            .and_then(|o| o.get("display_name"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        // Feb 2026 renamed the playlist's `tracks` to `items`; accept both.
        track_count: v
            .get("items")
            .or_else(|| v.get("tracks"))
            .and_then(|t| t.get("total"))
            .and_then(Value::as_u64)
            .map(|n| n as u32),
        image_url: v.get("images").and_then(|i| image_url_near(i, COVER_PX)),
    })
}

/// The best cover image URL from a Spotify `images` array: the one whose width is closest to
/// `TARGET_PX`, else the first. Spotify lists images largest-first with widths that may be null.
const TARGET_PX: i64 = 300;
/// Playlist rows show a small cover; pick the image nearest this width.
const COVER_PX: i64 = 64;

pub fn best_image_url(images: &Value) -> Option<String> {
    image_url_near(images, TARGET_PX)
}

/// Whether `url` is a Spotify image CDN address tug will fetch for the UI.
pub fn is_spotify_image_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or("");
    host.ends_with(".scdn.co") || host.ends_with(".spotifycdn.com")
}

fn image_url_near(images: &Value, target_px: i64) -> Option<String> {
    let arr = images.as_array()?;
    let mut best: Option<(i64, &str)> = None;
    for img in arr {
        let url = match img.get("url").and_then(Value::as_str) {
            Some(u) if u.starts_with("https://") => u,
            _ => continue,
        };
        // No width (common for playlist covers) sorts as "far" so a sized image wins if present.
        let width = img.get("width").and_then(Value::as_i64).unwrap_or(0);
        let dist = (width - target_px).abs();
        if best.is_none_or(|(d, _)| dist < d) {
            best = Some((dist, url));
        }
    }
    best.map(|(_, u)| u.to_string())
}

/// A device playback can be sent to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceChoice {
    pub id: String,
    pub name: String,
}

/// Pick the iPhone to play on from `GET /me/player/devices`: a `Smartphone`, preferring the one
/// whose name matches the paired phone and/or is already active. `None` when no phone is listed
/// (Spotify hasn't been opened on it recently) — the caller then tells the user to open it once.
pub fn choose_device(json: &str, phone_name: Option<&str>) -> Option<DeviceChoice> {
    let v: Value = serde_json::from_str(json).ok()?;
    let devices = v.get("devices")?.as_array()?;
    let phone = phone_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase);
    let mut best: Option<(i32, DeviceChoice)> = None;
    for d in devices {
        let is_phone = d
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| t.eq_ignore_ascii_case("smartphone"));
        if !is_phone {
            continue;
        }
        let id = match d.get("id").and_then(Value::as_str) {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => continue,
        };
        let name = d.get("name").and_then(Value::as_str).unwrap_or("iPhone").to_string();
        let name_matches = phone.as_deref().is_some_and(|p| {
            name.to_lowercase() == p || name.to_lowercase().contains(p) || p.contains(&name.to_lowercase())
        });
        let active = d.get("is_active").and_then(Value::as_bool).unwrap_or(false);
        let score = i32::from(name_matches) * 2 + i32::from(active);
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, DeviceChoice { id, name }));
        }
    }
    best.map(|(_, c)| c)
}

/// What `GET /me/player` reports, before album art is turned into a data URI.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerSnapshot {
    pub is_playing: bool,
    pub shuffle: bool,
    pub repeat: Option<RepeatMode>,
    pub track_uri: Option<String>,
    pub art_url: Option<String>,
    pub device_name: Option<String>,
}

/// Parse `GET /me/player`. Returns `None` for an empty body (HTTP 204: nothing is playing).
pub fn parse_player(json: &str) -> Option<PlayerSnapshot> {
    let v: Value = serde_json::from_str(json.trim()).ok()?;
    if !v.is_object() {
        return None;
    }
    let item = v.get("item");
    Some(PlayerSnapshot {
        is_playing: v.get("is_playing").and_then(Value::as_bool).unwrap_or(false),
        shuffle: v.get("shuffle_state").and_then(Value::as_bool).unwrap_or(false),
        repeat: v
            .get("repeat_state")
            .and_then(Value::as_str)
            .and_then(repeat_from_spotify),
        track_uri: item
            .and_then(|i| i.get("uri"))
            .and_then(Value::as_str)
            .filter(|u| !u.is_empty())
            .map(str::to_string),
        art_url: item
            .and_then(|i| i.get("album"))
            .and_then(|a| a.get("images"))
            .and_then(best_image_url),
        device_name: v
            .get("device")
            .and_then(|d| d.get("name"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    })
}

/// Parse `GET /me/library/contains` (a JSON array of booleans): whether the first item is saved.
pub fn parse_contains(json: &str) -> Option<bool> {
    serde_json::from_str::<Value>(json).ok()?.as_array()?.first()?.as_bool()
}

/// The account's display name from `GET /me` (falls back to the account id).
pub fn account_name(json: &str) -> Option<String> {
    let v: Value = serde_json::from_str(json).ok()?;
    v.get("display_name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| v.get("id").and_then(Value::as_str))
        .map(str::to_string)
}

/// tug's repeat mode → Spotify's `state` for `PUT /me/player/repeat`.
pub fn repeat_to_spotify(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::Off => "off",
        RepeatMode::One => "track",
        RepeatMode::All => "context",
    }
}

/// Spotify's `repeat_state` → tug's repeat mode.
pub fn repeat_from_spotify(state: &str) -> Option<RepeatMode> {
    match state {
        "off" => Some(RepeatMode::Off),
        "track" => Some(RepeatMode::One),
        "context" => Some(RepeatMode::All),
        _ => None,
    }
}

/// A safe on-disk cache file name for an album-art URL: the last path segment (Spotify's stable
/// image id) reduced to `[A-Za-z0-9]`, capped, with an `.img` suffix. Pure, so it's tested.
pub fn art_cache_name(url: &str) -> String {
    let tail = url.rsplit('/').next().unwrap_or(url);
    let id: String = tail.chars().filter(|c| c.is_ascii_alphanumeric()).take(80).collect();
    let id = if id.is_empty() { "art".to_string() } else { id };
    format!("{id}.img")
}

/// The access token is expired (or close enough that it shouldn't be used), with a safety skew
/// so a token that expires mid-request is refreshed first.
pub fn token_expired(expires_at_ms: i64, now_ms: i64, skew_ms: i64) -> bool {
    now_ms + skew_ms >= expires_at_ms
}

/// A Spotify Web API error, classified from the HTTP status and body so the connector can react
/// (refresh on 401, wait on 429) and show the user something plain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// 401: the access token is stale; refresh once and retry.
    Unauthorized,
    /// 429: too many requests; wait `retry_after` seconds (from the header).
    RateLimited { retry_after: u64 },
    /// 403 with a premium_required reason: the account isn't Premium.
    PremiumRequired,
    /// 404 with no active device: Spotify isn't open on the iPhone.
    NoActiveDevice,
    /// Any other error status, with a cleaned-up message.
    Other { status: u16, message: String },
}

impl ApiError {
    /// A plain-English message for the UI (never contains tokens or the account name).
    pub fn user_message(&self) -> String {
        match self {
            Self::Unauthorized => "Spotify sign-in expired. Reconnect under Settings › Connectors.".into(),
            Self::RateLimited { retry_after } => {
                format!("Spotify is rate-limiting; try again in {retry_after}s.")
            }
            Self::PremiumRequired => "This needs Spotify Premium on the connected account.".into(),
            Self::NoActiveDevice => "Open Spotify on your iPhone once, then try again.".into(),
            Self::Other { status, message } if message.is_empty() => format!("Spotify error (HTTP {status})."),
            Self::Other { message, .. } => format!("Spotify: {message}"),
        }
    }
}

/// Classify a non-success response. `retry_after` is the parsed `Retry-After` header, if any.
pub fn classify(status: u16, body: &str, retry_after: Option<u64>) -> ApiError {
    let lower = body.to_lowercase();
    match status {
        401 => ApiError::Unauthorized,
        429 => ApiError::RateLimited {
            retry_after: retry_after.unwrap_or(1),
        },
        403 if lower.contains("premium") => ApiError::PremiumRequired,
        404 if lower.contains("no active device") || lower.contains("no_active_device") => ApiError::NoActiveDevice,
        _ => ApiError::Other {
            status,
            message: error_message(body),
        },
    }
}

/// Pull the human part out of a Spotify error body (`{"error":{"message":"…"}}`, or the OAuth
/// `{"error_description":"…"}`), falling back to empty.
fn error_message(body: &str) -> String {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    v.get("error")
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .or_else(|| v.get("error_description").and_then(Value::as_str))
        // The OAuth token endpoint returns a plain-string `error` too.
        .or_else(|| v.get("error").and_then(Value::as_str))
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_page_of_playlists_and_the_next_link() {
        let json = r#"{
            "next": "https://api.spotify.com/v1/me/playlists?offset=50&limit=50",
            "items": [
                {"uri":"spotify:playlist:1","name":"Focus","owner":{"display_name":"Dave"},
                 "tracks":{"total":42},"images":[{"url":"https://i.scdn.co/a","width":640},{"url":"https://i.scdn.co/b","width":300}]},
                null,
                {"uri":"spotify:playlist:2","name":"Runs","owner":{"display_name":""},"items":{"total":10},"images":[]},
                {"uri":"spotify:playlist:3","name":"Followed","owner":{"display_name":"Someone"},"images":[{"url":"https://mosaic.scdn.co/60/x","width":60}]},
                {"uri":"spotify:album:nope","name":"Not a playlist","tracks":{"total":1}}
            ]
        }"#;
        let (items, next) = parse_playlists(json);
        assert_eq!(items.len(), 3, "null and non-playlist uris are skipped");
        assert_eq!(items[0].name, "Focus");
        assert_eq!(items[0].owner.as_deref(), Some("Dave"));
        assert_eq!(items[0].track_count, Some(42), "pre-2026 `tracks`");
        assert_eq!(
            items[0].image_url.as_deref(),
            Some("https://i.scdn.co/b"),
            "smallest near 64 px"
        );
        assert_eq!(items[1].owner, None, "empty display name is None");
        assert_eq!(items[1].track_count, Some(10), "Feb 2026 `items`");
        assert_eq!(items[1].image_url, None);
        assert_eq!(items[2].track_count, None, "no count given: unknown, not 0");
        assert_eq!(items[2].image_url.as_deref(), Some("https://mosaic.scdn.co/60/x"));
        assert_eq!(
            next.as_deref(),
            Some("https://api.spotify.com/v1/me/playlists?offset=50&limit=50")
        );
        // Last page: next is null.
        let (_, none) = parse_playlists(r#"{"next":null,"items":[]}"#);
        assert_eq!(none, None);
    }

    #[test]
    fn only_spotify_image_hosts_are_fetched() {
        assert!(is_spotify_image_url("https://i.scdn.co/image/ab67"));
        assert!(is_spotify_image_url("https://mosaic.scdn.co/640/x"));
        assert!(is_spotify_image_url("https://image-cdn-ak.spotifycdn.com/image/x"));
        assert!(!is_spotify_image_url("http://i.scdn.co/image/ab67"));
        assert!(!is_spotify_image_url("https://evil.example/i.scdn.co"));
        assert!(!is_spotify_image_url("https://scdn.co.evil.example/x"));
    }

    #[test]
    fn chooses_the_iphone_preferring_name_then_active() {
        let json = r#"{"devices":[
            {"id":"pc","name":"Dave's PC","type":"Computer","is_active":true},
            {"id":"ph2","name":"Spare iPhone","type":"Smartphone","is_active":true},
            {"id":"ph1","name":"Dave's iPhone","type":"Smartphone","is_active":false}
        ]}"#;
        // Name match beats the active-but-wrong phone.
        assert_eq!(
            choose_device(json, Some("Dave's iPhone")),
            Some(DeviceChoice {
                id: "ph1".into(),
                name: "Dave's iPhone".into()
            })
        );
        // With no phone name, the active smartphone wins.
        assert_eq!(choose_device(json, None).unwrap().id, "ph2");
        // No smartphone at all: nothing to play on.
        assert_eq!(
            choose_device(r#"{"devices":[{"id":"pc","name":"PC","type":"Computer"}]}"#, Some("x")),
            None
        );
        assert_eq!(choose_device(r#"{"devices":[]}"#, None), None);
    }

    #[test]
    fn parses_the_player_snapshot() {
        let json = r#"{
            "is_playing": true,
            "shuffle_state": true,
            "repeat_state": "context",
            "device": {"name":"Dave's iPhone","type":"Smartphone"},
            "item": {"uri":"spotify:track:abc","album":{"images":[{"url":"https://i.scdn.co/big","width":640},{"url":"https://i.scdn.co/mid","width":300}]}}
        }"#;
        let p = parse_player(json).unwrap();
        assert!(p.is_playing && p.shuffle);
        assert_eq!(p.repeat, Some(RepeatMode::All));
        assert_eq!(p.track_uri.as_deref(), Some("spotify:track:abc"));
        assert_eq!(p.art_url.as_deref(), Some("https://i.scdn.co/mid"));
        assert_eq!(p.device_name.as_deref(), Some("Dave's iPhone"));
        // 204/empty body: nothing playing.
        assert_eq!(parse_player(""), None);
        assert_eq!(parse_player("   "), None);
    }

    #[test]
    fn repeat_mode_maps_both_ways() {
        for mode in [RepeatMode::Off, RepeatMode::One, RepeatMode::All] {
            assert_eq!(repeat_from_spotify(repeat_to_spotify(mode)), Some(mode));
        }
        assert_eq!(repeat_to_spotify(RepeatMode::One), "track");
        assert_eq!(repeat_to_spotify(RepeatMode::All), "context");
        assert_eq!(repeat_from_spotify("weird"), None);
    }

    #[test]
    fn contains_and_account_parse() {
        assert_eq!(parse_contains("[true]"), Some(true));
        assert_eq!(parse_contains("[false, true]"), Some(false));
        assert_eq!(parse_contains("[]"), None);
        assert_eq!(
            account_name(r#"{"display_name":"Dave James","id":"dj"}"#).as_deref(),
            Some("Dave James")
        );
        assert_eq!(account_name(r#"{"display_name":"","id":"dj"}"#).as_deref(), Some("dj"));
    }

    #[test]
    fn art_cache_names_are_safe_and_stable() {
        assert_eq!(
            art_cache_name("https://i.scdn.co/image/ab67616d00001e02"),
            "ab67616d00001e02.img"
        );
        assert_eq!(art_cache_name("https://i.scdn.co/image/ab?x=1/../y"), "y.img");
        // Same id → same file (so the cache actually hits).
        assert_eq!(
            art_cache_name("https://i.scdn.co/image/ZZ"),
            art_cache_name("https://cdn.example/other/ZZ")
        );
        assert_eq!(art_cache_name(""), "art.img");
    }

    #[test]
    fn token_expiry_respects_the_skew() {
        // Expires at 1000; with a 100 ms skew it's "expired" from 900 onward.
        assert!(!token_expired(1000, 899, 100));
        assert!(token_expired(1000, 900, 100));
        assert!(token_expired(1000, 5000, 0));
    }

    #[test]
    fn classifies_errors_for_the_right_reaction() {
        assert_eq!(classify(401, "{}", None), ApiError::Unauthorized);
        assert_eq!(classify(429, "", Some(7)), ApiError::RateLimited { retry_after: 7 });
        assert_eq!(classify(429, "", None), ApiError::RateLimited { retry_after: 1 });
        assert_eq!(
            classify(
                403,
                r#"{"error":{"status":403,"reason":"PREMIUM_REQUIRED","message":"Player command failed: Premium required"}}"#,
                None
            ),
            ApiError::PremiumRequired
        );
        assert_eq!(
            classify(
                404,
                r#"{"error":{"status":404,"reason":"NO_ACTIVE_DEVICE","message":"Device not found"}}"#,
                None
            ),
            ApiError::NoActiveDevice
        );
        let other = classify(500, r#"{"error":{"message":"Server error"}}"#, None);
        assert_eq!(
            other,
            ApiError::Other {
                status: 500,
                message: "Server error".into()
            }
        );
        assert!(other.user_message().contains("Server error"));
        // OAuth-style body.
        assert_eq!(
            error_message(r#"{"error":"invalid_grant","error_description":"code expired"}"#),
            "code expired"
        );
    }
}
