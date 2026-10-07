//! The bridge's versioned JSON protocol: one newline-delimited JSON message per line, one call
//! per connection.
//!
//! ```text
//! client → {"type":"hello","v":1,"nonce":"<16 random bytes, hex>","client":{"name":"claude-code","kind":"mcp"}}
//! server → {"type":"challenge","v":1,"nonce":"<hex>","proof":"<HMAC proving tug knows the token>"}
//!          or {"type":"error","code":"off"|"version",...} and the connection closes
//! client → {"type":"call","proof":"<HMAC proving the client knows the token>","call":{"tool":"search_messages",...}}
//! server → {"type":"result","ok":{...}}  or  {"type":"error","code":"...","message":"..."}
//! ```
//!
//! The token never crosses the pipe, and both sides prove they hold it (see `auth.rs`), so a
//! process squatting the pipe name while tug is closed learns nothing. Pure: no I/O here.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::time::parse_since;

pub const PROTOCOL_VERSION: u32 = 1;
/// Longest line a client may send (a call is small; `send_text` is the biggest).
pub const MAX_CLIENT_LINE: usize = 64 * 1024;
/// Longest line the server may send (an image from Tugboat, base64, plus metadata).
pub const MAX_SERVER_LINE: usize = 8 * 1024 * 1024;

/// Limits callers can ask for, and what they get by default.
pub const DEFAULT_LIMIT: u32 = 20;
pub const MAX_LIMIT: u32 = 50;
pub const MAX_QUERY_CHARS: usize = 200;
pub const MAX_NAME_CHARS: usize = 100;
pub const MAX_TEXT_CHARS: usize = 1000;
pub const MAX_OFFER_FILES: usize = 20;
/// Developer notifications look back this far when no `since` is given.
pub const DEFAULT_DEV_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientKind {
    /// An AI tool, through `tug mcp`.
    Mcp,
    /// The `tug` command in a terminal.
    Cli,
}

/// Who's calling. The name is what the MCP client calls itself (`claude-code`, `cursor`), so
/// it's informational: anything running as the user could claim any name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub kind: ClientKind,
}

/// Characters that change how text around them is shown without being seen themselves:
/// zero-width spaces and joiners, and bidirectional overrides/isolates (which can make a name
/// read differently from what it is).
pub fn is_invisible_format(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

/// Text from somewhere else, made safe to print in a terminal or log line: no control
/// characters (no escape sequences, no line breaks) and no invisible formatting characters.
pub fn printable(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() && !is_invisible_format(*c))
        .collect()
}

/// MCP clients' names as they introduce themselves, and the names people know them by.
const KNOWN_CLIENTS: &[(&str, &str)] = &[
    ("claude-code", "Claude Code"),
    ("claude-ai", "Claude"),
    ("claude-desktop", "Claude"),
    ("cursor", "Cursor"),
    ("cursor-vscode", "Cursor"),
    ("vscode", "VS Code"),
    ("visual-studio-code", "VS Code"),
    ("windsurf", "Windsurf"),
    ("windsurf-client", "Windsurf"),
    ("codex", "Codex"),
    ("codex-mcp-client", "Codex"),
    ("gemini-cli", "Gemini CLI"),
    ("gemini-cli-mcp-client", "Gemini CLI"),
    ("zed", "Zed"),
    ("cline", "Cline"),
    ("goose", "Goose"),
    ("continue", "Continue"),
    ("mcp-inspector", "MCP Inspector"),
];

