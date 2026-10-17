//! Album art for the Now Playing card when Spotify can't supply it. AMS gives tug the title, artist
//! and album but never artwork, so (while **Show album art** is on) tug asks Apple's public iTunes
//! Search API for the album, picks the best match by artist and album, and keeps the 600×600 cover
//! in `<app data>/album_art`, keyed by a hash of the artist and album.
//!
//! Only the artist and album name are sent, and only when the cover isn't already on disk. Albums
//! Apple doesn't know are remembered for a day so they aren't asked again on every play. Lookups go
//! through a `Gate`: a lookup waits a moment in case the song changes again (rapid skips), and at
//! most one goes out every two seconds. Logs record hits, misses and timings, never the artist or
//! album.
//!
//! The matching and the gate are pure (plain values in, plain values out) and tested here; the
//! network goes through `Net` so the cache, miss memory and the setting can be tested without it.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// An album Apple doesn't know (or whose cover failed to download) is asked again after this.
pub const MISS_RETRY: Duration = Duration::from_secs(24 * 60 * 60);
/// The cover size asked of Apple's image servers (shown at 48 px, sharp on high-DPI screens).
pub const ART_PX: u32 = 600;
/// How many albums the search returns to choose from.
const SEARCH_LIMIT: u32 = 10;
/// A lookup waits this long before going out, so skipping through songs looks up only the one
/// that stays.
pub const DEBOUNCE: Duration = Duration::from_millis(600);
/// At most one lookup this often.
pub const MIN_GAP: Duration = Duration::from_secs(2);
/// Artist and album names longer than this aren't looked up (nothing real is this long).
const MAX_NAME: usize = 300;
/// The folder under tug's app data.
pub const DIR: &str = "album_art";
/// The Show album art switch (Settings › General), on unless it reads "false".
pub const SETTING: &str = "ui.albumArt";

/// Words that mark an edition of an album rather than a different album: "Abbey Road (Remastered
/// 2019)", "Mezzanine [Deluxe Edition]", "Hello - Single".
const EDITION_WORDS: &[&str] = &[
    "remaster",
    "deluxe",
    "expanded",
    "edition",
    "anniversary",
    "bonus",
    "version",
    "special",
    "collector",
    "reissue",
    "legacy",
    "explicit",
    "clean",
    "mono",
    "stereo",
    "extended",
    "single",
    "ep",
    "feat",
    "ft.",
];

/// Lowercase, accents folded, `&` as "and", everything that isn't a letter or digit as one space.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.to_lowercase().replace('&', " and ").chars() {
        let c = fold_accent(c);
        out.push(if c.is_alphanumeric() { c } else { ' ' });
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The common Latin accents, so "Beyoncé" and "BEYONCE" compare equal.
fn fold_accent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
        'ç' | 'č' | 'ć' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
        'ñ' | 'ń' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' => 'u',
        'ý' | 'ÿ' => 'y',
        'š' | 'ś' => 's',
        'ž' | 'ź' | 'ż' => 'z',
        'ł' => 'l',
        _ => c,
    }
}

fn is_edition(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.split(|c: char| !c.is_alphanumeric() && c != '.').any(|word| {
        EDITION_WORDS
            .iter()
            .any(|w| if w.len() <= 3 { word == *w } else { word.starts_with(w) })
    })
}

/// The album without its edition: bracketed groups and " - …" tails that name an edition are
/// dropped ("Abbey Road (Remastered)" → "Abbey Road"); anything else is kept ("Live (At Wembley)").
pub fn strip_edition(album: &str) -> String {
    let mut out = String::with_capacity(album.len());
    let mut rest = album;
    while let Some(open) = rest.find(['(', '[']) {
        let close_char = if rest[open..].starts_with('(') { ')' } else { ']' };
        let Some(len) = rest[open..].find(close_char) else {
            break;
        };
        let inner = &rest[open + 1..open + len];
        out.push_str(&rest[..open]);
        if !is_edition(inner) {
            out.push_str(&rest[open..=open + len]);
        }
        rest = &rest[open + len + 1..];
    }
    out.push_str(rest);
    // " - Remastered 2011", " - Single", " - EP": a dash tail that names an edition.
    let mut s = out.trim().to_string();
    while let Some(i) = s.rfind(" - ") {
        if is_edition(&s[i + 3..]) {
            s.truncate(i);
            s = s.trim().to_string();
        } else {
            break;
        }
    }
    s
}

