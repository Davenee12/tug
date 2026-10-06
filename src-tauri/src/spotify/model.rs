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
    /// The bare playlist id, for `GET /playlists/{id}/items` and `POST /playlists/{id}/items`.
    pub id: String,
    pub name: String,
    /// Owner's display name, when the API gives one.
    pub owner: Option<String>,
    /// Number of tracks, for a subtitle; `None` when Spotify doesn't say (Feb 2026: contents are
    /// only described for playlists the user owns or collaborates on).
    pub track_count: Option<u32>,
    /// A small cover image URL (fetched through tug, never by the webview).
    pub image_url: Option<String>,
    /// The user owns or collaborates on this playlist: tug may open its songs and add to it.
    pub owned: bool,
}

/// One page of `GET /me/playlists`: the playlists, and the `next` page URL when there is one.
/// Each is marked `owned` against the current user's id (`None` leaves them all unowned).
pub fn parse_playlists_owned(json: &str, me_id: Option<&str>) -> (Vec<Playlist>, Option<String>) {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return (Vec::new(), None),
    };
    let next = v.get("next").and_then(Value::as_str).map(str::to_string);
    let items = v
        .get("items")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(|p| parse_playlist_for(p, me_id)).collect())
        .unwrap_or_default();
    (items, next)
}

/// Parse a playlist, marking it `owned` when its owner id is `me` or it is collaborative (Feb 2026
/// only lets tug read items and add to playlists the user owns or collaborates on).
fn parse_playlist_for(v: &Value, me_id: Option<&str>) -> Option<Playlist> {
    // A `null` can appear in the items array (a playlist that became unavailable); skip it.
    let uri = v.get("uri")?.as_str()?.to_string();
    if !uri.starts_with("spotify:playlist:") {
        return None;
    }
    let name = v.get("name").and_then(Value::as_str).unwrap_or("Untitled").to_string();
    let owner_id = v.get("owner").and_then(|o| o.get("id")).and_then(Value::as_str);
    let collaborative = v.get("collaborative").and_then(Value::as_bool).unwrap_or(false);
    let owned = collaborative || matches!((me_id, owner_id), (Some(m), Some(o)) if m == o);
    let id = id_from_uri(v.get("id").and_then(Value::as_str), "playlist", &uri);
    Some(Playlist {
        uri,
        id,
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
        owned,
    })
}