/// An MCP client's name as people know it: a known tool by its product name ("claude-code" →
/// "Claude Code"), another identifier-style name in words ("my-agent" → "My Agent"), and
/// anything already written for people ("Claude Code", "My Tool v2") as it is.
fn friendly_client_name(name: &str) -> String {
    let key = name.to_ascii_lowercase();
    if let Some((_, known)) = KNOWN_CLIENTS.iter().find(|(id, _)| *id == key) {
        return (*known).to_string();
    }
    let slug = name.chars().any(|c| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'));
    if !slug {
        return name.to_string();
    }
    name.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars
                .next()
                .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl ClientInfo {
    /// A name safe to show and log: printable, single line, at most 40 characters. An AI tool's
    /// name reads the way people know it ("claude-code" → "Claude Code").
    pub fn display_name(&self) -> String {
        let printable: String = self
            .name
            .chars()
            .filter(|c| !c.is_control() && !is_invisible_format(*c))
            .take(200)
            .collect();
        let named = match self.kind {
            ClientKind::Mcp => friendly_client_name(printable.trim()),
            ClientKind::Cli => printable,
        };
        let clean = named.chars().take(40).collect::<String>().trim().to_string();
        if clean.is_empty() {
            match self.kind {
                ClientKind::Mcp => "An AI tool".into(),
                ClientKind::Cli => "tug command".into(),
            }
        } else {
            clean
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Hello { v: u32, nonce: String, client: ClientInfo },
    Call { proof: String, call: Call },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Challenge { v: u32, nonce: String, proof: String },
    Result { ok: serde_json::Value },
    Error(BridgeError),
}

/// Why a call didn't work. `message` is written for people: the CLI prints it and AI tools
/// pass it on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Settings › Developer tools › "Let AI tools use tug" is off.
    Off,
    /// The proof didn't check out: the token changed (Revoke access) or isn't the right one.
    Unauthorized,
    /// Client and app speak different protocol versions.
    Version,
    /// This tool is switched off in Settings › Developer tools.
    ToolOff,
    RateLimited,
    /// Bad arguments.
    Invalid,
    NotFound,
    /// The phone (or the part of it this needs) isn't connected.
    Unavailable,
    /// Another confirmation is already waiting.
    Busy,
    Internal,
}

impl BridgeError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        BridgeError {
            code,
            message: message.into(),
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Invalid, message)
    }
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BridgeError {}

/// A call as it travels. Arguments are checked by `Call::validate` before anything runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum Call {
    GetLatestCode {
        /// Also put it on the clipboard (`tug code --copy`).
        #[serde(default)]
        copy: bool,
    },
    SearchMessages {
        query: String,
        #[serde(default)]
        limit: Option<u32>,
        #[serde(default)]
        since: Option<String>,
    },
    RecentDevNotifications {
        #[serde(default)]
        since: Option<String>,
        #[serde(default)]
        limit: Option<u32>,
    },
    ListTugboatFiles {
        #[serde(default)]
        limit: Option<u32>,
        #[serde(default)]
        since: Option<String>,
    },
    GetTugboatFile {
        name: String,
    },
    PhoneStatus,
    MediaControl {
        action: String,
    },
    SendText {
        to: String,
        message: String,
    },
    /// `tug boat <file...>`: offer files to the phone through Tugboat.
    OfferFiles {
        paths: Vec<String>,
    },
    /// `tug status`: is tug running, what's on, and the phone.
    Status,
}

/// The switches in Settings › Developer tools. One switch can cover several tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Codes,
    Search,
    DevNotifications,
    TugboatFiles,
    PhoneStatus,
    Media,
    SendText,
    TugboatSend,
}

impl Permission {
    pub const ALL: [Permission; 8] = [
        Permission::Codes,
        Permission::Search,
        Permission::DevNotifications,
        Permission::TugboatFiles,
        Permission::PhoneStatus,
        Permission::Media,
        Permission::SendText,
        Permission::TugboatSend,
    ];

    /// On once the master switch is on, unless the user turned it off. Reading is on; anything
    /// that acts on the phone (music, texts) starts off, and so do verification codes: a code is
    /// a key to an account, so handing it to an AI tool is something to choose, not a default.
    pub fn default_on(self) -> bool {
        !matches!(self, Permission::Media | Permission::SendText | Permission::Codes)
    }

    pub fn key(self) -> &'static str {
        match self {
            Permission::Codes => "codes",
            Permission::Search => "search",
            Permission::DevNotifications => "dev_notifications",
            Permission::TugboatFiles => "tugboat_files",
            Permission::PhoneStatus => "phone_status",
            Permission::Media => "media",
            Permission::SendText => "send_text",
            Permission::TugboatSend => "tugboat_send",
        }
    }

    pub fn from_key(key: &str) -> Option<Permission> {
        Permission::ALL.into_iter().find(|p| p.key() == key)
    }

    /// The name the Settings switch and error messages use.
    pub fn label(self) -> &'static str {
        match self {
            Permission::Codes => "Verification codes",
            Permission::Search => "Search texts and notifications",
            Permission::DevNotifications => "Developer notifications",
            Permission::TugboatFiles => "Files from Tugboat",
            Permission::PhoneStatus => "Phone status",
            Permission::Media => "Music controls",
            Permission::SendText => "Send texts",
            Permission::TugboatSend => "Send files to your phone",
        }
    }
}

