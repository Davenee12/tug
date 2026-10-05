//! Real app icons for the Feed (Gmail's envelope instead of "GM"). iOS names the app behind
//! each notification by its bundle id; Apple's App Store lookup turns that into the app's icon.
//! Fetched once per app (only the bundle id is sent), then kept in `<app data>/icons`.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// An app the App Store doesn't know (or whose icon failed to download) is asked again after this.
const MISS_RETRY: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// Shown at 32-40 px; 128 keeps it sharp on high-DPI screens and small on disk.
const ICON_PX: u32 = 128;

/// Bundle ids are reverse-DNS: letters, digits, dots, dashes, underscores. Anything else never
/// reaches a URL or a file name.
pub fn valid_app_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id.contains('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// The icon URL from an App Store lookup answer, resized to `ICON_PX`.
pub fn artwork_url(lookup_json: &str) -> Option<String> {
    let j: serde_json::Value = serde_json::from_str(lookup_json).ok()?;
    let app = j.get("results")?.as_array()?.first()?;
    let url = ["artworkUrl100", "artworkUrl512", "artworkUrl60"]
        .iter()
        .find_map(|k| app.get(*k)?.as_str())?;
    if !url.starts_with("https://") {
        return None;
    }
    // ".../100x100bb.jpg" → ".../128x128bb.png" (the CDN renders any size and format).
    let resized = match url.rfind('/') {
        Some(i) if url[i + 1..].contains("bb.") => format!("{}/{ICON_PX}x{ICON_PX}bb.png", &url[..i]),
        _ => url.to_string(),
    };
    Some(resized)
}

fn mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else {
        None
    }
}

/// A `data:` URI the web view can show under tug's CSP (`img-src 'self' data:`).
pub fn data_uri(bytes: &[u8]) -> Option<String> {
    use base64::Engine as _;
    let mime = mime(bytes)?;
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// The app's own website from an App Store lookup answer (`sellerUrl`), for the Feed's "Open".
pub fn seller_url(lookup_json: &str) -> Option<String> {
    let j: serde_json::Value = serde_json::from_str(lookup_json).ok()?;
    let url = j
        .get("results")?
        .as_array()?
        .first()?
        .get("sellerUrl")?
        .as_str()?
        .trim();
    let ok = (url.starts_with("https://") || url.starts_with("http://"))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control());
    ok.then(|| url.to_string())
}

/// The app's website: from the cache (`<id>.url`), or looked up now. `Ok(None)` when the App
/// Store has none (remembered for a week); `Err` for a network failure (asked again next time).
pub fn website(dir: &Path, app_id: &str) -> Result<Option<String>, String> {
    if !valid_app_id(app_id) {
        return Ok(None);
    }
    let icons = dir.join("icons");
    let (url_file, miss) = (
        icons.join(format!("{app_id}.url")),
        icons.join(format!("{app_id}.nourl")),
    );
    if let Ok(url) = std::fs::read_to_string(&url_file) {
        return Ok(Some(url.trim().to_string()).filter(|u| !u.is_empty()));
    }
    let recent_miss = std::fs::metadata(&miss)
        .and_then(|m| m.modified())
        .is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < MISS_RETRY);
    if recent_miss {
        return Ok(None);
    }
    let lookup = http::get_string(&format!("https://itunes.apple.com/lookup?bundleId={app_id}"))?;
    let _ = std::fs::create_dir_all(&icons);
    match seller_url(&lookup) {
        Some(url) => {
            std::fs::write(&url_file, &url).map_err(|e| e.to_string())?;
            Ok(Some(url))
        }
        None => {
            let _ = std::fs::write(&miss, b"");
            Ok(None)
        }
    }
}

fn paths(dir: &Path, app_id: &str) -> (PathBuf, PathBuf) {
    let icons = dir.join("icons");
    (
        icons.join(format!("{app_id}.img")),
        icons.join(format!("{app_id}.none")),
    )
}

/// The app's icon as a data URI: from the cache, or fetched now. `Ok(None)` when the App Store
/// has no icon for it (remembered for a week); `Err` for a network failure (asked again next time).
pub fn icon(dir: &Path, app_id: &str) -> Result<Option<String>, String> {
    if !valid_app_id(app_id) {
        return Ok(None);
    }
    let (img, miss) = paths(dir, app_id);
    if let Ok(bytes) = std::fs::read(&img) {
        return Ok(data_uri(&bytes));
    }
    let recent_miss = std::fs::metadata(&miss)
        .and_then(|m| m.modified())
        .is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < MISS_RETRY);
    if recent_miss {
        return Ok(None);
    }
    let lookup = http::get_string(&format!("https://itunes.apple.com/lookup?bundleId={app_id}"))?;
    let bytes = match artwork_url(&lookup) {
        Some(url) => http::get_bytes(&url)?,
        None => Vec::new(),
    };
    let _ = std::fs::create_dir_all(img.parent().expect("icons dir"));
    match data_uri(&bytes) {
        Some(uri) => {
            std::fs::write(&img, &bytes).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(&miss);
            Ok(Some(uri))
        }
        None => {
            let _ = std::fs::write(&miss, b"");
            Ok(None)
        }
    }
}