/// The bare id, from an explicit `id` field when present, else the last `:` segment of the URI.
fn id_from_uri(explicit: Option<&str>, _kind: &str, uri: &str) -> String {
    match explicit {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => uri.rsplit(':').next().unwrap_or(uri).to_string(),
    }
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

/// The account's bare id from `GET /me` (for marking which playlists the user owns).
pub fn account_id(json: &str) -> Option<String> {
    serde_json::from_str::<Value>(json)
        .ok()?
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
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

/// Returned when Spotify can't see the iPhone (its Spotify app isn't open). The UI matches on
/// this exact text to wait for the phone instead of showing an error; keep it in sync with
/// `SPOTIFY_NO_PHONE` in `src/stores/tug.ts`.
pub const NO_PHONE: &str = "Open Spotify on your iPhone.";

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
    /// "about 19 hours" / "about 5 minutes" / "a moment" for a Retry-After in seconds.
    pub fn wait_phrase(secs: u64) -> String {
        match secs {
            0..=59 => "a moment".into(),
            60..=5399 => {
                let m = secs.div_ceil(60);
                if m == 1 {
                    "about a minute".into()
                } else {
                    format!("about {m} minutes")
                }
            }
            _ => {
                let h = (secs + 1800) / 3600;
                if h == 1 {
                    "about an hour".into()
                } else {
                    format!("about {h} hours")
                }
            }
        }
    }

    /// A plain-English message for the UI (never contains tokens or the account name).
    pub fn user_message(&self) -> String {
        match self {
            Self::Unauthorized => "Spotify sign-in expired. Reconnect under Settings › Connectors.".into(),
            Self::RateLimited { retry_after } => format!(
                "Spotify asked tug to slow down. Search and playlists are back in {}; play and pause still work.",
                Self::wait_phrase(*retry_after)
            ),
            Self::PremiumRequired => "This needs Spotify Premium on the connected account.".into(),
            Self::NoActiveDevice => NO_PHONE.into(),
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

// ---- Search, library browsing and playback detail (Feb 2026 Development Mode) -------------
//
// All of these parse the Web API's own JSON into the flat shapes the panel renders. Spotify's
// `GET /search` caps `limit` at 10 (default 5) and pages with `offset`; the per-type `total` tells
// us whether a "Show more" page exists. Batch lookups, artist top-tracks and browse/new-releases
// were removed, and recommendations/audio-features are not available to a Development Mode app, so
// nothing here depends on them.

/// A track as the panel lists it (search, queue, recently played, top, album and playlist songs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    /// `spotify:track:…`, played directly or queued.
    pub uri: String,
    pub name: String,
    /// All artists, joined "A, B".
    pub artists: String,
    /// The primary artist's `spotify:artist:…` (for "open artist" from a track), when known.
    pub artist_uri: Option<String>,
    pub album: String,
    /// The album's `spotify:album:…`, so a track can play in its album context.
    pub album_uri: Option<String>,
    pub image_url: Option<String>,
    pub duration_ms: u32,
}

/// An album as search results and artist pages list it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub uri: String,
    pub id: String,
    pub name: String,
    pub artists: String,
    pub image_url: Option<String>,
    pub total_tracks: Option<u32>,
    /// Release year (first four characters of `release_date`), when given.
    pub year: Option<String>,
}

/// An artist as search results list it and the artist page heads with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artist {
    pub uri: String,
    pub id: String,
    pub name: String,
    pub image_url: Option<String>,
}

/// Whether another `offset` page exists per type, so the UI shows "Show more".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMore {
    pub tracks: bool,
    pub albums: bool,
    pub artists: bool,
    pub playlists: bool,
}

/// A page of `GET /search` across the requested types.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotifySearch {
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
    pub more: SearchMore,
}

/// `GET /me/player/queue`: what's playing now and what's next.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Queue {
    pub currently_playing: Option<Track>,
    pub queue: Vec<Track>,
}

/// A device playback can be sent to, for the "Play on" picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    /// Spotify's device type: "Smartphone", "Computer", "Speaker"…
    pub kind: String,
    pub is_active: bool,
}

/// An album page: the album, plus its songs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumDetail {
    pub album: Album,
    pub tracks: Vec<Track>,
}

/// An artist page: the artist, plus their albums (artist top-tracks was removed in Feb 2026).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistDetail {
    pub artist: Artist,
    pub albums: Vec<Album>,
}

/// Join an `artists` array into "A, B".
fn joined_artists(v: &Value) -> String {
    v.get("artists")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("name").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

/// The primary artist's URI, for opening an artist from a track or album.
fn primary_artist_uri(v: &Value) -> Option<String> {
    v.get("artists")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|a| a.get("uri"))
        .and_then(Value::as_str)
        .filter(|u| u.starts_with("spotify:artist:"))
        .map(str::to_string)
}

/// Parse a full track object (search / queue / recently-played / top / playlist items). Album art
/// and name come from the track's own `album`; `album_fallback` fills them for album-page songs,
/// whose simplified track objects omit the album.
fn parse_track(v: &Value, album_fallback: Option<&Album>) -> Option<Track> {
    let uri = v.get("uri").and_then(Value::as_str)?;
    if !uri.starts_with("spotify:track:") {
        return None;
    }
    let album = v.get("album");
    let (album_name, album_uri, image_url) = match album {
        Some(a) => (
            a.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            a.get("uri").and_then(Value::as_str).map(str::to_string),
            a.get("images").and_then(best_image_url),
        ),
        None => match album_fallback {
            Some(a) => (a.name.clone(), Some(a.uri.clone()), a.image_url.clone()),
            None => (String::new(), None, None),
        },
    };
    Some(Track {
        uri: uri.to_string(),
        name: v.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
        artists: joined_artists(v),
        artist_uri: primary_artist_uri(v),
        album: album_name,
        album_uri,
        image_url,
        duration_ms: v.get("duration_ms").and_then(Value::as_u64).unwrap_or(0) as u32,
    })
}

