//! Files offered to the phone with `tug boat` or `send_file_to_phone`: which paths may be sent at
//! all. The `tug` command checks first (so a typo gets a sentence and exit code 2 before tug is
//! asked), and the app checks again before it shows its card, with the same rules:
//!
//! - regular files only: no folders, devices (`CON`, `\\.\…`) or pipes;
//! - nothing on another computer: a `\\server\share` path, a mapped network drive, or a link that
//!   leads to one is refused before anything is opened, because opening it would make Windows
//!   sign in to that server with the user's account (an NTLM hash anyone there could collect);
//! - a symbolic link counts as the file it points to, and that file's name is what's shown;
//! - the file must open for reading.
//!
//! The string rules are pure and unit-tested; `check` does the I/O.

use std::path::{Path, PathBuf};

/// Why a path can't be offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Missing,
    Folder,
    /// A device, a pipe, or anything else that isn't a plain file on a disk.
    NotRegular,
    Unreadable,
    /// On another computer (a network share or mapped drive, directly or through a link).
    Network,
}

impl Refusal {
    /// A sentence for people, naming the file as they gave it.
    pub fn sentence(self, shown: &str) -> String {
        let shown = crate::protocol::printable(shown);
        match self {
            Refusal::Missing => format!("There's no file at {shown}."),
            Refusal::Folder => format!("{shown} is a folder. Name the files inside it instead."),
            Refusal::NotRegular => format!("{shown} isn't a regular file, so it can't be sent."),
            Refusal::Unreadable => format!("tug couldn't read {shown}."),
            Refusal::Network => {
                format!("{shown} is on another computer, so tug won't send it. Copy it to this PC first.")
            }
        }
    }
}

/// Windows' device names: `C:\dir\NUL.txt` opens the NUL device, whatever the folder.
const DEVICES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
    "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
    "LPT9", "LPT¹", "LPT²", "LPT³",
];

/// `\\?\C:\x` → `C:\x` and `\\?\UNC\server\share\x` → `\\server\share\x`: the verbatim form
/// `canonicalize` gives, back to the ordinary one people read. Anything else is left alone.
pub fn plain(p: &str) -> String {
    if let Some(rest) = p.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    match p.strip_prefix(r"\\?\") {
        Some(rest) if rest.len() >= 3 && rest.as_bytes()[1] == b':' && rest.as_bytes()[0].is_ascii_alphabetic() => {
            rest.to_string()
        }
        _ => p.to_string(),
    }
}

/// A path that names a device rather than a file: the `\\.\` and `\\?\` namespaces (other than a
/// drive or share, see `plain`), or a last part that's a device name (`NUL`, `com1.txt`).
pub fn is_device_path(p: &str) -> bool {
    let p = plain(p);
    let lower = p.to_ascii_lowercase();
    if lower.starts_with(r"\\.\") || lower.starts_with(r"\\?\") || lower.starts_with(r"\??\") {
        return true;
    }
    let last = p.rsplit(['\\', '/']).next().unwrap_or("");
    let stem = last
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_uppercase();
    DEVICES.iter().any(|d| d.to_uppercase() == stem)
}

/// A path on another computer: `\\server\share\…` (either slash), or its verbatim or NT forms
/// (`\\?\UNC\…`, `\??\UNC\…`, `\\.\UNC\…`). Pure: decided by its spelling alone.
pub fn is_network_path(p: &str) -> bool {
    let p = p.replace('/', "\\").to_ascii_lowercase();
    if [r"\\?\unc\", r"\??\unc\", r"\\.\unc\"].iter().any(|v| p.starts_with(v)) {
        return true;
    }
    p.starts_with(r"\\") && !p.starts_with(r"\\?\") && !p.starts_with(r"\\.\")
}

/// A path on a drive letter Windows says is a network drive (a mapped share). Asking costs no
/// network traffic.
#[cfg(windows)]
pub fn on_network_drive(p: &str) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    const DRIVE_REMOTE: u32 = 4;
    let p = plain(p);
    let b = p.as_bytes();
    if b.len() < 2 || !b[0].is_ascii_alphabetic() || b[1] != b':' {
        return false;
    }
    let root = HSTRING::from(format!("{}:\\", b[0] as char));
    // SAFETY: plain FFI call with a valid, NUL-terminated string.
    unsafe { GetDriveTypeW(&root) == DRIVE_REMOTE }
}

#[cfg(not(windows))]
pub fn on_network_drive(_p: &str) -> bool {
    false
}

fn remote(p: &str) -> bool {
    is_network_path(p) || on_network_drive(p)
}

