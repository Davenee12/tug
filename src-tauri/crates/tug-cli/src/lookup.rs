//! `tug boat <name>` when `<name>` isn't a file as given: find what the user most likely meant,
//! in the folder they're in and then in Pictures\Tugboat (where Tugboat saves what the phone
//! sends). Only a bare file name is looked up this way, only plain files directly in those two
//! folders match, and nothing is ever offered without the user having named it.

use std::path::{Path, PathBuf};

/// How many matches to list when a name is ambiguous.
pub const LIST_AT_MOST: usize = 5;

/// What a lookup found.
#[derive(Debug, PartialEq, Eq)]
pub enum Found {
    One(PathBuf),
    /// Several files fit, sorted by path.
    Many(Vec<PathBuf>),
    None,
}

/// Whether `arg` is just a file name: no folder, drive, UNC prefix or `:` (so no alternate data
/// stream either). Anything else is taken exactly as given.
pub fn is_bare_name(arg: &str) -> bool {
    !arg.is_empty()
        && !arg.contains(['\\', '/', ':', '\0', '*', '?'])
        && arg != "."
        && arg != ".."
        && !arg.trim().is_empty()
}

/// Hidden files, Office lock files, Tugboat's half-written files and desktop.ini never match.
fn listed(name: &str) -> bool {
    !(name.starts_with('.')
        || name.starts_with("~$")
        || name.ends_with(".tugboat.tmp")
        || name.eq_ignore_ascii_case("desktop.ini"))
}

/// Does the file `file` fit the typed `name`? The same name in any case; and when `name` has no
/// extension, a file with that name and any extension (`img_6060` → `IMG_6060.jpeg`).
pub fn name_fits(name: &str, file: &str) -> bool {
    if file.eq_ignore_ascii_case(name) {
        return true;
    }
    if Path::new(name).extension().is_some() {
        return false;
    }
    match file.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => stem.eq_ignore_ascii_case(name),
        _ => false,
    }
}

/// The plain files directly in `dir` that fit `name`, sorted.
pub fn matches_in(dir: &Path, name: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            listed(&file) && name_fits(name, &file)
        })
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

/// Look `name` up in each folder in turn; the first folder with any match decides.
pub fn find(name: &str, dirs: &[PathBuf]) -> Found {
    if !is_bare_name(name) {
        return Found::None;
    }
    for dir in dirs {
        let mut hits = matches_in(dir, name);
        match hits.len() {
            0 => continue,
            1 => return Found::One(hits.remove(0)),
            _ => return Found::Many(hits),
        }
    }
    Found::None
}

/// Pictures\Tugboat, using the real Pictures folder (it may be redirected, e.g. to OneDrive).
#[cfg(windows)]
pub fn tugboat_dir() -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Pictures, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    // SAFETY: the returned string is read once, then freed with CoTaskMemFree as documented.
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Pictures, KF_FLAG_DEFAULT, None).ok()?;
        let path = p.to_string();
        CoTaskMemFree(Some(p.0 as *const core::ffi::c_void));
        path.ok().map(|s| PathBuf::from(s).join("Tugboat"))
    }
}

#[cfg(not(windows))]
pub fn tugboat_dir() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("tug-cli-lookup-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
        fn file(&self, name: &str) -> PathBuf {
            let p = self.0.join(name);
            std::fs::write(&p, b"x").unwrap();
            p
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn only_bare_names_are_looked_up() {
        assert!(is_bare_name("img_6060"));
        assert!(is_bare_name("IMG 6060.jpeg"));
        for arg in [
            r"C:\x\img.jpg",
            r"\\nas\share\img.jpg",
            "img.jpg:secret",
            "sub/img.jpg",
            r"sub\img.jpg",
            "*.jpg",
            "..",
            "",
            "  ",
        ] {
            assert!(!is_bare_name(arg), "{arg:?}");
        }
    }

    #[test]
    fn a_name_fits_in_any_case_and_with_any_extension_when_none_was_given() {
        assert!(name_fits("img_6060", "IMG_6060.jpeg"));
        assert!(name_fits("IMG_6060.JPEG", "IMG_6060.jpeg"));
        assert!(name_fits("notes", "notes"));
        assert!(!name_fits("img_6060.png", "IMG_6060.jpeg"));
        assert!(!name_fits("img_606", "IMG_6060.jpeg"));
        assert!(
            !name_fits("img_6060", "IMG_6060.jpeg.tugboat"),
            "only the last extension is optional"
        );
        assert!(!name_fits("img", ".img"));
    }

    #[test]
    fn finds_the_one_file_meant_in_the_first_folder_that_has_it() {
        let here = TempDir::new("here");
        let boat = TempDir::new("boat");
        let jpeg = boat.file("IMG_6060.jpeg");
        boat.file("IMG_6061.jpeg");
        let dirs = [here.0.clone(), boat.0.clone()];
        // Not in the current folder: Pictures\Tugboat.
        assert_eq!(find("img_6060", &dirs), Found::One(jpeg));
        // The current folder wins when it has a match.
        let local = here.file("img_6060.png");
        assert_eq!(find("img_6060", &dirs), Found::One(local));
        assert_eq!(find("nothing", &dirs), Found::None);
    }

    #[cfg(windows)]
    #[test]
    fn the_tugboat_folder_is_under_the_real_pictures_folder() {
        let dir = tugboat_dir().expect("Windows names a Pictures folder");
        assert!(dir.ends_with("Tugboat"));
        assert!(dir.is_absolute());
    }

    #[test]
    fn several_matches_are_listed_not_guessed() {
        let boat = TempDir::new("many");
        let a = boat.file("IMG_6060.HEIC");
        let b = boat.file("IMG_6060.jpeg");
        assert_eq!(find("img_6060", std::slice::from_ref(&boat.0)), Found::Many(vec![a, b]));
    }

    #[test]
    fn folders_hidden_and_partial_files_never_match() {
        let boat = TempDir::new("skip");
        std::fs::create_dir_all(boat.0.join("IMG_7000.jpeg")).unwrap();
        boat.file(".IMG_7001.jpeg");
        boat.file("IMG_7002.jpeg.tugboat.tmp");
        let dirs = [boat.0.clone()];
        assert_eq!(find("img_7000", &dirs), Found::None);
        assert_eq!(find(".img_7001", &dirs), Found::None);
        assert_eq!(find("IMG_7002.jpeg", &dirs), Found::None);
        // A path is never looked up loosely.
        assert_eq!(find(&format!("{}\\IMG_7000", boat.0.display()), &dirs), Found::None);
    }
}
