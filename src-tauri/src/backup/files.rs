//! Where imported photos and videos live, and the private scratch folder for decrypted files.
//!
//! - Photos and videos: `<local app data>\attachments\<sha-256 prefix>.<ext>`, named by their
//!   content, so the same photo sent to three people is one file, and a re-import reuses it. The
//!   local (not roaming) app-data folder, because they can add up to gigabytes. JPEG previews
//!   for the window are cached beside them in `thumbs\`.
//! - Scratch: `<local app data>\backup-tmp\<one folder per session>` for the decrypted databases
//!   and each attachment on its way in. Removed when the session ends (done, cancelled or
//!   failed), and the whole folder is swept at every start in case tug didn't get to (a crash or
//!   a power cut). Under the user's own profile, so only they can read it.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

pub const ATTACHMENTS_DIR: &str = "attachments";
const THUMBS_DIR: &str = "thumbs";
pub const TEMP_DIR: &str = "backup-tmp";

/// Extensions a stored file may have (pictures and videos only; see `smsdb::classify`).
const EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "heic", "heif", "webp", "mov", "mp4", "m4v", "3gp",
];

/// A key tug wrote: 32 lowercase hex digits, a dot and an allowed extension. Anything else from
/// the database or the window never reaches a path.
pub fn valid_key(key: &str) -> bool {
    let Some((stem, ext)) = key.split_once('.') else {
        return false;
    };
    stem.len() == 32
        && stem.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && EXTENSIONS.contains(&ext)
}

pub fn attachment_path(local_dir: &Path, key: &str) -> Option<PathBuf> {
    valid_key(key).then(|| local_dir.join(ATTACHMENTS_DIR).join(key))
}

fn thumb_path(local_dir: &Path, key: &str) -> Option<PathBuf> {
    let (stem, _) = key.split_once('.')?;
    valid_key(key).then(|| {
        local_dir
            .join(ATTACHMENTS_DIR)
            .join(THUMBS_DIR)
            .join(format!("{stem}.jpg"))
    })
}

/// Move a file that was just copied out of the backup into the attachments folder under its
/// content key (or drop it, if that photo is already there). Returns the key.
pub fn adopt(local_dir: &Path, temp_file: &Path, sha256: &[u8; 32], ext: &str) -> io::Result<String> {
    let stem: String = sha256.iter().take(16).map(|b| format!("{b:02x}")).collect();
    let key = format!("{stem}.{ext}");
    let dest = attachment_path(local_dir, &key).ok_or_else(|| io::Error::other("not a storable file type"))?;
    if dest.exists() {
        std::fs::remove_file(temp_file)?;
        return Ok(key);
    }
    std::fs::create_dir_all(dest.parent().expect("attachments folder"))?;
    if std::fs::rename(temp_file, &dest).is_err() {
        std::fs::copy(temp_file, &dest)?;
        std::fs::remove_file(temp_file)?;
    }
    Ok(key)
}

/// Delete stored files (and their previews) no row points at any more.
pub fn remove_unreferenced(local_dir: &Path, keep: &HashSet<String>) -> usize {
    let dir = local_dir.join(ATTACHMENTS_DIR);
    let keep_stems: HashSet<&str> = keep.iter().filter_map(|k| k.split_once('.').map(|(s, _)| s)).collect();
    let mut removed = 0;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_file() && valid_key(&name) && !keep.contains(&name) && std::fs::remove_file(e.path()).is_ok()
            {
                removed += 1;
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(dir.join(THUMBS_DIR)) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let stem = name.strip_suffix(".jpg").unwrap_or("");
            if !keep_stems.contains(stem) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    removed
}

/// Everything imported goes (Clear history, or Remove imported history with nothing left).
pub fn remove_all(local_dir: &Path) -> usize {
    let n = remove_unreferenced(local_dir, &HashSet::new());
    let _ = std::fs::remove_dir(local_dir.join(ATTACHMENTS_DIR).join(THUMBS_DIR));
    let _ = std::fs::remove_dir(local_dir.join(ATTACHMENTS_DIR));
    n
}

/// What the window shows for a stored photo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// A JPEG `data:` URI, when there's a picture to show.
    pub data_uri: Option<String>,
    /// A HEIC photo Windows can't open without HEIF Image Extensions.
    pub needs_heif: bool,
    /// A HEIC photo the HEIF extension opened but can't show without HEVC Video Extensions.
    pub needs_hevc: bool,
}

/// Which free Microsoft Store extension a HEIC photo is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingCodec {
    Heif,
    Hevc,
}

