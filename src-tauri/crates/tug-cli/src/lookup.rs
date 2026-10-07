//! `tug boat <name>` when `<name>` isn't a file as given: find the file the user most likely
//! meant. First the folder they're in; then Windows Search's index of the PC (the documented
//! Windows.Storage.Search API, indexer only, bounded in time); and if the index can't answer, a
//! small bounded walk of the usual folders (Desktop, Downloads, Documents, Pictures and
//! Pictures\Tugboat, OneDrive). Only a bare file name is looked up this way, only plain files
//! match, nothing leaves the PC, and nothing is offered without the user having named it.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// How many matches to list when a name is ambiguous.
pub const LIST_AT_MOST: usize = 5;
/// How long the index query may take before tug stops waiting for it.
pub const INDEX_TIMEOUT: Duration = Duration::from_secs(3);

/// What a lookup found.
#[derive(Debug, PartialEq, Eq)]
pub enum Found {
    One(PathBuf),
    /// Several files fit, newest first.
    Many(Vec<PathBuf>),
    None,
}

/// Characters a Windows file name can't contain, plus `\0`. A name holding any of them is a path,
/// a wildcard, an alternate data stream (`:`) or junk: never looked up loosely.
const NOT_IN_A_NAME: [char; 10] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|', '\0'];

/// Whether `arg` is just a file name: no folder, drive, UNC prefix, wildcard, quote or `:` (so
/// no alternate data stream either). Anything else is taken exactly as given.
pub fn is_bare_name(arg: &str) -> bool {
    !arg.trim().is_empty() && !arg.contains(NOT_IN_A_NAME) && arg != "." && arg != ".."
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
    if !listed(file) {
        return false;
    }
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

/// The Windows Search (AQS) filter for files whose name starts with `name`, or None for a name
/// that could change the query's meaning. `is_bare_name` already rules out `"`, `*`, `?`, `:`
/// and `\`; anything else inside the quotes is literal text to AQS.
pub fn index_filter(name: &str) -> Option<String> {
    if !is_bare_name(name) || name.chars().any(char::is_control) {
        return None;
    }
    Some(format!("System.FileName:~<\"{}\"", name.trim()))
}

fn modified(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Turn candidate paths into a result: only plain files whose name fits, de-duplicated,
/// newest first.
pub fn decide(name: &str, candidates: Vec<PathBuf>) -> Found {
    let mut hits: Vec<(SystemTime, PathBuf)> = Vec::new();
    for p in candidates {
        let fits = p.file_name().is_some_and(|f| name_fits(name, &f.to_string_lossy()));
        let dup = hits
            .iter()
            .any(|(_, h)| h.as_os_str().eq_ignore_ascii_case(p.as_os_str()));
        if fits && !dup && p.is_file() {
            hits.push((modified(&p), p));
        }
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut paths: Vec<PathBuf> = hits.into_iter().map(|(_, p)| p).collect();
    match paths.len() {
        0 => Found::None,
        1 => Found::One(paths.remove(0)),
        _ => Found::Many(paths),
    }
}

/// The plain files directly in `dir` that fit `name`, newest first.
pub fn matches_in(dir: &Path, name: &str) -> Found {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Found::None;
    };
    let candidates = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .collect();
    decide(name, candidates)
}

/// Bounds for the fallback walk.
#[derive(Debug, Clone, Copy)]
pub struct ScanLimits {
    pub max_depth: usize,
    pub max_entries: usize,
    pub max_time: Duration,
}

pub const SCAN_LIMITS: ScanLimits = ScanLimits {
    max_depth: 4,
    max_entries: 20_000,
    max_time: Duration::from_secs(3),
};

/// A folder the walk goes into: not hidden or a system folder, not a junction or symlink (which
/// could loop or lead anywhere), and not a dot-folder.
fn walkable(entry: &std::fs::DirEntry) -> bool {
    let Ok(kind) = entry.file_type() else { return false };
    if !kind.is_dir() || kind.is_symlink() {
        return false;
    }
    if entry.file_name().to_string_lossy().starts_with('.') {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        const REPARSE_POINT: u32 = 0x400;
        match entry.metadata() {
            Ok(m) => m.file_attributes() & (HIDDEN | SYSTEM | REPARSE_POINT) == 0,
            Err(_) => false,
        }
    }
    #[cfg(not(windows))]
    true
}

/// Walk `roots` breadth-first within `limits`, collecting files whose name fits `name`.
pub fn scan(roots: &[PathBuf], name: &str, limits: ScanLimits) -> Vec<PathBuf> {
    let started = Instant::now();
    let mut seen = 0usize;
    let mut out = Vec::new();
    let mut queue: std::collections::VecDeque<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    while let Some((dir, depth)) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            seen += 1;
            if seen > limits.max_entries || started.elapsed() > limits.max_time {
                return out;
            }
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_file() {
                if name_fits(name, &entry.file_name().to_string_lossy()) {
                    out.push(entry.path());
                }
            } else if depth < limits.max_depth && walkable(&entry) {
                queue.push_back((entry.path(), depth + 1));
            }
        }
    }
    out
}