/// The artist without the featured ones: "Calvin Harris feat. Rihanna" → "Calvin Harris".
pub fn lead_artist(artist: &str) -> String {
    let lower = artist.to_lowercase();
    let mut cut = artist.len();
    for sep in [
        " feat. ",
        " feat ",
        " ft. ",
        " ft ",
        " featuring ",
        " with ",
        "(feat",
        "(ft.",
        "(with ",
        "[feat",
    ] {
        if let Some(i) = lower.find(sep) {
            cut = cut.min(i);
        }
    }
    // `to_lowercase` can change byte lengths for a few scripts; never slice off a char boundary.
    let cut = if artist.is_char_boundary(cut) {
        cut
    } else {
        artist.len()
    };
    artist[..cut].trim().to_string()
}

/// The first named artist of a duo or list: "Calvin Harris & Disciples" → "Calvin Harris".
pub fn primary_artist(artist: &str) -> String {
    let lead = lead_artist(artist);
    let lower = lead.to_lowercase();
    let mut cut = lead.len();
    for sep in [" & ", ", ", " x ", " and ", " / "] {
        if let Some(i) = lower.find(sep) {
            cut = cut.min(i);
        }
    }
    let cut = if lead.is_char_boundary(cut) { cut } else { lead.len() };
    lead[..cut].trim().to_string()
}

/// The cache file stem: a hash of the lead artist and the album without its edition, so
/// "Mezzanine" and "Mezzanine (Remastered)" share one cover, and the names never reach a path.
pub fn cache_key(artist: &str, album: &str) -> String {
    use sha2::{Digest, Sha256};
    let text = format!(
        "{}\n{}",
        normalize(&lead_artist(artist)),
        normalize(&strip_edition(album))
    );
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

/// The search address: the lead artist and the album without its edition, as one term.
pub fn search_url(artist: &str, album: &str) -> String {
    let term = format!(
        "{} {}",
        normalize(&lead_artist(artist)),
        normalize(&strip_edition(album))
    );
    format!(
        "https://itunes.apple.com/search?term={}&media=music&entity=album&limit={SEARCH_LIMIT}",
        percent_encode(term.trim())
    )
}

/// A query value with everything but unreserved characters percent-encoded (RFC 3986).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// One album from a search answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub artist: String,
    pub album: String,
    /// The cover at `ART_PX`, on Apple's image servers.
    pub art: String,
}