#[cfg(windows)]
mod http {
    use std::time::Duration;
    use windows::core::HSTRING;
    use windows::Foundation::Uri;
    use windows::Web::Http::HttpClient;

    const TIMEOUT: Duration = Duration::from_secs(15);

    fn run<T>(f: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .map_err(|e| e.to_string())?
            .block_on(f)
    }

    fn err(e: windows::core::Error) -> String {
        e.message().to_string()
    }

    pub fn get_string(url: &str) -> Result<String, String> {
        run(async {
            let client = HttpClient::new().map_err(err)?;
            let uri = Uri::CreateUri(&HSTRING::from(url)).map_err(err)?;
            let body = tokio::time::timeout(TIMEOUT, client.GetStringAsync(&uri).map_err(err)?)
                .await
                .map_err(|_| "App Store lookup timed out".to_string())?
                .map_err(err)?;
            Ok(body.to_string())
        })
    }

    pub fn get_bytes(url: &str) -> Result<Vec<u8>, String> {
        run(async {
            let client = HttpClient::new().map_err(err)?;
            let uri = Uri::CreateUri(&HSTRING::from(url)).map_err(err)?;
            let buf = tokio::time::timeout(TIMEOUT, client.GetBufferAsync(&uri).map_err(err)?)
                .await
                .map_err(|_| "Icon download timed out".to_string())?
                .map_err(err)?;
            crate::ble::winrt::from_buffer(&buf).map_err(err)
        })
    }
}

#[cfg(not(windows))]
mod http {
    pub fn get_string(_url: &str) -> Result<String, String> {
        Err("App icons are only fetched on Windows".into())
    }
    pub fn get_bytes(_url: &str) -> Result<Vec<u8>, String> {
        Err("App icons are only fetched on Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_bundle_ids_get_through() {
        assert!(valid_app_id("com.google.Gmail"));
        assert!(valid_app_id("com.toyopagroup.picaboo"));
        assert!(!valid_app_id("Gmail"));
        assert!(!valid_app_id("com.evil/../../x"));
        assert!(!valid_app_id("com.a&b=c"));
        assert!(!valid_app_id(""));
    }

    #[test]
    fn artwork_is_resized_to_a_png() {
        let j = r#"{"resultCount":1,"results":[{"artworkUrl100":"https://is1-ssl.mzstatic.com/image/thumb/Purple/a/b/AppIcon.png/100x100bb.jpg"}]}"#;
        assert_eq!(
            artwork_url(j).as_deref(),
            Some("https://is1-ssl.mzstatic.com/image/thumb/Purple/a/b/AppIcon.png/128x128bb.png")
        );
        assert_eq!(artwork_url(r#"{"resultCount":0,"results":[]}"#), None);
        assert_eq!(
            artwork_url(r#"{"results":[{"artworkUrl100":"http://x/100x100bb.jpg"}]}"#),
            None
        );
        assert_eq!(artwork_url("not json"), None);
    }

    #[test]
    fn seller_url_only_for_web_addresses() {
        let j =
            r#"{"resultCount":1,"results":[{"sellerUrl":"https://www.chase.com/online/services/mobile-banking.htm"}]}"#;
        assert_eq!(
            seller_url(j).as_deref(),
            Some("https://www.chase.com/online/services/mobile-banking.htm")
        );
        assert_eq!(seller_url(r#"{"results":[{"sellerUrl":"javascript:alert(1)"}]}"#), None);
        assert_eq!(seller_url(r#"{"results":[{"sellerUrl":"https://a.com/x y"}]}"#), None);
        assert_eq!(seller_url(r#"{"results":[{}]}"#), None);
        assert_eq!(seller_url(r#"{"resultCount":0,"results":[]}"#), None);
    }

    #[test]
    fn data_uris_only_for_images() {
        assert_eq!(
            data_uri(&[0x89, b'P', b'N', b'G', 1]).unwrap(),
            "data:image/png;base64,iVBORwE="
        );
        assert!(data_uri(&[0xFF, 0xD8, 0xFF, 0])
            .unwrap()
            .starts_with("data:image/jpeg;base64,"));
        assert_eq!(data_uri(b"<html>"), None);
    }

    /// Network check: `cargo test --lib app_icons -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn fetches_gmail_once_then_from_cache() {
        let dir = std::env::temp_dir().join("tug-icon-test");
        let _ = std::fs::remove_dir_all(&dir);
        let first = icon(&dir, "com.google.Gmail").expect("fetch failed").expect("no icon");
        assert!(first.starts_with("data:image/png;base64,"));
        assert!(dir.join("icons/com.google.Gmail.img").exists());
        assert_eq!(icon(&dir, "com.google.Gmail").unwrap().unwrap(), first);
        assert_eq!(icon(&dir, "com.example.does.not.exist.tug").unwrap(), None);
        assert!(dir.join("icons/com.example.does.not.exist.tug.none").exists());
    }
}