fn is_heic(key: &str) -> bool {
    key.ends_with(".heic") || key.ends_with(".heif")
}

/// A preview of a stored photo: the cached JPEG, or one made now (and cached). A video has none.
pub fn preview(local_dir: &Path, key: &str) -> Preview {
    let (Some(src), Some(thumb)) = (attachment_path(local_dir, key), thumb_path(local_dir, key)) else {
        return Preview::default();
    };
    if [".mov", ".mp4", ".m4v", ".3gp"].iter().any(|v| key.ends_with(v)) {
        return Preview::default();
    }
    if let Ok(bytes) = std::fs::read(&thumb) {
        if let Some(uri) = crate::app_icons::data_uri(&bytes) {
            return Preview {
                data_uri: Some(uri),
                ..Preview::default()
            };
        }
    }
    match make_thumbnail(&src) {
        Ok(jpeg) => {
            if let Some(parent) = thumb.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&thumb, &jpeg);
            Preview {
                data_uri: crate::app_icons::data_uri(&jpeg),
                ..Preview::default()
            }
        }
        Err(missing) => no_preview(key, missing),
    }
}

/// No picture: and, for a HEIC photo, which extension would show it.
fn no_preview(key: &str, missing: Option<MissingCodec>) -> Preview {
    let missing = missing.filter(|_| is_heic(key));
    Preview {
        data_uri: None,
        needs_heif: missing == Some(MissingCodec::Heif),
        needs_hevc: missing == Some(MissingCodec::Hevc),
    }
}

/// `Err(Some(..))` when a HEIC photo is missing one of Windows' free extensions.
#[cfg(windows)]
fn make_thumbnail(src: &Path) -> Result<Vec<u8>, Option<MissingCodec>> {
    use super::wic::ThumbError;
    super::wic::jpeg_thumbnail(src, super::wic::MAX_SIDE).map_err(|e| match e {
        ThumbError::NeedsHeif => Some(MissingCodec::Heif),
        ThumbError::NeedsHevc => Some(MissingCodec::Hevc),
        ThumbError::Failed => None,
    })
}

#[cfg(not(windows))]
fn make_thumbnail(_src: &Path) -> Result<Vec<u8>, Option<MissingCodec>> {
    Err(None)
}