/// Whether following the links along `path` would lead to another computer. Each link is read
/// where it is (a local read), never followed, and its target checked before the walk goes on, so
/// a link that points at `\\server\share` is caught before Windows would open it. Too many links is
/// treated as leading somewhere it shouldn't.
fn leads_to_network(path: &Path) -> bool {
    use std::path::Component;
    let mut todo: Vec<PathBuf> = path.components().rev().map(|c| PathBuf::from(c.as_os_str())).collect();
    let mut base = PathBuf::new();
    let mut links = 0;
    while let Some(part) = todo.pop() {
        let next = base.join(&part);
        let normal = matches!(part.components().next(), Some(Component::Normal(_)));
        if !normal {
            base = next;
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(&next) else {
            // Not there (or not readable): nothing more to follow; the full check says why.
            return false;
        };
        if !meta.file_type().is_symlink() {
            base = next;
            continue;
        }
        links += 1;
        if links > 32 {
            return true;
        }
        let Ok(target) = std::fs::read_link(&next) else {
            return false;
        };
        if remote(&target.to_string_lossy()) {
            return true;
        }
        // Go on from where the link points: a relative target is against the link's own folder.
        let resolved = base.join(&target);
        base = PathBuf::new();
        todo.extend(resolved.components().rev().map(|c| PathBuf::from(c.as_os_str())));
    }
    false
}

/// A path naming one of a file's alternate data streams (`report.pdf:secret`, `a.txt::$DATA`):
/// any `:` after the drive's own (`C:`). Such a stream is hidden from Explorer and `dir`, so it
/// isn't the file the person sees on the card; never sent.
pub fn names_a_stream(p: &str) -> bool {
    let p = plain(p);
    let b = p.as_bytes();
    let rest = if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        &p[2..]
    } else {
        &p[..]
    };
    rest.contains(':')
}

/// A file that may be offered: where it really is (links followed) and its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub path: PathBuf,
    pub size: u64,
}

impl Checked {
    /// The file's own name: for a link, the name of the file it points to.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// The folder it's in, as people read it.
    pub fn folder(&self) -> Option<String> {
        self.path.parent().map(|d| d.to_string_lossy().into_owned())
    }
}

/// Check `path` (already absolute) can be offered: refuse anything on another computer before
/// any I/O, follow links, refuse folders, devices and pipes, and make sure it opens for reading.
pub fn check(path: &Path) -> Result<Checked, Refusal> {
    let given = path.to_string_lossy();
    // By its spelling first: opening `\\server\…` (even to look) signs in to that server.
    if remote(&given) {
        return Err(Refusal::Network);
    }
    if is_device_path(&given) || names_a_stream(&given) {
        return Err(Refusal::NotRegular);
    }
    if leads_to_network(path) {
        return Err(Refusal::Network);
    }
    let real = match std::fs::canonicalize(path) {
        Ok(p) => PathBuf::from(plain(&p.to_string_lossy())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Refusal::Missing),
        Err(_) => return Err(Refusal::Unreadable),
    };
    // Wherever it really is: never on another computer either.
    if remote(&real.to_string_lossy()) {
        return Err(Refusal::Network);
    }
    if is_device_path(&real.to_string_lossy()) {
        return Err(Refusal::NotRegular);
    }
    let meta = std::fs::metadata(&real).map_err(|_| Refusal::Unreadable)?;
    if meta.is_dir() {
        return Err(Refusal::Folder);
    }
    if !meta.is_file() {
        return Err(Refusal::NotRegular);
    }
    let file = std::fs::File::open(&real).map_err(|_| Refusal::Unreadable)?;
    if !on_disk(&file) {
        return Err(Refusal::NotRegular);
    }
    let size = file.metadata().map_err(|_| Refusal::Unreadable)?.len();
    Ok(Checked { path: real, size })
}

/// The open handle is a file on a disk (not a pipe, console or other character device).
#[cfg(windows)]
fn on_disk(file: &std::fs::File) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{GetFileType, FILE_TYPE_DISK};
    // SAFETY: the handle is open for as long as `file` is borrowed.
    unsafe { GetFileType(HANDLE(file.as_raw_handle())) == FILE_TYPE_DISK }
}

