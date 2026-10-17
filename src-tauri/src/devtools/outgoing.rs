//! `tug boat` and `send_file_to_phone` inside tug: check each file again before the card shows
//! (whatever the caller said), and name it the way the phone will see it. Regular readable files
//! only (no folders, devices or pipes; links followed, the target's name shown), within
//! Tugboat's limits, names through `tugboat::names::sanitize`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tug_bridge::outgoing::{check, Refusal};
use tug_bridge::protocol::{BridgeError, StdinFile, MAX_STDIN_BYTES};

use super::confirm::ConfirmFile;
use crate::tugboat::names::sanitize;
use crate::tugboat::session::{Outgoing, MAX_OFFER};

/// A file that passed, as the card shows it and Tugboat will offer it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picked {
    pub path: PathBuf,
    /// What the phone sees it called.
    pub name: String,
    pub size: u64,
    /// The folder it's in; `None` for what was piped in.
    pub folder: Option<String>,
    /// What came in on stdin: read into memory once the person clicks Send.
    pub stdin: bool,
}

impl Picked {
    pub fn card(&self) -> ConfirmFile {
        ConfirmFile {
            name: self.name.clone(),
            size: self.size,
            folder: self.folder.clone(),
        }
    }
}

const MB: u64 = 1024 * 1024;

/// The sentence for a file that's too big, by its limit.
fn too_big(shown: &str, limit: u64) -> String {
    let shown = tug_bridge::protocol::printable(shown);
    if limit >= 1024 * MB {
        format!(
            "{shown} is over {} GB, too big to send to a phone's browser.",
            limit / (1024 * MB)
        )
    } else {
        format!(
            "{shown} is over {} MB, too big to send from stdin. Save it to a file and send that.",
            limit / MB
        )
    }
}

/// Check one file the caller named: it must still be there, be a regular file, and fit.
pub fn pick(path: &str) -> Result<Picked, BridgeError> {
    pick_within(Path::new(path), path, MAX_OFFER)
}

fn pick_within(path: &Path, shown: &str, limit: u64) -> Result<Picked, BridgeError> {
    let checked = check(path).map_err(|r: Refusal| BridgeError::invalid(r.sentence(shown)))?;
    if checked.size > limit {
        return Err(BridgeError::invalid(too_big(shown, limit)));
    }
    Ok(Picked {
        name: sanitize(&checked.name()),
        size: checked.size,
        folder: checked.folder(),
        path: checked.path,
        stdin: false,
    })
}

/// What came in on stdin: the command's own copy, under the name it was given.
pub fn pick_stdin(s: &StdinFile) -> Result<Picked, BridgeError> {
    let mut p = pick_within(Path::new(&s.path), &s.name, MAX_STDIN_BYTES)?;
    p.name = sanitize(&s.name);
    p.folder = None;
    p.stdin = true;
    Ok(p)
}

/// Every file of a call, in order; the first problem refuses the whole call (the card shows
/// exactly what will be sent, or nothing).
pub fn pick_all(paths: &[String], stdin: Option<&StdinFile>) -> Result<Vec<Picked>, BridgeError> {
    let mut out: Vec<Picked> = paths.iter().map(|p| pick(p)).collect::<Result<_, _>>()?;
    if let Some(s) = stdin {
        out.push(pick_stdin(s)?);
    }
    Ok(out)
}

/// Approved: what Tugboat offers. Piped input is read into memory now (the command deletes its
/// copy once it hears back), and must be what the card showed.
pub fn to_offer(picked: &[Picked]) -> Result<Vec<Outgoing>, BridgeError> {
    picked
        .iter()
        .map(|p| {
            if !p.stdin {
                return Ok(Outgoing::File {
                    path: p.path.clone(),
                    name: p.name.clone(),
                    size: p.size,
                });
            }
            let bytes = std::fs::read(&p.path)
                .ok()
                .filter(|b| b.len() as u64 == p.size)
                .ok_or_else(|| {
                    BridgeError::invalid("What came in on stdin changed before it could be sent. Try again.")
                })?;
            Ok(Outgoing::Memory {
                name: p.name.clone(),
                bytes: Arc::new(bytes),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tug_bridge::protocol::ErrorCode;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tug-devtools-out-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn regular_files_pass_with_their_folder_and_a_safe_name() {
        let d = dir("ok");
        let f = d.join("Holiday.url");
        std::fs::write(&f, b"[InternetShortcut]").unwrap();
        let p = pick(&f.to_string_lossy()).unwrap();
        // Explorer hides .url even with extensions shown: the phone gets it defused.
        assert_eq!(p.name, "Holiday.url.download");
        assert_eq!(p.size, 18);
        // The folder as it really is: links followed and short (8.3) names spelled out, as on a
        // CI runner whose temp folder is under C:\Users\RUNNER~1.
        let real = tug_bridge::outgoing::plain(&std::fs::canonicalize(&d).unwrap().to_string_lossy());
        assert_eq!(p.folder.as_deref(), Some(real.as_str()));
        assert!(!p.stdin);
        assert_eq!(p.card().name, "Holiday.url.download");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn folders_devices_and_missing_files_refuse_the_whole_call() {
        let d = dir("bad");
        let ok = d.join("a.txt");
        std::fs::write(&ok, b"a").unwrap();
        let folder = d.to_string_lossy().into_owned();
        let e = pick_all(&[ok.to_string_lossy().into_owned(), folder.clone()], None).unwrap_err();
        assert_eq!(e.code, ErrorCode::Invalid);
        assert!(e.message.contains("is a folder"), "{e}");
        assert!(pick(r"C:\NUL").unwrap_err().message.contains("isn't a regular file"));
        assert!(pick(&d.join("gone.txt").to_string_lossy())
            .unwrap_err()
            .message
            .contains("no file"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn stdin_is_named_as_asked_read_on_send_and_checked() {
        let d = dir("stdin");
        let copy = d.join("tug-boat-1.tmp");
        std::fs::write(&copy, b"piped text").unwrap();
        let s = StdinFile {
            path: copy.to_string_lossy().into_owned(),
            name: "notes<1>.txt".into(),
        };
        let picked = pick_all(&[], Some(&s)).unwrap();
        assert_eq!(picked[0].name, "notes_1_.txt");
        assert_eq!(
            picked[0].folder, None,
            "the card says it was piped in, not the temp folder"
        );
        assert!(picked[0].stdin);
        match &to_offer(&picked).unwrap()[0] {
            Outgoing::Memory { name, bytes } => {
                assert_eq!(name, "notes_1_.txt");
                assert_eq!(bytes.as_slice(), b"piped text");
            }
            Outgoing::File { .. } => panic!("stdin is held in memory"),
        }
        // Changed after the card: refused rather than sending something else.
        std::fs::write(&copy, b"something else entirely").unwrap();
        assert!(to_offer(&picked).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn too_big_says_the_limit() {
        assert!(too_big("big.mov", MAX_OFFER).contains("over 1 GB"));
        assert!(too_big("log", MAX_STDIN_BYTES).contains("over 64 MB"));
    }
}