/// Which switches are on, given what's saved (`None` = never touched, so the default).
pub fn effective_permissions(saved: &BTreeMap<String, bool>) -> BTreeMap<Permission, bool> {
    Permission::ALL
        .into_iter()
        .map(|p| (p, saved.get(p.key()).copied().unwrap_or_else(|| p.default_on())))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaAction {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
}

impl MediaAction {
    pub fn parse(s: &str) -> Option<MediaAction> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "play" => MediaAction::Play,
            "pause" => MediaAction::Pause,
            "toggle" | "play_pause" | "playpause" => MediaAction::Toggle,
            "next" | "skip" => MediaAction::Next,
            "previous" | "prev" | "back" => MediaAction::Previous,
            _ => return None,
        })
    }
}

/// A call whose arguments have been checked and normalized.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    LatestCode {
        copy: bool,
    },
    Search {
        query: String,
        limit: u32,
        since_ms: Option<i64>,
    },
    DevNotifications {
        since_ms: i64,
        limit: u32,
    },
    ListFiles {
        limit: u32,
        since_ms: Option<i64>,
    },
    GetFile {
        name: String,
    },
    PhoneStatus,
    Media(MediaAction),
    SendText {
        to: String,
        message: String,
    },
    OfferFiles {
        paths: Vec<String>,
    },
    Status,
}

impl Request {
    /// The tool name, for logs and rate limits.
    pub fn tool(&self) -> &'static str {
        match self {
            Request::LatestCode { .. } => "get_latest_code",
            Request::Search { .. } => "search_messages",
            Request::DevNotifications { .. } => "recent_dev_notifications",
            Request::ListFiles { .. } => "list_tugboat_files",
            Request::GetFile { .. } => "get_tugboat_file",
            Request::PhoneStatus => "phone_status",
            Request::Media(_) => "media_control",
            Request::SendText { .. } => "send_text",
            Request::OfferFiles { .. } => "offer_files",
            Request::Status => "status",
        }
    }

    /// The switch that has to be on for this call. `Status` only needs the master switch.
    pub fn permission(&self) -> Option<Permission> {
        Some(match self {
            Request::LatestCode { .. } => Permission::Codes,
            Request::Search { .. } => Permission::Search,
            Request::DevNotifications { .. } => Permission::DevNotifications,
            Request::ListFiles { .. } | Request::GetFile { .. } => Permission::TugboatFiles,
            Request::PhoneStatus => Permission::PhoneStatus,
            Request::Media(_) => Permission::Media,
            Request::SendText { .. } => Permission::SendText,
            Request::OfferFiles { .. } => Permission::TugboatSend,
            Request::Status => return None,
        })
    }
}

fn limit(l: Option<u32>) -> Result<u32, BridgeError> {
    match l {
        None => Ok(DEFAULT_LIMIT),
        Some(0) => Err(BridgeError::invalid("`limit` must be at least 1")),
        Some(n) => Ok(n.min(MAX_LIMIT)),
    }
}

fn since(s: Option<&str>, now_ms: i64) -> Result<Option<i64>, BridgeError> {
    match s.map(str::trim) {
        None | Some("") => Ok(None),
        Some(s) => parse_since(s, now_ms).map(Some).map_err(BridgeError::invalid),
    }
}

/// Free text from a caller: trimmed, at most `max` characters, no control characters other
/// than newlines (and only where `multiline`).
fn text(field: &str, s: &str, max: usize, multiline: bool) -> Result<String, BridgeError> {
    let s = s.trim();
    if s.is_empty() {
        return Err(BridgeError::invalid(format!("`{field}` can't be empty")));
    }
    if s.chars().count() > max {
        return Err(BridgeError::invalid(format!(
            "`{field}` is longer than {max} characters"
        )));
    }
    if s.chars()
        .any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\r' | '\t')))
    {
        return Err(BridgeError::invalid(format!(
            "`{field}` has characters that can't be sent"
        )));
    }
    Ok(s.replace("\r\n", "\n"))
}

/// A file name in the Tugboat folder: a bare name, never a path.
pub fn valid_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '\0'])
        && !name.chars().any(char::is_control)
}

