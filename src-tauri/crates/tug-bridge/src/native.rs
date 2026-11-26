//! Native messaging: how tug's browser extension (Chrome or Edge) reaches tug.
//!
//! The browser starts `bin\tug.exe` as the extension's native messaging host (registered by the
//! installer, `windows/hooks.nsh`), passing the extension's origin as the first argument, and
//! talks to it over stdin/stdout. Each message is a 32-bit length in native byte order followed
//! by that many bytes of UTF-8 JSON:
//!
//! ```text
//! extension → {"id":7,"call":{"tool":"get_latest_code","copy":true}}
//! host      → {"id":7,"ok":{...}}  or  {"id":7,"error":{"code":"not_running","message":"..."}}
//! ```
//!
//! The host makes each call over the bridge like the `tug` command does, introducing itself as
//! `ClientKind::Browser`, so every switch, the rate limits and the confirmation card apply as
//! they do to AI tools. On top of that, only the calls in `allowed_call` get through: what the
//! extension's popup and "Fill code" need, and nothing that reads or offers files. Pure: no I/O
//! here except the generic `read_message`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::client::ClientError;
use crate::protocol::{Call, MAX_CLIENT_LINE};

/// Longest message the extension may send: the same cap as a bridge call's line.
pub const MAX_FROM_BROWSER: usize = MAX_CLIENT_LINE;
/// Longest message the browser accepts from a native host (Chrome's limit is 1 MB).
pub const MAX_TO_BROWSER: usize = 1024 * 1024;

/// Host-level error codes, beside the bridge's own (`ErrorCode`, as they serialize).
pub mod codes {
    pub const NOT_RUNNING: &str = "not_running";
    pub const NOT_SET_UP: &str = "not_set_up";
    pub const TIMEOUT: &str = "timeout";
    pub const SEND_TIMEOUT: &str = "send_timeout";
    pub const IMPOSTOR: &str = "impostor";
    pub const FAILED: &str = "failed";
    /// A call the extension isn't allowed to make.
    pub const NOT_ALLOWED: &str = "not_allowed";
    /// Not a message the host understands.
    pub const BAD_REQUEST: &str = "bad_request";
    /// tug's answer is bigger than the browser takes.
    pub const TOO_LARGE: &str = "too_large";
}

#[derive(Debug)]
pub enum ReadError {
    Io(std::io::Error),
    /// The length says more than `MAX_FROM_BROWSER`.
    TooLong(usize),
}

/// The next message from the browser, or `None` when it closed stdin between messages (it's
/// done with the host). A stream cut mid-message is an error.
pub async fn read_message<R: AsyncRead + Unpin>(r: &mut R) -> Result<Option<Vec<u8>>, ReadError> {
    let mut len = [0u8; 4];
    let mut got = 0;
    while got < len.len() {
        match r.read(&mut len[got..]).await.map_err(ReadError::Io)? {
            0 if got == 0 => return Ok(None),
            0 => return Err(ReadError::Io(std::io::ErrorKind::UnexpectedEof.into())),
            n => got += n,
        }
    }
    let n = u32::from_ne_bytes(len) as usize;
    if n > MAX_FROM_BROWSER {
        return Err(ReadError::TooLong(n));
    }
    let mut body = vec![0u8; n];
    r.read_exact(&mut body).await.map_err(ReadError::Io)?;
    Ok(Some(body))
}

