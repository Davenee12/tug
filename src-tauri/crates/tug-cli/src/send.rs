//! `tug boat` and the MCP tool `send_file_to_phone`: from what was typed to paths tug can offer.
//! Paths resolve against this program's own folder (the terminal's, or the AI tool's), links are
//! followed, and only regular files pass (`tug_bridge::outgoing`; tug checks again).

use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

use tug_bridge::outgoing::{self, Refusal};
use tug_bridge::protocol::MAX_STDIN_BYTES;

/// `input` as a full path against `cwd`, checked: links followed, folders, devices and pipes
/// refused.
pub fn resolve(input: &str, cwd: &Path) -> Result<String, Refusal> {
    // `join` keeps a full path as it is, and a root-relative one (`\notes.txt`) on cwd's drive.
    outgoing::check(&cwd.join(input)).map(|c| c.path.to_string_lossy().into_owned())
}

/// Every path for an AI tool's call, exactly as given (never looked up by name: an AI tool
/// mustn't be able to go looking through the PC). The first problem, as a sentence.
pub fn resolve_all(inputs: &[String], cwd: &Path) -> Result<Vec<String>, String> {
    inputs
        .iter()
        .map(|i| resolve(i, cwd).map_err(|r| r.sentence(i)))
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
pub enum StdinProblem {
    /// Nothing is piped in: stdin is the terminal itself.
    Terminal,
    Empty,
    TooBig,
    Unreadable,
}

impl StdinProblem {
    pub fn sentence(&self) -> String {
        match self {
            StdinProblem::Terminal => {
                "Pipe something into `tug boat -`, e.g. `type notes.txt | tug boat - --name notes.txt`.".into()
            }
            StdinProblem::Empty => "Nothing came in on stdin, so there's nothing to send.".into(),
            StdinProblem::TooBig => format!(
                "That's more than {} MB to send from stdin. Save it to a file and send that instead.",
                MAX_STDIN_BYTES / (1024 * 1024)
            ),
            StdinProblem::Unreadable => "Couldn't read stdin.".into(),
        }
    }
}

/// Read what's piped in, up to `MAX_STDIN_BYTES` (a byte more says it's too big).
pub fn read_stdin() -> Result<Vec<u8>, StdinProblem> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(StdinProblem::Terminal);
    }
    read_limited(stdin.lock(), MAX_STDIN_BYTES)
}

/// Pure enough to test: read `r` to the end, refusing nothing and more than `max` bytes.
pub fn read_limited(r: impl Read, max: u64) -> Result<Vec<u8>, StdinProblem> {
    let mut bytes = Vec::new();
    r.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| StdinProblem::Unreadable)?;
    if bytes.is_empty() {
        return Err(StdinProblem::Empty);
    }
    if bytes.len() as u64 > max {
        return Err(StdinProblem::TooBig);
    }
    Ok(bytes)
}

/// The command's own copy of stdin, in the user's temp folder, for tug to read once the person
/// clicks Send. Deleted when dropped, whichever way the command ends.
pub struct TempFile(PathBuf);

impl TempFile {
    pub fn save(bytes: &[u8]) -> std::io::Result<TempFile> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("tug-boat-{}-{nanos}.tmp", std::process::id()));
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
        let file = TempFile(path);
        f.write_all(bytes)?;
        f.flush()?;
        Ok(file)
    }

    pub fn path(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_resolve_against_the_working_folder() {
        let dir = std::env::temp_dir().join(format!("tug-cli-send-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("report.pdf"), b"%PDF").unwrap();

        let full = resolve(r"sub\report.pdf", &dir).unwrap();
        assert!(
            full.ends_with(r"sub\report.pdf") || full.ends_with("sub/report.pdf"),
            "{full}"
        );
        assert!(Path::new(&full).is_absolute());
        assert!(!full.starts_with(r"\\?\"), "{full}");
        // A full path is used as given; `..` is fine.
        assert_eq!(resolve(&full, Path::new(r"C:\")).unwrap(), full);
        assert_eq!(resolve(r"sub\..\sub\report.pdf", &dir).unwrap(), full);

        assert_eq!(resolve("sub", &dir), Err(Refusal::Folder));
        assert_eq!(resolve("nope.txt", &dir), Err(Refusal::Missing));
        assert_eq!(resolve("NUL", &dir), Err(Refusal::NotRegular));
        let err = resolve_all(&["sub\\report.pdf".into(), "sub".into()], &dir).unwrap_err();
        assert!(err.contains("sub is a folder"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn stdin_is_read_up_to_the_limit() {
        assert_eq!(read_limited(&b"hello"[..], 10), Ok(b"hello".to_vec()));
        assert_eq!(read_limited(&b""[..], 10), Err(StdinProblem::Empty));
        assert_eq!(read_limited(&b"0123456789A"[..], 10), Err(StdinProblem::TooBig));
        assert_eq!(read_limited(&b"0123456789"[..], 10).unwrap().len(), 10);
        for p in [
            StdinProblem::Terminal,
            StdinProblem::Empty,
            StdinProblem::TooBig,
            StdinProblem::Unreadable,
        ] {
            assert!(p.sentence().ends_with('.'));
        }
    }

    #[test]
    fn the_stdin_copy_goes_when_dropped() {
        let t = TempFile::save(b"piped").unwrap();
        let path = t.path();
        assert_eq!(std::fs::read(&path).unwrap(), b"piped");
        drop(t);
        assert!(!Path::new(&path).exists());
    }
}