#[cfg(not(windows))]
fn on_disk(_file: &std::fs::File) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_paths_read_plainly() {
        assert_eq!(plain(r"\\?\C:\Users\me\report.pdf"), r"C:\Users\me\report.pdf");
        assert_eq!(plain(r"\\?\UNC\nas\share\a.txt"), r"\\nas\share\a.txt");
        assert_eq!(plain(r"C:\a.txt"), r"C:\a.txt");
        // A volume with no drive letter stays verbatim (and counts as a device path below).
        assert_eq!(plain(r"\\?\Volume{0}\a.txt"), r"\\?\Volume{0}\a.txt");
    }

    #[test]
    fn devices_and_pipes_are_not_files() {
        for bad in [
            r"\\.\pipe\tug-bridge-1",
            r"\\.\PhysicalDrive0",
            r"\\?\GLOBALROOT\Device\Null",
            r"\\?\Volume{0}\a.txt",
            r"C:\temp\NUL",
            r"C:\temp\nul.txt",
            r"C:\temp\COM1.log",
            r"C:\temp\con",
            r"C:\temp\lpt9.tar.gz",
        ] {
            assert!(is_device_path(bad), "{bad}");
        }
        for ok in [
            r"C:\Users\me\report.pdf",
            r"\\?\C:\Users\me\report.pdf",
            r"\\nas\share\console.txt",
            r"C:\temp\nullable.txt",
            r"C:\temp\COM10.txt",
        ] {
            assert!(!is_device_path(ok), "{ok}");
        }
    }

    #[test]
    fn alternate_data_streams_are_never_files_to_send() {
        for stream in [
            r"C:\Users\me\report.pdf:hidden",
            r"C:\Users\me\report.pdf::$DATA",
            r"\\?\C:\Users\me\a.txt:x",
            r"C:a.txt:x",
        ] {
            assert!(names_a_stream(stream), "{stream}");
        }
        for ok in [r"C:\Users\me\report.pdf", r"\\?\C:\a.txt", r"D:\x\y.txt"] {
            assert!(!names_a_stream(ok), "{ok}");
        }
        let dir = std::env::temp_dir().join(format!("tug-outgoing-ads-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("report.pdf");
        std::fs::write(&file, b"%PDF").unwrap();
        let stream = PathBuf::from(format!("{}:hidden", file.display()));
        assert_eq!(check(&stream), Err(Refusal::NotRegular));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn network_paths_are_known_by_their_spelling() {
        for net in [
            r"\\server\share\a.txt",
            "//server/share/a.txt",
            r"\/server\share\a.txt",
            r"\\?\UNC\server\share\a.txt",
            r"\\?\unc\server\share\a.txt",
            r"\??\UNC\server\share\a.txt",
            r"\\.\UNC\server\share\a.txt",
            r"\\192.0.2.7\c$\a.txt",
        ] {
            assert!(is_network_path(net), "{net}");
        }
        for local in [
            r"C:\Users\me\a.txt",
            r"\\?\C:\Users\me\a.txt",
            r"\\.\pipe\x",
            r"\temp\a.txt",
            "a.txt",
        ] {
            assert!(!is_network_path(local), "{local}");
        }
        // The system drive is local.
        assert!(!on_network_drive(r"C:\Windows\win.ini"));
    }

    #[test]
    fn a_network_path_is_refused_before_anything_is_opened() {
        // A server that doesn't exist: had check touched it, the answer would be Missing or
        // Unreadable after a network timeout, not Network at once.
        let started = std::time::Instant::now();
        for p in [
            r"\\tug-test-no-such-server.invalid\share\a.txt",
            r"\\?\UNC\tug-test-no-such-server.invalid\share\a.txt",
        ] {
            assert_eq!(check(Path::new(p)), Err(Refusal::Network), "{p}");
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert!(Refusal::Network.sentence("x").contains("another computer"));
    }

    #[test]
    fn a_link_to_another_computer_is_refused_without_following_it() {
        let dir = std::env::temp_dir().join(format!("tug-outgoing-net-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = r"\\tug-test-no-such-server.invalid\share\a.txt";
        let link = dir.join("innocent.txt");
        let folder_link = dir.join("docs");
        // Making links needs Developer Mode or elevation on Windows; skipped when it isn't allowed.
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(target, &link).is_ok()
            && std::os::windows::fs::symlink_dir(r"\\tug-test-no-such-server.invalid\share", &folder_link).is_ok();
        #[cfg(not(windows))]
        let made = false;
        if made {
            let started = std::time::Instant::now();
            assert_eq!(check(&link), Err(Refusal::Network));
            assert_eq!(check(&folder_link.join("a.txt")), Err(Refusal::Network));
            assert!(started.elapsed() < std::time::Duration::from_secs(1));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn refusals_read_as_sentences() {
        for r in [
            Refusal::Missing,
            Refusal::Folder,
            Refusal::NotRegular,
            Refusal::Unreadable,
            Refusal::Network,
        ] {
            let s = r.sentence("notes\u{1b}[2J.txt");
            assert!(s.ends_with('.'), "{s}");
            assert!(!s.contains('\u{1b}'), "{s}");
        }
        assert!(Refusal::Folder.sentence("src").contains("is a folder"));
    }

    #[test]
    fn checks_real_files_folders_and_links() {
        let dir = std::env::temp_dir().join(format!("tug-outgoing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("report.pdf");
        std::fs::write(&file, b"%PDF-1.7").unwrap();

        let ok = check(&file).unwrap();
        assert_eq!(ok.size, 8);
        assert_eq!(ok.name(), "report.pdf");
        assert!(!ok.path.to_string_lossy().starts_with(r"\\?\"), "{:?}", ok.path);

        assert_eq!(check(&dir), Err(Refusal::Folder));
        assert_eq!(check(&dir.join("missing.txt")), Err(Refusal::Missing));
        assert_eq!(check(Path::new(r"C:\NUL")), Err(Refusal::NotRegular));

        // A link counts as the file it points to, under that file's name. (Making one needs
        // Developer Mode or elevation on Windows; skipped when it isn't allowed.)
        let link = dir.join("latest.pdf");
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&file, &link).is_ok();
        #[cfg(not(windows))]
        let made = std::os::unix::fs::symlink(&file, &link).is_ok();
        if made {
            let via = check(&link).unwrap();
            assert_eq!(via.name(), "report.pdf");
            assert_eq!(via.size, 8);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
