//! A size cap for the purely-cached image folders — album art and playlist covers
//! (`<cache>/spotify_art`) and app icons (`<data>/icons`). Both are rebuilt on demand from the
//! network, so the oldest entries can be dropped freely to keep the folder from growing without
//! limit. Contact photos are *not* capped here: they never leave the machine and can't be re-fetched,
//! so they're pruned only by reference (see `contact_photos::cleanup`).
//!
//! The selection of which files to drop is pure (`over_cap`) so it can be unit-tested without a disk;
//! `trim` is the thin I/O that reads the folder and applies it.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Spotify album art + covers: generous, since a heavy listener browses a lot of artwork.
pub const SPOTIFY_ART_CAP: u64 = 100 * 1024 * 1024;
/// App icons: one small icon per app the phone surfaces, so a tighter cap is plenty.
pub const APP_ICONS_CAP: u64 = 50 * 1024 * 1024;

/// A cached file as the selector sees it: something to identify it by, its size, and when it was
/// last read. Generic over the id so tests can use `&str` while the real caller uses a `PathBuf`.
pub struct CacheFile<T> {
    pub id: T,
    pub size: u64,
    pub accessed: SystemTime,
}

/// Which files to delete to bring the folder to `cap` bytes or under, least-recently-accessed first,
/// returned in deletion order. Returns empty when the total already fits. Ties (equal access time)
/// break by id so the choice is deterministic.
pub fn over_cap<T: Clone + Ord>(files: &[CacheFile<T>], cap: u64) -> Vec<T> {
    let total: u64 = files.iter().map(|f| f.size).sum();
    if total <= cap {
        return Vec::new();
    }
    let mut order: Vec<&CacheFile<T>> = files.iter().collect();
    // Oldest access first; equal times fall back to the id for a stable order.
    order.sort_by(|a, b| a.accessed.cmp(&b.accessed).then_with(|| a.id.cmp(&b.id)));
    let mut remaining = total;
    let mut drop = Vec::new();
    for f in order {
        if remaining <= cap {
            break;
        }
        remaining -= f.size;
        drop.push(f.id.clone());
    }
    drop
}

/// Trim the cached `.img` files in `dir` to `cap` bytes, dropping the least-recently-accessed. Only
/// the image files are weighed and removed; tiny sidecar markers (`.url`, `.none`, …) are left be. A
/// missing or unreadable folder, or a file that won't delete, is ignored: trimming is best-effort.
pub fn trim(dir: &Path, cap: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<CacheFile<PathBuf>> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("img") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        // Access time is the right signal (we want the least-recently-shown art), but Windows can
        // have last-access updates disabled; fall back to modified, then to the epoch.
        let accessed = meta
            .accessed()
            .or_else(|_| meta.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        files.push(CacheFile {
            id: path,
            size: meta.len(),
            accessed,
        });
    }
    for path in over_cap(&files, cap) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn file(id: &str, size: u64, accessed: u64) -> CacheFile<&str> {
        CacheFile {
            id,
            size,
            accessed: at(accessed),
        }
    }

    #[test]
    fn nothing_to_drop_when_under_cap() {
        let files = vec![file("a", 40, 1), file("b", 40, 2)];
        assert!(over_cap(&files, 100).is_empty());
        // Exactly at the cap is fine too.
        assert!(over_cap(&files, 80).is_empty());
    }

    #[test]
    fn drops_oldest_accessed_until_it_fits() {
        // 150 total, cap 100: drop the oldest (c@1, then b@2) until 100 or under remains.
        let files = vec![file("a", 50, 3), file("b", 50, 2), file("c", 50, 1)];
        // Drop c (→100) is enough; stop there.
        assert_eq!(over_cap(&files, 100), vec!["c"]);
        // Tighter cap keeps going into the next-oldest.
        assert_eq!(over_cap(&files, 60), vec!["c", "b"]);
    }

    #[test]
    fn ties_break_deterministically_by_id() {
        let files = vec![file("b", 50, 1), file("a", 50, 1), file("c", 50, 5)];
        // a and b share the oldest time; a sorts first, so it goes first.
        assert_eq!(over_cap(&files, 100), vec!["a"]);
    }

    #[test]
    fn can_empty_a_wildly_oversized_folder() {
        let files = vec![file("a", 200, 1)];
        assert_eq!(over_cap(&files, 100), vec!["a"]);
    }

    #[test]
    fn trim_removes_only_the_oldest_images_and_leaves_markers() {
        let dir = std::env::temp_dir().join(format!("tug-trim-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let big = vec![0u8; 60];
        std::fs::write(dir.join("old.img"), &big).unwrap();
        // Make the second file newer so it survives.
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.join("new.img"), &big).unwrap();
        std::fs::write(dir.join("keep.none"), b"").unwrap();
        // Cap 100 with 120 bytes of images: the older image goes, the newer stays, markers stay.
        trim(&dir, 100);
        assert!(!dir.join("old.img").exists(), "oldest image trimmed");
        assert!(dir.join("new.img").exists(), "newest image kept");
        assert!(dir.join("keep.none").exists(), "sidecar marker left alone");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