/// One message for the browser: its length, then the JSON.
pub fn frame(json: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(json.len() + 4);
    out.extend_from_slice(&(json.len() as u32).to_ne_bytes());
    out.extend_from_slice(json);
    out
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BrowserRequest {
    /// The extension's own number for the call, echoed in the reply.
    pub id: u64,
    pub call: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplyError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserReply {
    /// `None` only when the request couldn't be read at all.
    pub id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ReplyError>,
}

impl BrowserReply {
    pub fn ok(id: u64, value: Value) -> Self {
        BrowserReply {
            id: Some(id),
            ok: Some(value),
            error: None,
        }
    }

    pub fn error(id: Option<u64>, code: &str, message: impl Into<String>) -> Self {
        BrowserReply {
            id,
            ok: None,
            error: Some(ReplyError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }

    /// The reply for a call that failed, in the same words the `tug` command uses.
    pub fn failed(id: u64, e: &ClientError) -> Self {
        Self::error(Some(id), &error_code(e), e.to_string())
    }

    /// The framed bytes for the browser. An answer too big for it becomes an error saying so,
    /// rather than a message the browser would drop along with the connection.
    pub fn to_bytes(&self) -> Vec<u8> {
        let json = serde_json::to_vec(self).expect("replies always serialize");
        if json.len() <= MAX_TO_BROWSER {
            return frame(&json);
        }
        let small = Self::error(self.id, codes::TOO_LARGE, "tug's answer was too big for the browser.");
        frame(&serde_json::to_vec(&small).expect("replies always serialize"))
    }
}

/// The code the extension sees for a failed call: the bridge's own when tug answered, else the
/// host's.
pub fn error_code(e: &ClientError) -> String {
    match e {
        ClientError::Bridge(b) => serde_json::to_value(b.code)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| codes::FAILED.into()),
        ClientError::NotRunning => codes::NOT_RUNNING.into(),
        ClientError::NotSetUp => codes::NOT_SET_UP.into(),
        ClientError::Timeout => codes::TIMEOUT.into(),
        ClientError::SendTimeout => codes::SEND_TIMEOUT.into(),
        ClientError::Impostor => codes::IMPOSTOR.into(),
        ClientError::Other(_) => codes::FAILED.into(),
    }
}

/// Read one request. `Err` is the reply to send instead.
pub fn parse_request(bytes: &[u8]) -> Result<(u64, Call), BrowserReply> {
    let req: BrowserRequest = serde_json::from_slice(bytes)
        .map_err(|_| BrowserReply::error(None, codes::BAD_REQUEST, "That isn't a message tug understands."))?;
    let call: Call = serde_json::from_value(req.call)
        .map_err(|_| BrowserReply::error(Some(req.id), codes::BAD_REQUEST, "That isn't a call tug knows."))?;
    let call = allowed_call(call).map_err(|m| BrowserReply::error(Some(req.id), codes::NOT_ALLOWED, m))?;
    Ok((req.id, call))
}

/// The calls the extension may make, with anything that only a person at a terminal may claim
/// taken out. Files are left out entirely: a web page's extension has no business naming paths
/// on this PC, and Tugboat files can be bigger than the browser takes.
pub fn allowed_call(call: Call) -> Result<Call, &'static str> {
    match call {
        Call::Status | Call::PhoneStatus | Call::GetLatestCode { .. } | Call::MediaControl { .. } => Ok(call),
        Call::SearchMessages { .. } => Ok(call),
        // Never `typed`: that's the terminal's switch. The browser has its own, which tug checks
        // against the kind of caller (a browser client), not anything in the call.
        Call::SendText { to, message, .. } => Ok(Call::SendText {
            to,
            message,
            progress: false,
            typed: false,
        }),
        Call::RecentDevNotifications { .. }
        | Call::ListTugboatFiles { .. }
        | Call::GetTugboatFile { .. }
        | Call::OfferFiles { .. } => Err("The browser extension can't do that."),
    }
}

/// Whether the browser started this host for an extension: Chrome and Edge pass the caller's
/// origin, `chrome-extension://<32 letters a–p>/`, as the first argument.
pub fn is_extension_origin(arg: &str) -> bool {
    arg.strip_prefix("chrome-extension://")
        .and_then(|rest| rest.strip_suffix('/'))
        .is_some_and(|id| id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{BridgeError, ErrorCode};
    use serde_json::json;

    fn req(call: Value) -> Vec<u8> {
        serde_json::to_vec(&json!({ "id": 3, "call": call })).unwrap()
    }

    #[tokio::test]
    async fn messages_are_length_prefixed_json() {
        let mut stream = frame(br#"{"a":1}"#);
        stream.extend(frame(b"{}"));
        let mut r = stream.as_slice();
        assert_eq!(read_message(&mut r).await.unwrap().unwrap(), br#"{"a":1}"#);
        assert_eq!(read_message(&mut r).await.unwrap().unwrap(), b"{}");
        // Closed between messages: done, not an error.
        assert!(read_message(&mut r).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_cut_or_oversized_message_is_an_error() {
        let mut cut: &[u8] = &[10, 0];
        assert!(matches!(read_message(&mut cut).await, Err(ReadError::Io(_))));
        let mut short = frame(b"{\"a\":1}");
        short.truncate(6);
        assert!(matches!(
            read_message(&mut short.as_slice()).await,
            Err(ReadError::Io(_))
        ));
        let huge = ((MAX_FROM_BROWSER + 1) as u32).to_ne_bytes();
        assert!(matches!(
            read_message(&mut huge.as_slice()).await,
            Err(ReadError::TooLong(_))
        ));
    }

    #[test]
    fn the_popups_calls_get_through() {
        for call in [
            json!({"tool":"status"}),
            json!({"tool":"phone_status"}),
            json!({"tool":"get_latest_code","copy":true}),
            json!({"tool":"search_messages","query":"dinner"}),
            json!({"tool":"media_control","action":"next"}),
        ] {
            let (id, _) = parse_request(&req(call.clone())).unwrap_or_else(|e| panic!("{call}: {e:?}"));
            assert_eq!(id, 3);
        }
    }

    #[test]
    fn a_text_always_goes_through_the_card() {
        let call = json!({"tool":"send_text","to":"Maya","message":"hi","typed":true,"progress":true});
        let (_, call) = parse_request(&req(call)).unwrap();
        assert_eq!(
            call,
            Call::SendText {
                to: "Maya".into(),
                message: "hi".into(),
                progress: false,
                typed: false,
            }
        );
    }

    #[test]
    fn file_calls_are_refused() {
        for call in [
            json!({"tool":"offer_files","paths":["C:\\Users\\me\\secret.txt"]}),
            json!({"tool":"get_tugboat_file","name":"a.jpg"}),
            json!({"tool":"list_tugboat_files"}),
            json!({"tool":"recent_dev_notifications"}),
        ] {
            let e = parse_request(&req(call)).unwrap_err();
            assert_eq!(e.id, Some(3));
            assert_eq!(e.error.unwrap().code, codes::NOT_ALLOWED);
        }
    }

    #[test]
    fn junk_gets_a_bad_request_reply() {
        let e = parse_request(b"not json").unwrap_err();
        assert_eq!((e.id, e.error.unwrap().code.as_str()), (None, codes::BAD_REQUEST));
        let e = parse_request(&req(json!({"tool":"format_disk"}))).unwrap_err();
        assert_eq!((e.id, e.error.unwrap().code.as_str()), (Some(3), codes::BAD_REQUEST));
    }

    #[test]
    fn errors_carry_the_bridges_code_or_the_hosts() {
        let off = ClientError::Bridge(BridgeError::new(ErrorCode::ToolOff, "Codes is off."));
        let r = BrowserReply::failed(9, &off);
        assert_eq!(r.error.as_ref().unwrap().code, "tool_off");
        assert_eq!(r.error.unwrap().message, "Codes is off.");
        assert_eq!(error_code(&ClientError::NotRunning), codes::NOT_RUNNING);
        assert_eq!(error_code(&ClientError::NotSetUp), codes::NOT_SET_UP);
        assert_eq!(error_code(&ClientError::SendTimeout), codes::SEND_TIMEOUT);
        assert_eq!(error_code(&ClientError::Other("x".into())), codes::FAILED);
    }

    #[test]
    fn replies_are_framed_and_never_too_big_for_the_browser() {
        let bytes = BrowserReply::ok(1, json!({"code":"482913"})).to_bytes();
        let len = u32::from_ne_bytes(bytes[..4].try_into().unwrap()) as usize;
        assert_eq!(len, bytes.len() - 4);
        let back: BrowserReply = serde_json::from_slice(&bytes[4..]).unwrap();
        assert_eq!(back.ok.unwrap()["code"], "482913");
        assert!(back.error.is_none());

        let big = BrowserReply::ok(2, json!({ "text": "x".repeat(MAX_TO_BROWSER) })).to_bytes();
        assert!(big.len() <= MAX_TO_BROWSER + 4);
        let back: BrowserReply = serde_json::from_slice(&big[4..]).unwrap();
        assert_eq!(
            (back.id, back.error.unwrap().code.as_str()),
            (Some(2), codes::TOO_LARGE)
        );
    }

    #[test]
    fn only_an_extension_origin_starts_the_host() {
        assert!(is_extension_origin(
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop/"
        ));
        assert!(!is_extension_origin(
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop"
        ));
        assert!(!is_extension_origin("chrome-extension://short/"));
        assert!(!is_extension_origin(
            "chrome-extension://ABCDEFGHIJKLMNOPABCDEFGHIJKLMNOP/"
        ));
        assert!(!is_extension_origin("https://example.com/"));
        assert!(!is_extension_origin("code"));
    }
}
