//! The local bridge between the running tug app and companion processes on the same PC: the
//! `tug` command and `tug mcp` (the MCP server AI tools start). It's a Windows named pipe that
//! only the current user can open, never a network port; calls carry proof of a per-install
//! token kept in a user-only file; the protocol is versioned JSON. See docs/DEVELOPERS.md.
//!
//! Pure parts (protocol, framing, auth, times, paths) are unit-tested; `client.rs` also runs the
//! whole exchange over an in-memory stream and over a real pipe.

pub mod auth;
pub mod client;
pub mod framing;
pub mod paths;
pub mod protocol;
pub mod server;
pub mod time;
pub mod token_file;
#[cfg(windows)]
pub mod win;
