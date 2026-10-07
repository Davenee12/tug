//! The token file: created by tug the first time Developer tools are switched on, replaced by
//! "Revoke access", read by the `tug` command and MCP server.

use std::io;
use std::path::Path;

use crate::auth::{new_token, parse_token};

#[derive(Debug)]
pub enum TokenError {
    /// No file: Developer tools have never been switched on.
    Missing,
    /// The file isn't a token (edited or truncated).
    Invalid,
    Io(io::Error),
}

pub fn load(path: &Path) -> Result<String, TokenError> {
    match std::fs::read_to_string(path) {
        Ok(s) => parse_token(&s).ok_or(TokenError::Invalid),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Err(TokenError::Missing),
        Err(e) => Err(TokenError::Io(e)),
    }
}

/// Write a brand-new token (user-only ACL) and return it.
pub fn rotate(path: &Path) -> io::Result<String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let token = new_token();
    write(path, &token)?;
    Ok(token)
}

/// The token in the file, or a new one if there's no usable file yet.
pub fn load_or_create(path: &Path) -> io::Result<String> {
    match load(path) {
        Ok(t) => Ok(t),
        Err(TokenError::Io(e)) => Err(e),
        Err(TokenError::Missing | TokenError::Invalid) => rotate(path),
    }
}

#[cfg(windows)]
fn write(path: &Path, token: &str) -> io::Result<()> {
    crate::win::write_private_file(path, token.as_bytes())
}

#[cfg(not(windows))]
fn write(path: &Path, token: &str) -> io::Result<()> {
    std::fs::write(path, token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_load_rotate() {
        let dir = std::env::temp_dir().join(format!("tug-token-test-{}", std::process::id()));
        let path = dir.join("nested").join("bridge.token");
        assert!(matches!(load(&path), Err(TokenError::Missing)));
        let a = load_or_create(&path).unwrap();
        assert_eq!(load(&path).unwrap(), a);
        assert_eq!(load_or_create(&path).unwrap(), a, "an existing token is kept");
        let b = rotate(&path).unwrap();
        assert_ne!(a, b);
        assert_eq!(load(&path).unwrap(), b);
        std::fs::write(&path, "garbage").unwrap();
        assert!(matches!(load(&path), Err(TokenError::Invalid)));
        let c = load_or_create(&path).unwrap();
        assert_eq!(load(&path).unwrap(), c);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