/// Open a stored photo or video in its default app (Photos, usually). The path is only ever one
/// tug built from a valid key, so the window can't open anything else.
#[cfg(windows)]
pub fn open(local_dir: &Path, key: &str) -> Result<(), String> {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let path = attachment_path(local_dir, key).ok_or("That photo isn't on this PC.")?;
    if !path.is_file() {
        return Err("That photo isn't on this PC any more.".into());
    }
    // From the &str, not the OsStr: tug's own folder is always valid Unicode, and the OsStr
    // conversion's shared code drags a common-controls-v6-only import into the unit tests.
    let file = HSTRING::from(&*path.to_string_lossy());
    // SAFETY: plain FFI call; every pointer argument outlives the call.
    let result = unsafe { ShellExecuteW(None, w!("open"), &file, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err("Windows couldn't open it.".into())
    }
}

#[cfg(not(windows))]
pub fn open(_local_dir: &Path, _key: &str) -> Result<(), String> {
    Err("Opening photos is only supported on Windows".into())
}

/// One session's private scratch folder, deleted with everything in it when dropped.
#[derive(Debug)]
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn create(local_dir: &Path) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "s{}-{}-{}",
            std::process::id(),
            crate::state::now_ms(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = local_dir.join(TEMP_DIR).join(name);
        std::fs::create_dir_all(&path)?;
        Ok(TempDir(path))
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// A fresh file name inside the folder.
    pub fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// At startup: whatever an earlier run left in the scratch folder (it crashed, or the PC lost
/// power mid-import) goes. Returns whether anything was there.
pub fn sweep_temp(local_dir: &Path) -> bool {
    let dir = local_dir.join(TEMP_DIR);
    if !dir.exists() {
        return false;
    }
    let _ = std::fs::remove_dir_all(&dir);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::discover::tests::temp_dir;

    #[test]
    fn keys_are_checked_before_they_touch_a_path() {
        let good = format!("{}.heic", "0123456789abcdef".repeat(2));
        assert!(valid_key(&good));
        for bad in [
            "../../tug.db".to_string(),
            format!("{}.exe", "0123456789abcdef".repeat(2)),
            format!("{}.HEIC", "0123456789abcdef".repeat(2)),
            format!("{}.jpg", "0123456789ABCDEF".repeat(2)),
            format!("{}.jpg", "0123456789abcdef"),
            "nodot".to_string(),
            format!("{}.jpg.lnk", "0123456789abcdef".repeat(2)),
        ] {
            assert!(!valid_key(&bad), "{bad}");
            assert!(attachment_path(Path::new("L"), &bad).is_none());
        }
    }

    #[test]
    fn the_same_photo_is_stored_once_and_unreferenced_files_go() {
        let local = temp_dir("files");
        let tmp = TempDir::create(&local).unwrap();
        let sha = [0xAB; 32];
        std::fs::write(tmp.file("a"), b"photo").unwrap();
        let key = adopt(&local, &tmp.file("a"), &sha, "jpg").unwrap();
        assert_eq!(key, format!("{}.jpg", "ab".repeat(16)));
        assert!(!tmp.file("a").exists(), "moved, not copied");
        std::fs::write(tmp.file("b"), b"photo").unwrap();
        assert_eq!(adopt(&local, &tmp.file("b"), &sha, "jpg").unwrap(), key);
        assert!(!tmp.file("b").exists(), "the duplicate is dropped");
        assert!(adopt(&local, &tmp.file("c"), &sha, "exe").is_err());
        let other = adopt(
            &local,
            &{
                std::fs::write(tmp.file("d"), b"other").unwrap();
                tmp.file("d")
            },
            &[0xCD; 32],
            "heic",
        )
        .unwrap();
        // A preview beside each.
        let thumbs = local.join(ATTACHMENTS_DIR).join(THUMBS_DIR);
        std::fs::create_dir_all(&thumbs).unwrap();
        std::fs::write(thumbs.join(format!("{}.jpg", "ab".repeat(16))), b"t").unwrap();
        std::fs::write(thumbs.join(format!("{}.jpg", "cd".repeat(16))), b"t").unwrap();

        let keep: HashSet<String> = [key.clone()].into();
        assert_eq!(remove_unreferenced(&local, &keep), 1);
        assert!(attachment_path(&local, &key).unwrap().exists());
        assert!(!attachment_path(&local, &other).unwrap().exists());
        assert!(!thumbs.join(format!("{}.jpg", "cd".repeat(16))).exists());
        assert!(thumbs.join(format!("{}.jpg", "ab".repeat(16))).exists());
        assert_eq!(remove_all(&local), 1);
        assert!(!local.join(ATTACHMENTS_DIR).exists());
        drop(tmp);
        std::fs::remove_dir_all(&local).unwrap();
    }

    #[test]
    fn scratch_folders_go_when_dropped_and_at_the_next_start() {
        let local = temp_dir("scratch");
        let tmp = TempDir::create(&local).unwrap();
        let path = tmp.path().to_path_buf();
        std::fs::write(tmp.file("sms.db"), b"decrypted").unwrap();
        drop(tmp);
        assert!(!path.exists(), "removed with what's in it");
        // A crash leaves one behind (simulated by forgetting it): the next start sweeps it.
        let left = TempDir::create(&local).unwrap();
        std::fs::write(left.file("Manifest.db"), b"decrypted").unwrap();
        let left_path = left.path().to_path_buf();
        std::mem::forget(left);
        assert!(left_path.exists());
        assert!(sweep_temp(&local));
        assert!(!local.join(TEMP_DIR).exists());
        assert!(!sweep_temp(&local), "nothing the second time");
        std::fs::remove_dir_all(&local).unwrap();
    }

    #[test]
    fn the_hint_names_the_missing_extension_for_heic_photos_only() {
        let heic = format!("{}.heic", "ab".repeat(16));
        let jpg = format!("{}.jpg", "ab".repeat(16));
        assert_eq!(
            no_preview(&heic, Some(MissingCodec::Heif)),
            Preview {
                data_uri: None,
                needs_heif: true,
                needs_hevc: false
            }
        );
        assert_eq!(
            no_preview(&heic, Some(MissingCodec::Hevc)),
            Preview {
                data_uri: None,
                needs_heif: false,
                needs_hevc: true
            }
        );
        assert_eq!(no_preview(&heic, None), Preview::default());
        assert_eq!(no_preview(&jpg, Some(MissingCodec::Heif)), Preview::default());
    }

    #[test]
    fn a_video_has_no_preview_and_an_unknown_key_has_nothing() {
        let local = temp_dir("preview");
        assert_eq!(preview(&local, &format!("{}.mov", "ab".repeat(16))), Preview::default());
        assert_eq!(preview(&local, "../x.jpg"), Preview::default());
        std::fs::remove_dir_all(&local).unwrap();
    }
}