impl Call {
    pub fn validate(&self, now_ms: i64) -> Result<Request, BridgeError> {
        Ok(match self {
            Call::GetLatestCode { copy } => Request::LatestCode { copy: *copy },
            Call::SearchMessages {
                query,
                limit: l,
                since: s,
            } => Request::Search {
                query: text("query", query, MAX_QUERY_CHARS, false)?,
                limit: limit(*l)?,
                since_ms: since(s.as_deref(), now_ms)?,
            },
            Call::RecentDevNotifications { since: s, limit: l } => Request::DevNotifications {
                since_ms: since(s.as_deref(), now_ms)?.unwrap_or(now_ms - DEFAULT_DEV_WINDOW_MS),
                limit: limit(*l)?,
            },
            Call::ListTugboatFiles { limit: l, since: s } => Request::ListFiles {
                limit: limit(*l)?,
                since_ms: since(s.as_deref(), now_ms)?,
            },
            Call::GetTugboatFile { name } => {
                let name = name.trim();
                if !valid_file_name(name) {
                    return Err(BridgeError::invalid(
                        "`name` must be a file name from list_tugboat_files, not a path",
                    ));
                }
                Request::GetFile { name: name.to_string() }
            }
            Call::PhoneStatus => Request::PhoneStatus,
            Call::MediaControl { action } => Request::Media(
                MediaAction::parse(action)
                    .ok_or_else(|| BridgeError::invalid("`action` must be play, pause, toggle, next or previous"))?,
            ),
            Call::SendText { to, message } => Request::SendText {
                to: text("to", to, MAX_NAME_CHARS, false)?,
                message: text("message", message, MAX_TEXT_CHARS, true)?,
            },
            Call::OfferFiles { paths } => {
                if paths.is_empty() {
                    return Err(BridgeError::invalid("Name at least one file to send."));
                }
                if paths.len() > MAX_OFFER_FILES {
                    return Err(BridgeError::invalid(format!(
                        "Send at most {MAX_OFFER_FILES} files at a time."
                    )));
                }
                for p in paths {
                    if p.is_empty() || p.contains('\0') || !is_absolute_windows_path(p) {
                        return Err(BridgeError::invalid("Files must be given as full paths."));
                    }
                }
                Request::OfferFiles { paths: paths.clone() }
            }
            Call::Status => Request::Status,
        })
    }
}

/// `C:\dir\file` or `\\server\share\file`: the CLI resolves relative paths against its own
/// folder before sending, so the app never guesses what a relative path meant.
pub fn is_absolute_windows_path(p: &str) -> bool {
    let b = p.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
        || p.starts_with("\\\\")
}

