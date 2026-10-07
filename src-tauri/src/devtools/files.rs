//! Files from Tugboat (`Pictures\Tugboat`) for `list_tugboat_files` / `get_tugboat_file`: newest
//! first, only plain files directly in that folder, and a requested name can never reach outside
//! it. Small images are handed over as base64 so an AI tool can look at a phone screenshot.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use base64::Engine;
use tug_bridge::protocol::{valid_file_name, FileContent, FileInfo};
use tug_bridge::time::iso_utc;

/// Images up to this size come back as image content (AI tools cap images around 5 MB, and
/// base64 adds a third).
pub const MAX_IMAGE_BYTES: u64 = 3_500_000;

fn mime_for(name: &str) -> (&'static str, &'static str) {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" => ("image", "image/png"),
        "jpg" | "jpeg" => ("image", "image/jpeg"),
        "gif" => ("image", "image/gif"),
        "webp" => ("image", "image/webp"),
        "heic" => ("image", "image/heic"),
        "heif" => ("image", "image/heif"),
        "mov" => ("video", "video/quicktime"),
        "mp4" | "m4v" => ("video", "video/mp4"),
        "pdf" => ("file", "application/pdf"),
        "txt" | "log" | "md" => ("file", "text/plain"),
        "json" => ("file", "application/json"),
        "zip" => ("file", "application/zip"),
        _ => ("file", "application/octet-stream"),
    }
}

/// Image types AI tools can read directly (HEIC isn't one).
fn viewable_image(mime: &str) -> bool {
    matches!(mime, "image/png" | "image/jpeg" | "image/gif" | "image/webp")
}

/// Tugboat's own half-written files and hidden files aren't results.
fn listed(name: &str) -> bool {
    !(name.starts_with('.')
        || name.starts_with("~$")
        || name.ends_with(".tugboat.tmp")
        || name.eq_ignore_ascii_case("desktop.ini"))
}

fn info(path: &Path, meta: &std::fs::Metadata) -> Option<(i64, FileInfo)> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as i64);
    let (kind, mime) = mime_for(&name);
    Some((
        modified_ms,
        FileInfo {
            path: path.to_string_lossy().into_owned(),
            name,
            size_bytes: meta.len(),
            modified: iso_utc(modified_ms),
            kind: kind.into(),
            mime: mime.into(),
        },
    ))
}

/// The newest files in `folder`, at most `limit`, modified at or after `since_ms` if given.
pub fn list(folder: &Path, limit: usize, since_ms: Option<i64>) -> Vec<FileInfo> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<(i64, FileInfo)> = entries
        .flatten()
        .filter(|e| listed(&e.file_name().to_string_lossy()))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            meta.is_file().then(|| info(&e.path(), &meta)).flatten()
        })
        .filter(|(at, _)| since_ms.is_none_or(|s| *at >= s))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    files.into_iter().take(limit).map(|(_, f)| f).collect()
}

/// The file called `name` in `folder`, only if it really is directly inside it (no `..`, no
/// links pointing elsewhere).
pub fn resolve(folder: &Path, name: &str) -> Result<PathBuf, String> {
    if !valid_file_name(name) || !listed(name) {
        return Err("That isn't a Tugboat file name.".into());
    }
    let not_found =
        || format!("There's no file called \"{name}\" in Tugboat. Use list_tugboat_files to see what's there.");
    let root = std::fs::canonicalize(folder).map_err(|_| not_found())?;
    let path = std::fs::canonicalize(folder.join(name)).map_err(|_| not_found())?;
    if path.parent() != Some(root.as_path()) || !path.is_file() {
        return Err(not_found());
    }
    Ok(path)
}

/// Metadata, plus the image itself when it's small enough and of a kind AI tools can read.
pub fn read(folder: &Path, name: &str) -> Result<FileContent, String> {
    let path = resolve(folder, name)?;
    let meta = std::fs::metadata(&path).map_err(|_| "Couldn't read that file.".to_string())?;
    // Report the path as the folder names it, not its canonical `\\?\` form.
    let (_, file) = info(&folder.join(name), &meta).ok_or("Couldn't read that file.")?;
    let (image_base64, note) = if !viewable_image(&file.mime) {
        let why = if file.kind == "image" {
            "This image type can't be shown here; open it from its path."
        } else {
            "Not an image: use the path to open it."
        };
        (None, Some(why.to_string()))
    } else if file.size_bytes > MAX_IMAGE_BYTES {
        (None, Some("Too big to show here; open it from its path.".to_string()))
    } else {
        let bytes = std::fs::read(&path).map_err(|_| "Couldn't read that file.".to_string())?;
        (Some(base64::engine::general_purpose::STANDARD.encode(bytes)), None)
    };
    Ok(FileContent {
        file,
        image_base64,
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tug-devtools-files-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lists_newest_plain_files_only() {
        let dir = folder("list");
        std::fs::write(dir.join("a.png"), b"x").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join("b.txt"), b"hello").unwrap();
        std::fs::write(dir.join("~$c.png.tugboat.tmp"), b"half").unwrap();
        std::fs::write(dir.join(".hidden"), b"").unwrap();
        std::fs::create_dir(dir.join("sub")).unwrap();
        let l = list(&dir, 10, None);
        let names: Vec<_> = l.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["b.txt", "a.png"]);
        assert_eq!(l[1].kind, "image");
        assert_eq!(l[1].mime, "image/png");
        assert_eq!(l[0].size_bytes, 5);
        assert_eq!(list(&dir, 1, None).len(), 1);
        assert!(list(&dir, 10, Some(i64::MAX)).is_empty());
        assert!(list(&dir.join("missing"), 10, None).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn names_cant_escape_the_folder() {
        let dir = folder("escape");
        let inner = dir.join("Tugboat");
        std::fs::create_dir(&inner).unwrap();
        std::fs::write(dir.join("secret.txt"), b"no").unwrap();
        std::fs::write(inner.join("ok.txt"), b"yes").unwrap();
        assert!(resolve(&inner, "ok.txt").is_ok());
        for bad in [
            "../secret.txt",
            "..\\secret.txt",
            "..",
            "sub/ok.txt",
            "missing.txt",
            "~$x.tugboat.tmp",
        ] {
            assert!(resolve(&inner, bad).is_err(), "{bad}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn small_images_come_back_as_base64() {
        let dir = folder("read");
        std::fs::write(dir.join("shot.png"), [0x89, b'P', b'N', b'G']).unwrap();
        std::fs::write(dir.join("photo.heic"), b"heic").unwrap();
        std::fs::write(dir.join("notes.txt"), b"hi").unwrap();
        let png = read(&dir, "shot.png").unwrap();
        assert_eq!(png.image_base64.as_deref(), Some("iVBORw=="));
        let heic = read(&dir, "photo.heic").unwrap();
        assert!(heic.image_base64.is_none() && heic.note.is_some());
        let txt = read(&dir, "notes.txt").unwrap();
        assert!(txt.image_base64.is_none());
        assert!(txt.file.path.ends_with("notes.txt"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
