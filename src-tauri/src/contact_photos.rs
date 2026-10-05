//! Contact photos the iPhone shares over PBAP (a friend's face instead of their initials). The
//! image bytes are kept as files under `<app data>/contacts`, content-addressed by a hash of the
//! bytes, so the contacts table (and every row sent to the UI) holds only a short reference. The
//! photos never leave this machine: unlike app icons there is no network side here at all.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// A contact photo over this is rejected rather than stored. iOS PBAP thumbnails sit well under it;
/// the cap keeps one oversized card from filling the disk or bloating a data URI.
const MAX_BYTES: usize = 200 * 1024;

/// The subfolder under the app data dir that holds photo files (`<hash>.img`).
const DIR: &str = "contacts";

/// JPEG or PNG by magic bytes — the only two formats stored (and the two the web view shows).
fn is_image(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF]) || bytes.starts_with(&[0x89, b'P', b'N', b'G'])
}

/// A stable 64-bit content hash as hex (FNV-1a). Stable across runs and platforms — unlike
/// `DefaultHasher` — so the same photo always maps to the same file, which dedupes shared photos
/// and lets a resync reuse what's already on disk.
fn hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// A file key we wrote: 16 lowercase hex digits. Anything else never reaches a path.
fn valid_key(key: &str) -> bool {
    key.len() == 16 && key.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn path(dir: &Path, key: &str) -> PathBuf {
    dir.join(DIR).join(format!("{key}.img"))
}

/// Validate and store a contact photo, returning its key (the reference saved in the database).
/// Rejects anything that isn't a JPEG or PNG, or is larger than `MAX_BYTES`, by returning `None`.
/// Writing the same photo again is a no-op (same content, same file).
pub fn store_photo(dir: &Path, bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES || !is_image(bytes) {
        return None;
    }
    let key = hash(bytes);
    let file = path(dir, &key);
    if !file.exists() {
        std::fs::create_dir_all(file.parent()?).ok()?;
        std::fs::write(&file, bytes).ok()?;
    }
    Some(key)
}

/// A stored photo as a `data:` URI the web view can show, or `None` if the key is unknown or the
/// file has gone (e.g. cleaned up). The bytes are re-validated on the way out.
pub fn photo_data_uri(dir: &Path, key: &str) -> Option<String> {
    if !valid_key(key) {
        return None;
    }
    let bytes = std::fs::read(path(dir, key)).ok()?;
    is_image(&bytes).then(|| crate::app_icons::data_uri(&bytes)).flatten()
}

/// Delete photo files no contact points at any more (a contact removed on the phone, or one whose
/// photo changed). `keep` is the set of keys still referenced in the database.
pub fn cleanup(dir: &Path, keep: &HashSet<String>) {
    let folder = dir.join(DIR);
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("img") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        if !keep.contains(stem) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 9, 9];

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tug-photo-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn stores_and_round_trips_an_image() {
        let dir = temp();
        let key = store_photo(&dir, JPEG).expect("jpeg stored");
        assert!(valid_key(&key));
        assert!(path(&dir, &key).exists());
        let uri = photo_data_uri(&dir, &key).expect("data uri");
        assert!(uri.starts_with("data:image/jpeg;base64,"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_non_images_and_oversize() {
        let dir = temp();
        assert_eq!(store_photo(&dir, b"<html>not an image"), None);
        assert_eq!(store_photo(&dir, b""), None);
        // A valid JPEG header but past the size cap is still rejected.
        let mut huge = vec![0xFF, 0xD8, 0xFF];
        huge.resize(MAX_BYTES + 1, 0);
        assert_eq!(store_photo(&dir, &huge), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_bytes_share_one_file() {
        let dir = temp();
        let a = store_photo(&dir, PNG).unwrap();
        let b = store_photo(&dir, PNG).unwrap();
        assert_eq!(a, b, "content-addressed: identical photos dedupe");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cleanup_removes_unreferenced_photos_only() {
        let dir = temp();
        let keep_key = store_photo(&dir, JPEG).unwrap();
        let drop_key = store_photo(&dir, PNG).unwrap();
        let keep: HashSet<String> = [keep_key.clone()].into_iter().collect();
        cleanup(&dir, &keep);
        assert!(path(&dir, &keep_key).exists(), "referenced photo kept");
        assert!(!path(&dir, &drop_key).exists(), "orphan photo removed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_keys_never_read() {
        let dir = temp();
        assert_eq!(photo_data_uri(&dir, "../../etc/passwd"), None);
        assert_eq!(photo_data_uri(&dir, "ABCDEF0123456789"), None, "uppercase rejected");
        assert_eq!(photo_data_uri(&dir, "short"), None);
    }
}