fn parse_album(v: &Value) -> Option<Album> {
    let uri = v.get("uri").and_then(Value::as_str)?;
    if !uri.starts_with("spotify:album:") {
        return None;
    }
    Some(Album {
        uri: uri.to_string(),
        id: id_from_uri(v.get("id").and_then(Value::as_str), "album", uri),
        name: v.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
        artists: joined_artists(v),
        image_url: v.get("images").and_then(best_image_url),
        total_tracks: v.get("total_tracks").and_then(Value::as_u64).map(|n| n as u32),
        year: v
            .get("release_date")
            .and_then(Value::as_str)
            .filter(|s| s.len() >= 4)
            .map(|s| s[..4].to_string()),
    })
}

fn parse_artist(v: &Value) -> Option<Artist> {
    let uri = v.get("uri").and_then(Value::as_str)?;
    if !uri.starts_with("spotify:artist:") {
        return None;
    }
    Some(Artist {
        uri: uri.to_string(),
        id: id_from_uri(v.get("id").and_then(Value::as_str), "artist", uri),
        name: v.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
        image_url: v.get("images").and_then(best_image_url),
    })
}

/// Whether a `{items,total,limit,offset}` page has another page after it.
fn has_more(section: &Value) -> bool {
    let total = section.get("total").and_then(Value::as_u64).unwrap_or(0);
    let limit = section.get("limit").and_then(Value::as_u64).unwrap_or(0);
    let offset = section.get("offset").and_then(Value::as_u64).unwrap_or(0);
    offset + limit < total
}

fn items_of(section: Option<&Value>) -> &[Value] {
    section
        .and_then(|s| s.get("items"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// Parse `GET /search`. Each requested type's section is optional (we only ask for the ones the
/// caller wants). `me_id` marks owned playlists; `None` leaves them all unowned.
pub fn parse_search(json: &str, me_id: Option<&str>) -> SpotifySearch {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return SpotifySearch::default(),
    };
    let tracks_s = v.get("tracks");
    let albums_s = v.get("albums");
    let artists_s = v.get("artists");
    let playlists_s = v.get("playlists");
    SpotifySearch {
        tracks: items_of(tracks_s).iter().filter_map(|t| parse_track(t, None)).collect(),
        albums: items_of(albums_s).iter().filter_map(parse_album).collect(),
        artists: items_of(artists_s).iter().filter_map(parse_artist).collect(),
        playlists: items_of(playlists_s)
            .iter()
            .filter_map(|p| parse_playlist_for(p, me_id))
            .collect(),
        more: SearchMore {
            tracks: tracks_s.is_some_and(has_more),
            albums: albums_s.is_some_and(has_more),
            artists: artists_s.is_some_and(has_more),
            playlists: playlists_s.is_some_and(has_more),
        },
    }
}

/// Parse `GET /me/player/queue`.
pub fn parse_queue(json: &str) -> Queue {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Queue::default(),
    };
    Queue {
        currently_playing: v.get("currently_playing").and_then(|t| parse_track(t, None)),
        queue: v
            .get("queue")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(|t| parse_track(t, None)).collect())
            .unwrap_or_default(),
    }
}

/// Parse `GET /me/player/recently-played` (each item wraps a `track`), newest first and de-duped
/// so the same song played twice in a row shows once.
pub fn parse_recently_played(json: &str) -> Vec<Track> {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<Track> = Vec::new();
    for item in items_of(Some(&v)) {
        if let Some(t) = item.get("track").and_then(|t| parse_track(t, None)) {
            if out.last().map(|p| &p.uri) != Some(&t.uri) {
                out.push(t);
            }
        }
    }
    out
}