// ---- Results. The app serializes these; the CLI and MCP server read them back. ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeResult {
    pub code: String,
    /// Who sent it: the contact or the app's notification title.
    pub from: String,
    /// "Messages", or the app the notification came from.
    pub app: String,
    pub received_at: String,
    pub age_seconds: i64,
    /// `copy` was asked for and it's on the clipboard.
    #[serde(default)]
    pub copied: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitKind {
    Text,
    Notification,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageHit {
    pub kind: HitKind,
    /// The sender (or, for a text you sent, "You").
    pub from: String,
    pub app: String,
    pub time: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    pub modified: String,
    /// "image", "video" or "file".
    pub kind: String,
    pub mime: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileContent {
    pub file: FileInfo,
    /// Base64 of an image small enough to hand to an AI tool; otherwise only the path.
    #[serde(default)]
    pub image_base64: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NowPlayingInfo {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// "playing", "paused" or "stopped".
    pub state: String,
    pub app: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhoneStatus {
    pub connected: bool,
    /// "connected", "connecting", "disconnected" or "no_phone".
    pub connection: String,
    /// "iPhone 15 Pro Max", when the phone has said.
    pub model: Option<String>,
    pub battery_percent: Option<u8>,
    pub notifications: bool,
    pub texts: bool,
    pub now_playing: Option<NowPlayingInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusResult {
    pub version: String,
    pub permissions: BTreeMap<Permission, bool>,
    pub phone: PhoneStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendOutcome {
    Sent,
    /// The person clicked "Don't send".
    Declined,
    /// Nobody answered the confirmation within 2 minutes.
    TimedOut,
    /// Turned off, revoked or tug quit while it was waiting.
    Cancelled,
    /// Approved, but the phone didn't take it.
    Failed,
    /// Approved and saved in tug, but the phone hadn't answered when the call stopped waiting:
    /// it may still send. Never to be sent again by the caller.
    Queued,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SendResult {
    pub outcome: SendOutcome,
    /// The contact it went (or would have gone) to.
    pub to: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkippedFile {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfferResult {
    pub offered: usize,
    pub skipped: Vec<SkippedFile>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_791_365_400_000;

    fn parse(json: &str) -> Call {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn calls_round_trip_on_the_wire() {
        let calls = vec![
            Call::GetLatestCode { copy: true },
            Call::SearchMessages {
                query: "dinner".into(),
                limit: Some(5),
                since: Some("2h".into()),
            },
            Call::PhoneStatus,
            Call::SendText {
                to: "Sam".into(),
                message: "On my way".into(),
            },
        ];
        for c in calls {
            let wire = serde_json::to_string(&ClientMsg::Call {
                proof: "p".into(),
                call: c.clone(),
            })
            .unwrap();
            assert_eq!(
                serde_json::from_str::<ClientMsg>(&wire).unwrap(),
                ClientMsg::Call {
                    proof: "p".into(),
                    call: c
                }
            );
        }
        assert_eq!(
            serde_json::to_value(Call::PhoneStatus).unwrap(),
            serde_json::json!({"tool": "phone_status"})
        );
    }

    #[test]
    fn optional_arguments_can_be_left_out() {
        assert_eq!(
            parse(r#"{"tool":"get_latest_code"}"#),
            Call::GetLatestCode { copy: false }
        );
        assert_eq!(
            parse(r#"{"tool":"recent_dev_notifications"}"#).validate(NOW),
            Ok(Request::DevNotifications {
                since_ms: NOW - DEFAULT_DEV_WINDOW_MS,
                limit: DEFAULT_LIMIT
            })
        );
        assert!(serde_json::from_str::<Call>(r#"{"tool":"rm_rf"}"#).is_err());
        assert!(serde_json::from_str::<Call>(r#"{"tool":"search_messages"}"#).is_err());
    }

    #[test]
    fn errors_serialize_flat() {
        let e = ServerMsg::Error(BridgeError::new(ErrorCode::ToolOff, "off"));
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({"type": "error", "code": "tool_off", "message": "off"})
        );
        let back: ServerMsg = serde_json::from_value(serde_json::to_value(&e).unwrap()).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn search_arguments_are_checked() {
        let ok = Call::SearchMessages {
            query: "  dentist ".into(),
            limit: Some(500),
            since: Some("1d".into()),
        };
        assert_eq!(
            ok.validate(NOW),
            Ok(Request::Search {
                query: "dentist".into(),
                limit: MAX_LIMIT,
                since_ms: Some(NOW - 86_400_000)
            })
        );
        let bad = |query: &str, limit: Option<u32>, since: Option<&str>| {
            Call::SearchMessages {
                query: query.into(),
                limit,
                since: since.map(Into::into),
            }
            .validate(NOW)
            .unwrap_err()
            .code
        };
        assert_eq!(bad("   ", None, None), ErrorCode::Invalid);
        assert_eq!(bad(&"x".repeat(201), None, None), ErrorCode::Invalid);
        assert_eq!(bad("a\u{7}b", None, None), ErrorCode::Invalid);
        assert_eq!(bad("ok", Some(0), None), ErrorCode::Invalid);
        assert_eq!(bad("ok", None, Some("someday")), ErrorCode::Invalid);
    }

    #[test]
    fn send_text_arguments_are_checked() {
        let v = |to: &str, msg: &str| {
            Call::SendText {
                to: to.into(),
                message: msg.into(),
            }
            .validate(NOW)
        };
        assert_eq!(
            v(" Sam ", "line one\r\nline two"),
            Ok(Request::SendText {
                to: "Sam".into(),
                message: "line one\nline two".into()
            })
        );
        assert!(v("", "hi").is_err());
        assert!(v("Sam", "").is_err());
        assert!(v("Sam\nBob", "hi").is_err());
        assert!(v("Sam", &"y".repeat(MAX_TEXT_CHARS + 1)).is_err());
        assert!(v("Sam", "bell\u{7}").is_err());
    }

    #[test]
    fn file_names_are_never_paths() {
        let get = |n: &str| Call::GetTugboatFile { name: n.into() }.validate(NOW);
        assert_eq!(
            get("IMG_0001.HEIC"),
            Ok(Request::GetFile {
                name: "IMG_0001.HEIC".into()
            })
        );
        for bad in [
            "",
            "..",
            "../secret.txt",
            "C:\\Windows\\win.ini",
            "a/b.png",
            "x:y",
            "nul\0",
        ] {
            assert!(get(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn media_actions() {
        let m = |a: &str| Call::MediaControl { action: a.into() }.validate(NOW);
        assert_eq!(m("Play"), Ok(Request::Media(MediaAction::Play)));
        assert_eq!(m("next"), Ok(Request::Media(MediaAction::Next)));
        assert_eq!(m("prev"), Ok(Request::Media(MediaAction::Previous)));
        assert_eq!(m("toggle"), Ok(Request::Media(MediaAction::Toggle)));
        assert!(m("volume_up").is_err());
    }

    #[test]
    fn offered_files_must_be_full_paths() {
        let o = |p: &[&str]| {
            Call::OfferFiles {
                paths: p.iter().map(|s| s.to_string()).collect(),
            }
            .validate(NOW)
        };
        assert!(o(&["C:\\Users\\me\\shot.png", "\\\\nas\\share\\a.txt"]).is_ok());
        assert!(o(&[]).is_err());
        assert!(o(&["shot.png"]).is_err());
        assert!(o(&["..\\shot.png"]).is_err());
        let many: Vec<String> = (0..21).map(|i| format!("C:\\f{i}")).collect();
        assert!(Call::OfferFiles { paths: many }.validate(NOW).is_err());
    }

    #[test]
    fn permissions_default_to_read_on_act_off() {
        let eff = effective_permissions(&BTreeMap::new());
        assert!(eff[&Permission::Search] && eff[&Permission::PhoneStatus] && eff[&Permission::TugboatFiles]);
        assert!(!eff[&Permission::Media] && !eff[&Permission::SendText] && !eff[&Permission::Codes]);
        let saved = BTreeMap::from([("media".to_string(), true), ("search".to_string(), false)]);
        let eff = effective_permissions(&saved);
        assert!(eff[&Permission::Media] && !eff[&Permission::Search]);
        for p in Permission::ALL {
            assert_eq!(Permission::from_key(p.key()), Some(p));
        }
    }

    #[test]
    fn every_call_needs_its_switch() {
        assert_eq!(Request::Status.permission(), None);
        assert_eq!(
            Request::GetFile { name: "a".into() }.permission(),
            Some(Permission::TugboatFiles)
        );
        assert_eq!(
            Request::SendText {
                to: "a".into(),
                message: "b".into()
            }
            .permission(),
            Some(Permission::SendText)
        );
    }

    #[test]
    fn client_names_are_tamed_for_display() {
        let c = |name: &str| ClientInfo {
            name: name.into(),
            kind: ClientKind::Mcp,
        };
        assert_eq!(c("claude-code").display_name(), "Claude Code");
        assert_eq!(c("evil\nname\u{1b}[31m").display_name(), "evilname[31m");
        assert_eq!(c("").display_name(), "An AI tool");
        assert_eq!(c(" \u{200B} ").display_name(), "An AI tool");
        // Bidi overrides and zero-width characters go before the name is read or prettified.
        assert_eq!(c("claude\u{202E}-code\u{200B}").display_name(), "Claude Code");
        assert_eq!(c("claude\u{202E}edoc\u{200B}").display_name(), "Claudeedoc");
        assert_eq!(printable("a\u{1b}[2Jb\nc\u{2066}d"), "a[2Jbcd");
        assert_eq!(c(&"a".repeat(100)).display_name().chars().count(), 40);
        assert_eq!(c(&"Ab".repeat(100)).display_name().chars().count(), 40);
    }

    #[test]
    fn ai_tools_are_named_the_way_people_know_them() {
        let c = |name: &str| {
            ClientInfo {
                name: name.into(),
                kind: ClientKind::Mcp,
            }
            .display_name()
        };
        assert_eq!(c("claude-code"), "Claude Code");
        assert_eq!(c("Claude-Code"), "Claude Code");
        assert_eq!(c("cursor-vscode"), "Cursor");
        assert_eq!(c("my-agent_2"), "My Agent 2");
        // Already written for people: left as it is.
        assert_eq!(c("Claude Code"), "Claude Code");
        assert_eq!(c("My Tool v2"), "My Tool v2");
        assert_eq!(c("  claude-code  "), "Claude Code");
        // The tug command keeps its own name, lowercase.
        let cli = ClientInfo {
            name: "tug".into(),
            kind: ClientKind::Cli,
        };
        assert_eq!(cli.display_name(), "tug");
    }
}