/// Look `name` up: the current folder, then the index, then the bounded walk.
pub fn find(name: &str, cwd: Option<&Path>) -> Found {
    if !is_bare_name(name) {
        return Found::None;
    }
    if let Some(cwd) = cwd {
        let here = matches_in(cwd, name);
        if here != Found::None {
            return here;
        }
    }
    if let Some(hits) = index_search(name) {
        let found = decide(name, hits);
        if found != Found::None {
            return found;
        }
    }
    // The index is off, still building, doesn't cover the folder, or found nothing.
    decide(name, scan(&scan_roots(), name, SCAN_LIMITS))
}

/// The folders the fallback walk covers: Desktop, Downloads, Documents, Pictures (and
/// Pictures\Tugboat, where Tugboat saves), and OneDrive — the real ones, wherever they're
/// redirected.
#[cfg(windows)]
pub fn scan_roots() -> Vec<PathBuf> {
    use windows::Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Pictures, FOLDERID_SkyDrive,
    };
    let mut roots = Vec::new();
    // Tugboat's folder first: it's the likeliest place for a phone file, and gets the walk's
    // budget before anything else.
    if let Some(pictures) = known_folder(&FOLDERID_Pictures) {
        roots.push(pictures.join("Tugboat"));
    }
    for id in [
        &FOLDERID_Desktop,
        &FOLDERID_Downloads,
        &FOLDERID_Documents,
        &FOLDERID_Pictures,
        &FOLDERID_SkyDrive,
    ] {
        if let Some(p) = known_folder(id) {
            if !roots.contains(&p) {
                roots.push(p);
            }
        }
    }
    roots
}

#[cfg(not(windows))]
pub fn scan_roots() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(windows)]
fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    // SAFETY: the returned string is read once, then freed with CoTaskMemFree as documented.
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let path = p.to_string();
        CoTaskMemFree(Some(p.0 as *const core::ffi::c_void));
        path.ok().map(PathBuf::from)
    }
}

/// Files under the user's profile whose name starts with `name`, from the Windows Search index
/// only (never a slow crawl), newest first, at most 10. None when the index can't answer in
/// time (Windows Search off, or the query failed).
#[cfg(windows)]
pub fn index_search(name: &str) -> Option<Vec<PathBuf>> {
    use windows::Win32::UI::Shell::FOLDERID_Profile;
    let filter = index_filter(name)?;
    let profile = known_folder(&FOLDERID_Profile)?;
    let (tx, rx) = std::sync::mpsc::channel();
    // Its own thread so a stuck query can be abandoned after INDEX_TIMEOUT.
    std::thread::spawn(move || {
        let _ = tx.send(query_index(&profile, &filter).ok());
    });
    rx.recv_timeout(INDEX_TIMEOUT).ok().flatten()
}

#[cfg(windows)]
fn query_index(root: &Path, filter: &str) -> windows::core::Result<Vec<PathBuf>> {
    use windows::core::HSTRING;
    use windows::Storage::Search::{FolderDepth, IndexerOption, QueryOptions, SortEntry};
    use windows::Storage::StorageFolder;
    let folder = StorageFolder::GetFolderFromPathAsync(&HSTRING::from(root.as_os_str()))?.join()?;
    let options = QueryOptions::new()?;
    options.SetFolderDepth(FolderDepth::Deep)?;
    options.SetIndexerOption(IndexerOption::OnlyUseIndexer)?;
    options.SetApplicationSearchFilter(&HSTRING::from(filter))?;
    options.SortOrder()?.Append(&SortEntry {
        PropertyName: HSTRING::from("System.DateModified"),
        AscendingOrder: false,
    })?;
    let files = folder
        .CreateFileQueryWithOptions(&options)?
        .GetFilesAsync(0, 10)?
        .join()?;
    let mut out = Vec::new();
    for i in 0..files.Size()? {
        out.push(PathBuf::from(files.GetAt(i)?.Path()?.to_string()));
    }
    Ok(out)
}