/// Parse `GET /playlists/{id}/items` (each item wraps a `track`); order kept, repeats kept.
pub fn parse_playlist_items(json: &str) -> Vec<Track> {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    items_of(Some(&v))
        .iter()
        .filter_map(|item| item.get("track").and_then(|t| parse_track(t, None)))
        .collect()
}

/// Parse `GET /me/top/tracks`.
pub fn parse_top_tracks(json: &str) -> Vec<Track> {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    items_of(Some(&v)).iter().filter_map(|t| parse_track(t, None)).collect()
}

/// Parse `GET /me/top/artists`.
pub fn parse_top_artists(json: &str) -> Vec<Artist> {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    items_of(Some(&v)).iter().filter_map(parse_artist).collect()
}

/// Parse `GET /me/player/devices` into the "Play on" list.
pub fn parse_devices(json: &str) -> Vec<Device> {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    v.get("devices")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|d| {
                    let id = d.get("id").and_then(Value::as_str).filter(|s| !s.is_empty())?;
                    Some(Device {
                        id: id.to_string(),
                        name: d.get("name").and_then(Value::as_str).unwrap_or("Device").to_string(),
                        kind: d.get("type").and_then(Value::as_str).unwrap_or("").to_string(),
                        is_active: d.get("is_active").and_then(Value::as_bool).unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse `GET /albums/{id}` (its own `tracks.items` are simplified: they carry no album, so the
/// album's name/art is folded into each song).
pub fn parse_album_detail(json: &str) -> Option<AlbumDetail> {
    let v: Value = serde_json::from_str(json).ok()?;
    let album = parse_album(&v)?;
    let tracks = items_of(v.get("tracks"))
        .iter()
        .filter_map(|t| parse_track(t, Some(&album)))
        .collect();
    Some(AlbumDetail { album, tracks })
}

/// Parse `GET /artists/{id}`.
pub fn parse_artist_detail_head(json: &str) -> Option<Artist> {
    parse_artist(&serde_json::from_str::<Value>(json).ok()?)
}

/// Parse `GET /artists/{id}/albums`, de-duped by name (Spotify lists album + single + compilation
/// variants that would otherwise repeat a title).
pub fn parse_artist_albums(json: &str) -> Vec<Album> {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut seen = std::collections::HashSet::new();
    items_of(Some(&v))
        .iter()
        .filter_map(parse_album)
        .filter(|a| seen.insert(a.name.to_lowercase()))
        .collect()
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
        let (items, next) = parse_playlists_owned(json, None);
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
        let (_, none) = parse_playlists_owned(r#"{"next":null,"items":[]}"#, None);
        assert_eq!(none, None);
        // Id falls out of the uri, and without a `me` id nothing is owned.
        assert_eq!(items[0].id, "1");
        assert!(!items[0].owned);
    }

    #[test]
    fn marks_owned_and_collaborative_playlists() {
        let json = r#"{"items":[
            {"uri":"spotify:playlist:a","id":"a","name":"Mine","owner":{"id":"dj","display_name":"Dave"}},
            {"uri":"spotify:playlist:b","id":"b","name":"Theirs","owner":{"id":"someone","display_name":"X"}},
            {"uri":"spotify:playlist:c","id":"c","name":"Shared","collaborative":true,"owner":{"id":"someone"}}
        ]}"#;
        let (items, _) = parse_playlists_owned(json, Some("dj"));
        assert!(items[0].owned, "owner id matches me");
        assert!(!items[1].owned, "someone else's playlist");
        assert!(items[2].owned, "collaborative counts as owned");
    }

    #[test]
    fn parses_a_search_page_across_types_with_more_flags() {
        let json = r#"{
          "tracks":{"total":42,"limit":10,"offset":0,"items":[
            {"uri":"spotify:track:t1","name":"Teardrop","duration_ms":330000,
             "artists":[{"name":"Massive Attack","uri":"spotify:artist:ma"}],
             "album":{"uri":"spotify:album:mz","name":"Mezzanine","images":[{"url":"https://i.scdn.co/mz","width":300}]}},
            {"name":"not a track"}
          ]},
          "albums":{"total":5,"limit":10,"offset":0,"items":[
            {"uri":"spotify:album:mz","id":"mz","name":"Mezzanine","total_tracks":11,"release_date":"1998-04-20",
             "artists":[{"name":"Massive Attack"}],"images":[{"url":"https://i.scdn.co/mz","width":300}]}
          ]},
          "artists":{"total":1,"limit":10,"offset":0,"items":[
            {"uri":"spotify:artist:ma","id":"ma","name":"Massive Attack","images":[{"url":"https://i.scdn.co/ma","width":300}]}
          ]},
          "playlists":{"total":30,"limit":10,"offset":0,"items":[
            {"uri":"spotify:playlist:p","id":"p","name":"Trip Hop","owner":{"id":"dj","display_name":"Dave"}}
          ]}
        }"#;
        let r = parse_search(json, Some("dj"));
        assert_eq!(r.tracks.len(), 1, "non-track item skipped");
        let t = &r.tracks[0];
        assert_eq!(t.name, "Teardrop");
        assert_eq!(t.artists, "Massive Attack");
        assert_eq!(t.artist_uri.as_deref(), Some("spotify:artist:ma"));
        assert_eq!(t.album, "Mezzanine");
        assert_eq!(t.album_uri.as_deref(), Some("spotify:album:mz"));
        assert_eq!(t.duration_ms, 330000);
        assert_eq!(r.albums[0].year.as_deref(), Some("1998"));
        assert_eq!(r.albums[0].total_tracks, Some(11));
        assert_eq!(r.artists[0].id, "ma");
        assert!(r.playlists[0].owned);
        // tracks: 0+10 < 42 → more; playlists 0+10 < 30 → more; albums/artists exhausted.
        assert_eq!(
            r.more,
            SearchMore {
                tracks: true,
                albums: false,
                artists: false,
                playlists: true
            }
        );
        // A type we didn't ask for is simply absent, never "more".
        let only_tracks = parse_search(r#"{"tracks":{"total":3,"limit":10,"offset":0,"items":[]}}"#, None);
        assert!(only_tracks.albums.is_empty());
        assert!(!only_tracks.more.albums && !only_tracks.more.tracks);
        assert_eq!(parse_search("nonsense", None), SpotifySearch::default());
    }

    #[test]
    fn parses_queue_now_and_next() {
        let json = r#"{
          "currently_playing":{"uri":"spotify:track:now","name":"Now","duration_ms":1000,
            "artists":[{"name":"A"}],"album":{"name":"Al","uri":"spotify:album:x"}},
          "queue":[
            {"uri":"spotify:track:n1","name":"Next 1","duration_ms":2000,"artists":[{"name":"B"}],"album":{"name":"Al2"}},
            {"uri":"spotify:track:n2","name":"Next 2","duration_ms":3000,"artists":[],"album":{"name":""}}
          ]
        }"#;
        let q = parse_queue(json);
        assert_eq!(q.currently_playing.as_ref().unwrap().name, "Now");
        assert_eq!(q.queue.len(), 2);
        assert_eq!(q.queue[0].artists, "B");
        // Empty / nothing playing.
        assert_eq!(
            parse_queue(r#"{"currently_playing":null,"queue":[]}"#),
            Queue::default()
        );
    }

    #[test]
    fn recently_played_dedupes_consecutive_repeats() {
        let json = r#"{"items":[
          {"track":{"uri":"spotify:track:a","name":"A","duration_ms":1,"artists":[{"name":"x"}],"album":{"name":"al"}}},
          {"track":{"uri":"spotify:track:a","name":"A","duration_ms":1,"artists":[{"name":"x"}],"album":{"name":"al"}}},
          {"track":{"uri":"spotify:track:b","name":"B","duration_ms":1,"artists":[{"name":"x"}],"album":{"name":"al"}}}
        ]}"#;
        let r = parse_recently_played(json);
        assert_eq!(
            r.iter().map(|t| t.uri.as_str()).collect::<Vec<_>>(),
            ["spotify:track:a", "spotify:track:b"]
        );
    }

    #[test]
    fn parses_top_tracks_and_artists() {
        let tracks = parse_top_tracks(
            r#"{"items":[{"uri":"spotify:track:a","name":"A","duration_ms":1,"artists":[{"name":"x"}],"album":{"name":"al"}}]}"#,
        );
        assert_eq!(tracks.len(), 1);
        let artists = parse_top_artists(r#"{"items":[{"uri":"spotify:artist:a","id":"a","name":"X"}]}"#);
        assert_eq!(artists[0].name, "X");
    }

    #[test]
    fn parses_devices_for_the_picker() {
        let json = r#"{"devices":[
          {"id":"ph","name":"Dave's iPhone","type":"Smartphone","is_active":true},
          {"id":"pc","name":"Dave's PC","type":"Computer","is_active":false},
          {"id":"","name":"No id","type":"Speaker"}
        ]}"#;
        let d = parse_devices(json);
        assert_eq!(d.len(), 2, "a device without an id can't be targeted");
        assert_eq!(d[0].kind, "Smartphone");
        assert!(d[0].is_active);
    }

    #[test]
    fn parses_an_album_page_folding_the_album_into_its_songs() {
        let json = r#"{
          "uri":"spotify:album:mz","id":"mz","name":"Mezzanine","total_tracks":2,"release_date":"1998",
          "artists":[{"name":"Massive Attack"}],"images":[{"url":"https://i.scdn.co/mz","width":300}],
          "tracks":{"items":[
            {"uri":"spotify:track:t1","name":"Angel","duration_ms":379000,"artists":[{"name":"Massive Attack","uri":"spotify:artist:ma"}]},
            {"uri":"spotify:track:t2","name":"Teardrop","duration_ms":330000,"artists":[{"name":"Massive Attack"}]}
          ]}
        }"#;
        let d = parse_album_detail(json).unwrap();
        assert_eq!(d.album.name, "Mezzanine");
        assert_eq!(d.tracks.len(), 2);
        // The simplified album-page track has no album of its own: fold in the page's album.
        assert_eq!(d.tracks[0].album, "Mezzanine");
        assert_eq!(d.tracks[0].album_uri.as_deref(), Some("spotify:album:mz"));
        assert_eq!(d.tracks[0].image_url.as_deref(), Some("https://i.scdn.co/mz"));
    }

    #[test]
    fn parses_artist_head_and_dedupes_their_albums() {
        let head =
            parse_artist_detail_head(r#"{"uri":"spotify:artist:ma","id":"ma","name":"Massive Attack"}"#).unwrap();
        assert_eq!(head.name, "Massive Attack");
        let albums = parse_artist_albums(
            r#"{"items":[
              {"uri":"spotify:album:1","id":"1","name":"Mezzanine","artists":[{"name":"MA"}]},
              {"uri":"spotify:album:2","id":"2","name":"mezzanine","artists":[{"name":"MA"}]},
              {"uri":"spotify:album:3","id":"3","name":"Heligoland","artists":[{"name":"MA"}]}
            ]}"#,
        );
        assert_eq!(
            albums.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
            ["Mezzanine", "Heligoland"]
        );
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
        assert_eq!(ApiError::wait_phrase(5), "a moment");
        assert_eq!(ApiError::wait_phrase(61), "about 2 minutes");
        assert_eq!(ApiError::wait_phrase(3600), "about 60 minutes");
        assert_eq!(ApiError::wait_phrase(69817), "about 19 hours");
        assert!(ApiError::RateLimited { retry_after: 69817 }
            .user_message()
            .contains("about 19 hours"));
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
