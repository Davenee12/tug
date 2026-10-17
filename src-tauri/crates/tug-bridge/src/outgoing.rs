//! Files offered to the phone with `tug boat` or `send_file_to_phone`: which paths may be sent at
//! all. The `tug` command checks first (so a typo gets a sentence and exit code 2 before tug is
//! asked), and the app checks again before it shows its card, with the same rules:
//!
//! - regular files only: no folders, devices (`CON`, `\\.\…`) or pipes;
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

/// Check `path` (already absolute) can be offered: follow links, refuse folders, devices and
/// pipes, and make sure it opens for reading.
pub fn check(path: &Path) -> Result<Checked, Refusal> {
    if is_device_path(&path.to_string_lossy()) {
        return Err(Refusal::NotRegular);
    }
    let real = match std::fs::canonicalize(path) {
        Ok(p) => PathBuf::from(plain(&p.to_string_lossy())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Refusal::Missing),
        Err(_) => return Err(Refusal::Unreadable),
    };
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
    fn refusals_read_as_sentences() {
        for r in [
            Refusal::Missing,
            Refusal::Folder,
            Refusal::NotRegular,
            Refusal::Unreadable,
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
