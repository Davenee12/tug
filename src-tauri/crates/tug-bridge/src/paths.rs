//! Where the bridge lives: the pipe name and the token file. Both are per Windows user and per
//! app identifier, so a test build with its own identifier never meets the installed tug.

use std::path::PathBuf;

use sha2::{Digest, Sha256};

/// tug's identifier (`tauri.conf.json`).
pub const DEFAULT_APP_ID: &str = "dev.davejames.tug";
/// Lets a test build of tug (run with another identifier) and its `tug` command find each other.
pub const APP_ID_ENV: &str = "TUG_APP_ID";
pub const TOKEN_FILE: &str = "bridge.token";

/// An identifier is reverse-DNS style: letters, digits, dots and dashes only (it becomes part of
/// a folder name).
pub fn valid_app_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && !id.starts_with('.')
        && !id.contains("..")
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// The identifier a `tug` command should look for.
pub fn client_app_id() -> String {
    std::env::var(APP_ID_ENV)
        .ok()
        .filter(|id| valid_app_id(id))
        .unwrap_or_else(|| DEFAULT_APP_ID.to_string())
}

/// `\\.\pipe\tug-bridge-<hash>`: the hash ties the name to this Windows user (their SID) and the
/// app identifier without putting either in a name other users can list.
pub fn pipe_name(user_sid: &str, app_id: &str) -> String {
    let mut h = Sha256::new();
    h.update(user_sid.as_bytes());
    h.update(b"\n");
    h.update(app_id.as_bytes());
    let digest = h.finalize();
    format!(r"\\.\pipe\tug-bridge-{}", crate::auth::hex(&digest[..12]))
}

/// `%LOCALAPPDATA%\<app id>\bridge.token`: the same folder Tauri gives tug as its local data dir.
pub fn token_path(app_id: &str) -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join(app_id).join(TOKEN_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_names_are_per_user_and_app() {
        let a = pipe_name("S-1-5-21-1-2-3-1001", DEFAULT_APP_ID);
        assert!(a.starts_with(r"\\.\pipe\tug-bridge-"));
        assert_eq!(a.len(), r"\\.\pipe\tug-bridge-".len() + 24);
        assert_eq!(a, pipe_name("S-1-5-21-1-2-3-1001", DEFAULT_APP_ID));
        assert_ne!(a, pipe_name("S-1-5-21-1-2-3-1002", DEFAULT_APP_ID));
        assert_ne!(a, pipe_name("S-1-5-21-1-2-3-1001", "dev.davejames.tug.test"));
        assert!(!a.contains("1001"));
    }

    #[test]
    fn app_ids_are_folder_safe() {
        assert!(valid_app_id(DEFAULT_APP_ID));
        assert!(valid_app_id("dev.davejames.tug-test"));
        for bad in ["", "..\\x", "a/b", ".hidden", "a..b", "a b", "C:"] {
            assert!(!valid_app_id(bad), "{bad}");
        }
    }
}