#[cfg(not(windows))]
pub fn index_search(_name: &str) -> Option<Vec<PathBuf>> {
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
        fn file(&self, rel: &str) -> PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
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
            "img?",
            "a\"b",
            "a<b",
            "a|b",
            "..",
            "",
            "  ",
        ] {
            assert!(!is_bare_name(arg), "{arg:?}");
        }
    }

    #[test]
    fn the_index_filter_quotes_a_plain_name_and_refuses_anything_else() {
        assert_eq!(
            index_filter("img_6060").as_deref(),
            Some("System.FileName:~<\"img_6060\"")
        );
        assert_eq!(
            index_filter("IMG 6060 (1).jpeg").as_deref(),
            Some("System.FileName:~<\"IMG 6060 (1).jpeg\"")
        );
        // Nothing that could close the quotes, add a wildcard or name another property.
        for bad in [
            "a\" OR System.Size:>0 \"",
            "img*",
            "System.FileName:x",
            "a\tb",
            r"..\x",
            "",
        ] {
            assert_eq!(index_filter(bad), None, "{bad:?}");
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
            !name_fits("img_6060", "IMG_6060_edited.jpeg"),
            "a prefix from the index isn't a match"
        );
        assert!(
            !name_fits("img_6060", "IMG_6060.jpeg.tugboat"),
            "only the last extension is optional"
        );
        assert!(!name_fits("img", ".img"));
        assert!(
            !name_fits("x.jpg.tugboat.tmp", "x.jpg.tugboat.tmp"),
            "half-written files never match"
        );
    }

    #[test]
    fn the_current_folder_is_looked_in_first() {
        let here = TempDir::new("here");
        let local = here.file("img_6060.png");
        assert_eq!(find("img_6060", Some(&here.0)), Found::One(local));
    }

    #[test]
    fn several_matches_are_listed_newest_first_not_guessed() {
        let dir = TempDir::new("many");
        let old = dir.file("IMG_6060.HEIC");
        let new = dir.file("IMG_6060.jpeg");
        let earlier = SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(earlier)
            .unwrap();
        assert_eq!(matches_in(&dir.0, "img_6060"), Found::Many(vec![new, old]));
    }

    #[test]
    fn the_walk_finds_files_in_subfolders_within_its_bounds() {
        let root = TempDir::new("walk");
        let deep = root.file(r"Pictures\Tugboat\IMG_6060.jpeg");
        root.file(r"Pictures\Tugboat\IMG_6061.jpeg");
        root.file(r"a\b\c\d\e\IMG_6060.png"); // depth 5: past the limit
        let roots = [root.0.clone()];
        let hits = scan(&roots, "img_6060", SCAN_LIMITS);
        assert_eq!(hits, vec![deep.clone()]);
        assert_eq!(decide("img_6060", hits), Found::One(deep));
        // The entry budget stops the walk.
        let tiny = ScanLimits {
            max_entries: 1,
            ..SCAN_LIMITS
        };
        assert!(scan(&roots, "img_6060", tiny).is_empty());
        assert!(scan(&roots, "nothing", SCAN_LIMITS).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn the_walk_skips_hidden_folders_and_dot_folders() {
        let root = TempDir::new("hidden");
        root.file(r".git\IMG_6060.jpeg");
        root.file(r"Secret\IMG_6060.jpeg");
        let status = std::process::Command::new("attrib")
            .arg("+h")
            .arg(root.0.join("Secret"))
            .status()
            .unwrap();
        assert!(status.success());
        assert!(scan(std::slice::from_ref(&root.0), "img_6060", SCAN_LIMITS).is_empty());
    }

    /// Needs Windows Search running with the profile indexed, so it isn't part of the normal run:
    /// `cargo test -p tug-cli -- --ignored`. It checks the query answers (or gives up) in time.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn the_index_query_answers_within_its_time_limit() {
        let started = Instant::now();
        let name = std::env::var("TUG_INDEX_TEST_NAME").unwrap_or_else(|_| "desktop".into());
        let hits = index_search(&name);
        assert!(started.elapsed() < INDEX_TIMEOUT + Duration::from_secs(1));
        eprintln!("index: {hits:?}");
    }

    #[cfg(windows)]
    #[test]
    fn the_walk_covers_the_real_known_folders() {
        let roots = scan_roots();
        assert!(roots.iter().any(|r| r.ends_with("Tugboat")), "{roots:?}");
        assert!(roots.iter().all(|r| r.is_absolute()));
    }
}