/// A cover address from a search result, resized to `ART_PX` ("…/100x100bb.jpg" → "…/600x600bb.jpg").
/// Only `https` on Apple's image servers (`*.mzstatic.com`).
pub fn art_url(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let host = rest.split('/').next()?;
    if !(host == "mzstatic.com" || host.ends_with(".mzstatic.com")) {
        return None;
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let i = url.rfind('/')?;
    if url[i + 1..].contains("bb.") {
        Some(format!("{}/{ART_PX}x{ART_PX}bb.jpg", &url[..i]))
    } else {
        Some(url.to_string())
    }
}

/// The albums in a search answer that have a usable cover, in Apple's order.
pub fn candidates(json: &str) -> Vec<Candidate> {
    let Ok(j) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(results) = j.get("results").and_then(|r| r.as_array()) else {
        return Vec::new();
    };
    results
        .iter()
        .filter_map(|r| {
            let artist = r.get("artistName")?.as_str()?.to_string();
            let album = r.get("collectionName")?.as_str()?.to_string();
            let art = ["artworkUrl100", "artworkUrl60"]
                .iter()
                .find_map(|k| r.get(*k)?.as_str())
                .and_then(art_url)?;
            Some(Candidate { artist, album, art })
        })
        .collect()
}

fn words(s: &str) -> std::collections::BTreeSet<&str> {
    s.split(' ').filter(|w| !w.is_empty()).collect()
}

/// How alike two normalised titles are by their words (0 to 1).
fn word_overlap(a: &str, b: &str) -> f64 {
    let (a, b) = (words(a), words(b));
    let union = a.union(&b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(&b).count() as f64 / union as f64
}

/// How well an album title matches: exact, the same once editions are dropped, or mostly the same
/// words. 0 is no match.
fn album_score(wanted: &str, found: &str) -> u32 {
    let (w, f) = (normalize(wanted), normalize(found));
    if w.is_empty() || f.is_empty() {
        return 0;
    }
    if w == f {
        return 60;
    }
    let (ws, fs) = (normalize(&strip_edition(wanted)), normalize(&strip_edition(found)));
    if !ws.is_empty() && ws == fs {
        return 50;
    }
    if word_overlap(&ws, &fs) >= 0.75 {
        return 30;
    }
    0
}

/// How well an artist matches: exact, the same lead artist (featured ones dropped), or the same
/// first-named artist. 0 is no match.
fn artist_score(wanted: &str, found: &str) -> u32 {
    let (w, f) = (normalize(wanted), normalize(found));
    if w.is_empty() || f.is_empty() {
        return 0;
    }
    if w == f {
        return 40;
    }
    if normalize(&lead_artist(wanted)) == normalize(&lead_artist(found)) {
        return 36;
    }
    let (wp, fp) = (normalize(&primary_artist(wanted)), normalize(&primary_artist(found)));
    if !wp.is_empty() && wp == fp {
        return 30;
    }
    0
}

/// The candidate's score for this artist and album; 0 unless both the artist and album match.
pub fn score(c: &Candidate, artist: &str, album: &str) -> u32 {
    match (artist_score(artist, &c.artist), album_score(album, &c.album)) {
        (0, _) | (_, 0) => 0,
        (a, b) => a + b,
    }
}

/// The best-matching album's cover from a search answer, or `None` when nothing matches both the
/// artist and the album. Ties go to Apple's order (its own relevance).
pub fn pick(json: &str, artist: &str, album: &str) -> Option<String> {
    let mut best: Option<(u32, Candidate)> = None;
    for c in candidates(json) {
        let s = score(&c, artist, album);
        if s > 0 && best.as_ref().is_none_or(|(b, _)| s > *b) {
            best = Some((s, c));
        }
    }
    best.map(|(_, c)| c.art)
}

/// Whether a lookup may go out: the setting reads anything but "false" (on by default).
pub fn enabled_from_setting(value: Option<&str>) -> bool {
    value != Some("false")
}

/// What a lookup waiting at the gate should do now.
#[derive(Debug, PartialEq, Eq)]
pub enum Admit {
    /// Go ahead (and the gap starts now).
    Go,
    /// Ask again after this long.
    Wait(Duration),
    /// A newer lookup arrived (the song changed again): give up.
    Superseded,
}

/// Debounce and rate limit for lookups. Each lookup takes a ticket; only the newest ticket may go,
/// after it has waited `debounce` and at least `min_gap` has passed since the last one went.
pub struct Gate {
    debounce: Duration,
    min_gap: Duration,
    newest: u64,
    last: Option<Instant>,
}

impl Gate {
    pub const fn new(debounce: Duration, min_gap: Duration) -> Self {
        Self {
            debounce,
            min_gap,
            newest: 0,
            last: None,
        }
    }

    /// A new lookup: every older one still waiting is superseded.
    pub fn ticket(&mut self) -> u64 {
        self.newest += 1;
        self.newest
    }

    /// May the lookup holding `ticket`, asked for at `asked`, go at `now`?
    pub fn admit(&mut self, ticket: u64, asked: Instant, now: Instant) -> Admit {
        if ticket != self.newest {
            return Admit::Superseded;
        }
        let waited = now.saturating_duration_since(asked);
        if waited < self.debounce {
            return Admit::Wait(self.debounce - waited);
        }
        if let Some(last) = self.last {
            let since = now.saturating_duration_since(last);
            if since < self.min_gap {
                return Admit::Wait(self.min_gap - since);
            }
        }
        self.last = Some(now);
        Admit::Go
    }
}

/// The app's gate: every lookup goes through it.
pub static GATE: Mutex<Gate> = Mutex::new(Gate::new(DEBOUNCE, MIN_GAP));

/// Wait at the gate until this lookup may go (true) or a newer request replaced it (false).
fn wait_turn(gate: &Mutex<Gate>, ticket: u64, asked: Instant) -> bool {
    loop {
        let admit = gate
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .admit(ticket, asked, Instant::now());
        match admit {
            Admit::Go => return true,
            Admit::Superseded => return false,
            Admit::Wait(d) => std::thread::sleep(d.min(MIN_GAP)),
        }
    }
}

/// The network, so the cache, the miss memory and the setting can be tested without it.
pub trait Net {
    fn get_string(&self, url: &str) -> Result<String, String>;
    fn get_bytes(&self, url: &str) -> Result<Vec<u8>, String>;
}

/// The real network: Windows' HTTP client, time-limited (shared with the app icon lookup).
pub struct Http;

impl Net for Http {
    fn get_string(&self, url: &str) -> Result<String, String> {
        crate::app_icons::http::get_string(url)
    }
    fn get_bytes(&self, url: &str) -> Result<Vec<u8>, String> {
        crate::app_icons::http::get_bytes(url)
    }
}

fn paths(dir: &Path, key: &str) -> (PathBuf, PathBuf) {
    let art = dir.join(DIR);
    (art.join(format!("{key}.img")), art.join(format!("{key}.none")))
}

fn recent_miss(marker: &Path, now: SystemTime) -> bool {
    std::fs::metadata(marker)
        .and_then(|m| m.modified())
        .is_ok_and(|t| now.duration_since(t).unwrap_or_default() < MISS_RETRY)
}

/// Remember a miss from now. Rewriting an empty file doesn't move its modified time on Windows, so
/// the time is set outright (otherwise a stale marker would let every play look the album up again).
fn mark_miss(marker: &Path) {
    let file = std::fs::File::create(marker);
    if let Ok(f) = file {
        let _ = f.set_modified(SystemTime::now());
    }
}

/// A name worth looking up: something left after trimming, and not absurdly long.
fn usable(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && n.chars().count() <= MAX_NAME && !normalize(n).is_empty()
}

/// The album's cover as a `data:` URI: from the cache, or looked up now. `Ok(None)` when the setting
/// is off, the names aren't usable, Apple has no match (remembered for a day), or a newer lookup
/// replaced this one; `Err` for a network failure (asked again next time).
pub fn art(dir: &Path, artist: &str, album: &str, enabled: bool) -> Result<Option<String>, String> {
    art_with(dir, artist, album, enabled, &Http, &GATE)
}

/// `art` with the network and the gate passed in (tests use a fake network and an open gate).
pub fn art_with(
    dir: &Path,
    artist: &str,
    album: &str,
    enabled: bool,
    net: &dyn Net,
    gate: &Mutex<Gate>,
) -> Result<Option<String>, String> {
    if !enabled || !usable(artist) || !usable(album) {
        return Ok(None);
    }
    let started = Instant::now();
    // Every request takes a ticket, cached or not: the song changing again (even to an album
    // already on disk) makes a lookup still waiting at the gate give up.
    let ticket = gate.lock().unwrap_or_else(|e| e.into_inner()).ticket();
    let key = cache_key(artist, album);
    let (img, miss) = paths(dir, &key);
    if let Ok(bytes) = std::fs::read(&img) {
        if let Some(uri) = crate::app_icons::data_uri(&bytes) {
            log::debug!("album art: cache hit in {} ms", started.elapsed().as_millis());
            return Ok(Some(uri));
        }
        // Not an image after all (a damaged file): look it up again.
        let _ = std::fs::remove_file(&img);
    }
    if recent_miss(&miss, SystemTime::now()) {
        log::debug!("album art: known miss, not asked again yet");
        return Ok(None);
    }
    if !wait_turn(gate, ticket, started) {
        log::debug!("album art: lookup skipped, the song changed");
        return Ok(None);
    }
    let asked = Instant::now();
    let answer = net.get_string(&search_url(artist, album)).inspect_err(|_| {
        log::info!("album art: lookup failed after {} ms", asked.elapsed().as_millis());
    })?;
    let bytes = match pick(&answer, artist, album) {
        Some(url) => net.get_bytes(&url).inspect_err(|_| {
            log::info!("album art: download failed after {} ms", asked.elapsed().as_millis());
        })?,
        None => Vec::new(),
    };
    let _ = std::fs::create_dir_all(dir.join(DIR));
    match crate::app_icons::data_uri(&bytes) {
        Some(uri) => {
            std::fs::write(&img, &bytes).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(&miss);
            log::info!(
                "album art: found in {} ms ({} KB)",
                asked.elapsed().as_millis(),
                bytes.len() / 1024
            );
            crate::cache_trim::ALBUM_ART_TRIM.after_write(&dir.join(DIR), crate::cache_trim::ALBUM_ART_CAP);
            Ok(Some(uri))
        }
        None => {
            mark_miss(&miss);
            log::info!("album art: no match ({} ms)", asked.elapsed().as_millis());
            Ok(None)
        }
    }
}

/// Drop miss markers older than a day (they no longer stop a lookup), so the folder doesn't
/// collect them forever. Best-effort, run with the startup cache trim.
pub fn sweep_misses(art_dir: &Path, now: SystemTime) {
    let Ok(entries) = std::fs::read_dir(art_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("none") && !recent_miss(&path, now) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn result(artist: &str, album: &str, id: &str) -> String {
        format!(
            r#"{{"wrapperType":"collection","collectionType":"Album","artistName":"{artist}","collectionName":"{album}","artworkUrl100":"https://is1-ssl.mzstatic.com/image/thumb/Music/{id}/source/100x100bb.jpg"}}"#
        )
    }

    fn answer(results: &[String]) -> String {
        format!(
            r#"{{"resultCount":{},"results":[{}]}}"#,
            results.len(),
            results.join(",")
        )
    }

    fn art_for(id: &str) -> String {
        format!("https://is1-ssl.mzstatic.com/image/thumb/Music/{id}/source/600x600bb.jpg")
    }

    #[test]
    fn exact_match_beats_an_edition() {
        let j = answer(&[
            result("Massive Attack", "Mezzanine (Deluxe)", "deluxe"),
            result("Massive Attack", "Mezzanine", "plain"),
        ]);
        assert_eq!(pick(&j, "Massive Attack", "Mezzanine"), Some(art_for("plain")));
    }

    #[test]
    fn remastered_and_deluxe_suffixes_still_match() {
        // Only an edition in the catalog.
        let j = answer(&[result("Massive Attack", "Mezzanine [Deluxe Edition]", "deluxe")]);
        assert_eq!(pick(&j, "Massive Attack", "Mezzanine"), Some(art_for("deluxe")));
        // The phone names an edition the catalog doesn't (or the other way round).
        let j = answer(&[
            result("The Beatles", "Let It Be", "lib"),
            result("The Beatles", "Abbey Road", "abbey"),
        ]);
        assert_eq!(
            pick(&j, "The Beatles", "Abbey Road (Remastered 2019)"),
            Some(art_for("abbey"))
        );
        assert_eq!(
            pick(&j, "The Beatles", "Abbey Road - Remastered"),
            Some(art_for("abbey"))
        );
        let j = answer(&[result("Adele", "Hello - Single", "hello")]);
        assert_eq!(pick(&j, "Adele", "Hello"), Some(art_for("hello")));
    }

    #[test]
    fn fuzzy_titles_match_but_different_albums_do_not() {
        let j = answer(&[result("Pink Floyd", "Dark Side of the Moon", "dsotm")]);
        assert_eq!(
            pick(&j, "Pink Floyd", "The Dark Side of the Moon"),
            Some(art_for("dsotm"))
        );
        // Accents and case.
        let j = answer(&[result("BEYONCE", "Lemonade", "lem")]);
        assert_eq!(pick(&j, "Beyoncé", "LEMONADE"), Some(art_for("lem")));
        // A different album by the right artist is no match.
        let j = answer(&[result("Pink Floyd", "The Wall", "wall")]);
        assert_eq!(pick(&j, "Pink Floyd", "Animals"), None);
    }

    #[test]
    fn featured_artists_match_the_lead_artist() {
        let j = answer(&[result("Calvin Harris", "18 Months", "18m")]);
        assert_eq!(
            pick(&j, "Calvin Harris feat. Rihanna", "18 Months"),
            Some(art_for("18m"))
        );
        let j = answer(&[result(
            "Calvin Harris & Disciples",
            "How Deep Is Your Love - Single",
            "hd",
        )]);
        assert_eq!(pick(&j, "Calvin Harris", "How Deep Is Your Love"), Some(art_for("hd")));
        // A duo is one artist when both sides name it the same way.
        let j = answer(&[result("Simon & Garfunkel", "Bookends", "bk")]);
        assert_eq!(pick(&j, "Simon and Garfunkel", "Bookends"), Some(art_for("bk")));
    }

    #[test]
    fn the_right_artist_wins_a_shared_title() {
        let j = answer(&[
            result("ABBA", "Greatest Hits", "abba"),
            result("Queen", "Greatest Hits", "queen"),
        ]);
        assert_eq!(pick(&j, "Queen", "Greatest Hits"), Some(art_for("queen")));
        // Nobody by that name: nothing, rather than the wrong cover.
        assert_eq!(pick(&j, "Nobody Real", "Greatest Hits"), None);
    }

    #[test]
    fn bad_answers_pick_nothing() {
        assert_eq!(pick("not json", "A", "B"), None);
        assert_eq!(pick(r#"{"resultCount":0,"results":[]}"#, "A", "B"), None);
        // A cover anywhere but Apple's image servers is ignored.
        let j = r#"{"results":[{"artistName":"A","collectionName":"B","artworkUrl100":"https://evil.example/100x100bb.jpg"}]}"#;
        assert_eq!(pick(j, "A", "B"), None);
        let j = r#"{"results":[{"artistName":"A","collectionName":"B","artworkUrl100":"http://a1.mzstatic.com/100x100bb.jpg"}]}"#;
        assert_eq!(pick(j, "A", "B"), None);
    }

    #[test]
    fn editions_and_featured_artists_are_stripped() {
        assert_eq!(strip_edition("Abbey Road (Remastered 2019)"), "Abbey Road");
        assert_eq!(strip_edition("Mezzanine [Deluxe Edition]"), "Mezzanine");
        assert_eq!(strip_edition("Rumours (Super Deluxe) - Remastered"), "Rumours");
        assert_eq!(strip_edition("Thing - EP"), "Thing");
        // Not an edition: kept.
        assert_eq!(strip_edition("Live (At Wembley)"), "Live (At Wembley)");
        assert_eq!(strip_edition("Ep"), "Ep");
        assert_eq!(strip_edition("Unclosed (Deluxe"), "Unclosed (Deluxe");
        assert_eq!(lead_artist("Calvin Harris feat. Rihanna"), "Calvin Harris");
        assert_eq!(lead_artist("Artist (feat. Someone)"), "Artist");
        assert_eq!(primary_artist("Calvin Harris & Disciples"), "Calvin Harris");
        assert_eq!(primary_artist("Solo"), "Solo");
    }

    #[test]
    fn search_sends_only_the_artist_and_album() {
        let url = search_url("Calvin Harris feat. Rihanna", "18 Months (Deluxe)");
        assert_eq!(
            url,
            "https://itunes.apple.com/search?term=calvin%20harris%2018%20months&media=music&entity=album&limit=10"
        );
    }

    #[test]
    fn cache_keys_are_stable_safe_and_edition_blind() {
        let k = cache_key("Massive Attack", "Mezzanine");
        assert_eq!(k.len(), 32);
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(k, cache_key("massive attack", "MEZZANINE"));
        assert_eq!(k, cache_key("Massive Attack", "Mezzanine (Remastered)"));
        assert_eq!(k, cache_key("Massive Attack feat. Someone", "Mezzanine"));
        assert_ne!(k, cache_key("Massive Attack", "Heligoland"));
        assert_ne!(k, cache_key("Someone Else", "Mezzanine"));
        // The artist/album boundary matters: "A B" + "C" isn't "A" + "B C".
        assert_ne!(cache_key("A B", "C"), cache_key("A", "B C"));
    }

    #[test]
    fn the_gate_debounces_and_spaces_lookups() {
        let mut g = Gate::new(Duration::from_millis(600), Duration::from_secs(2));
        let t0 = Instant::now();
        let a = g.ticket();
        assert_eq!(g.admit(a, t0, t0), Admit::Wait(Duration::from_millis(600)));
        // Skipped again before the wait was over: the first lookup gives up.
        let b = g.ticket();
        let t1 = t0 + Duration::from_millis(300);
        assert_eq!(g.admit(a, t0, t1), Admit::Superseded);
        assert_eq!(g.admit(b, t1, t1 + Duration::from_millis(600)), Admit::Go);
        // The next lookup, debounced, still waits out the two-second gap.
        let c = g.ticket();
        let t2 = t1 + Duration::from_millis(900);
        let wait = g.admit(c, t2, t2 + Duration::from_millis(700));
        assert_eq!(wait, Admit::Wait(Duration::from_millis(1000)));
        assert_eq!(g.admit(c, t2, t1 + Duration::from_millis(2600)), Admit::Go);
    }

    #[test]
    fn the_setting_is_on_unless_switched_off() {
        assert!(enabled_from_setting(None));
        assert!(enabled_from_setting(Some("true")));
        assert!(!enabled_from_setting(Some("false")));
    }

    /// A fake network that records what it was asked.
    struct FakeNet {
        search: String,
        image: Vec<u8>,
        asked: RefCell<Vec<String>>,
    }

    impl Net for FakeNet {
        fn get_string(&self, url: &str) -> Result<String, String> {
            self.asked.borrow_mut().push(url.to_string());
            Ok(self.search.clone())
        }
        fn get_bytes(&self, url: &str) -> Result<Vec<u8>, String> {
            self.asked.borrow_mut().push(url.to_string());
            Ok(self.image.clone())
        }
    }

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 1, 2, 3];

    fn fake(results: &[String]) -> FakeNet {
        FakeNet {
            search: answer(results),
            image: PNG.to_vec(),
            asked: RefCell::new(Vec::new()),
        }
    }

    fn open_gate() -> Mutex<Gate> {
        Mutex::new(Gate::new(Duration::ZERO, Duration::ZERO))
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tug-album-art-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn looks_up_once_then_serves_from_the_cache() {
        let dir = temp("cache");
        let net = fake(&[result("Massive Attack", "Mezzanine", "m")]);
        let gate = open_gate();
        let first = art_with(&dir, "Massive Attack", "Mezzanine", true, &net, &gate).unwrap();
        assert_eq!(first.as_deref(), Some("data:image/png;base64,iVBORwECAw=="));
        assert_eq!(net.asked.borrow().len(), 2, "one search, one download");
        assert!(dir
            .join(DIR)
            .join(format!("{}.img", cache_key("Massive Attack", "Mezzanine")))
            .exists());
        // Same album again (even named as an edition): no network at all.
        let again = art_with(&dir, "Massive Attack", "Mezzanine (Remastered)", true, &net, &gate).unwrap();
        assert_eq!(again, first);
        assert_eq!(net.asked.borrow().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn misses_are_remembered_for_a_day() {
        let dir = temp("miss");
        let net = fake(&[result("Someone Else", "Other Album", "x")]);
        let gate = open_gate();
        assert_eq!(art_with(&dir, "Nobody", "Nothing", true, &net, &gate).unwrap(), None);
        assert_eq!(net.asked.borrow().len(), 1, "searched, nothing to download");
        // Asked again soon: not looked up.
        assert_eq!(art_with(&dir, "Nobody", "Nothing", true, &net, &gate).unwrap(), None);
        assert_eq!(net.asked.borrow().len(), 1);
        // A day later it's worth asking again.
        let marker = dir.join(DIR).join(format!("{}.none", cache_key("Nobody", "Nothing")));
        let old = SystemTime::now() - MISS_RETRY - Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&marker)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(art_with(&dir, "Nobody", "Nothing", true, &net, &gate).unwrap(), None);
        assert_eq!(net.asked.borrow().len(), 2);
        // The startup sweep keeps fresh markers and drops stale ones.
        let stale = dir.join(DIR).join("stale.none");
        std::fs::write(&stale, b"").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(old)
            .unwrap();
        sweep_misses(&dir.join(DIR), SystemTime::now());
        assert!(!stale.exists());
        assert!(marker.exists(), "re-marked by the last lookup, so still fresh");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn switched_off_or_unusable_names_never_reach_the_network() {
        let dir = temp("off");
        let net = fake(&[result("Massive Attack", "Mezzanine", "m")]);
        let gate = open_gate();
        assert_eq!(
            art_with(&dir, "Massive Attack", "Mezzanine", false, &net, &gate).unwrap(),
            None
        );
        assert_eq!(art_with(&dir, "  ", "Mezzanine", true, &net, &gate).unwrap(), None);
        assert_eq!(art_with(&dir, "Massive Attack", "", true, &net, &gate).unwrap(), None);
        assert_eq!(
            art_with(&dir, "Massive Attack", &"x".repeat(400), true, &net, &gate).unwrap(),
            None
        );
        assert!(net.asked.borrow().is_empty());
        assert!(!dir.exists(), "nothing written either");
    }

    #[test]
    fn a_superseded_lookup_stays_off_the_network() {
        let dir = temp("superseded");
        let net = fake(&[result("Massive Attack", "Mezzanine", "m")]);
        // A long debounce, and a newer ticket taken while this one waits.
        let gate = Mutex::new(Gate::new(Duration::from_millis(400), Duration::ZERO));
        std::thread::scope(|s| {
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(100));
                gate.lock().unwrap().ticket();
            });
            assert_eq!(
                art_with(&dir, "Massive Attack", "Mezzanine", true, &net, &gate).unwrap(),
                None
            );
        });
        assert!(net.asked.borrow().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Network check against Apple's real catalog:
    /// `cargo test --lib album_art -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn fetches_a_real_cover_once_then_from_the_cache() {
        let dir = temp("network");
        let gate = open_gate();
        let first = art_with(&dir, "Massive Attack", "Mezzanine", true, &Http, &gate)
            .expect("lookup failed")
            .expect("no match");
        assert!(first.starts_with("data:image/jpeg;base64,"));
        assert_eq!(
            art_with(&dir, "Massive Attack", "Mezzanine", true, &Http, &gate)
                .unwrap()
                .unwrap(),
            first
        );
        let none = art_with(
            &dir,
            "Nobody Real Tug Test",
            "No Such Album Tug Test",
            true,
            &Http,
            &gate,
        )
        .unwrap();
        assert_eq!(none, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_album_art_folder_is_trimmed_by_least_recent_use() {
        let dir = temp("trim");
        let art = dir.join(DIR);
        std::fs::create_dir_all(&art).unwrap();
        let old = cache_key("Old", "Album");
        let new = cache_key("New", "Album");
        std::fs::write(art.join(format!("{old}.img")), vec![0u8; 60]).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(art.join(format!("{new}.img")), vec![0u8; 60]).unwrap();
        std::fs::write(art.join(format!("{}.none", cache_key("Miss", "Album"))), b"").unwrap();
        crate::cache_trim::trim(&art, 100);
        assert!(!art.join(format!("{old}.img")).exists());
        assert!(art.join(format!("{new}.img")).exists());
        assert!(art.join(format!("{}.none", cache_key("Miss", "Album"))).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
