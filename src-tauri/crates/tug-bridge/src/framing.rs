//! One JSON message per line, with a hard cap on line length so neither side can be made to
//! buffer without limit.

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Debug)]
pub enum FrameError {
    Io(std::io::Error),
    /// The other side hung up before a full line.
    Closed,
    TooLong,
    BadJson(String),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Io(e) => write!(f, "{e}"),
            FrameError::Closed => f.write_str("the connection closed"),
            FrameError::TooLong => f.write_str("message too long"),
            FrameError::BadJson(e) => write!(f, "bad message: {e}"),
        }
    }
}

/// Serialize as one line (serde_json never writes a raw newline inside a value).
pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    let mut v = serde_json::to_vec(msg).expect("bridge messages always serialize");
    v.push(b'\n');
    v
}

pub fn decode<T: DeserializeOwned>(line: &[u8]) -> Result<T, FrameError> {
    serde_json::from_slice(line).map_err(|e| FrameError::BadJson(e.to_string()))
}

/// Read up to the next newline, refusing lines longer than `max` bytes.
pub async fn read_line<R: AsyncBufRead + Unpin>(r: &mut R, max: usize) -> Result<Vec<u8>, FrameError> {
    let mut out = Vec::new();
    loop {
        let buf = r.fill_buf().await.map_err(FrameError::Io)?;
        if buf.is_empty() {
            return Err(FrameError::Closed);
        }
        match buf.iter().position(|&b| b == b'\n') {
            Some(i) => {
                if out.len() + i > max {
                    return Err(FrameError::TooLong);
                }
                out.extend_from_slice(&buf[..i]);
                r.consume(i + 1);
                if out.last() == Some(&b'\r') {
                    out.pop();
                }
                return Ok(out);
            }
            None => {
                let n = buf.len();
                if out.len() + n > max {
                    return Err(FrameError::TooLong);
                }
                out.extend_from_slice(buf);
                r.consume(n);
            }
        }
    }
}

pub async fn read_msg<T: DeserializeOwned, R: AsyncBufRead + Unpin>(r: &mut R, max: usize) -> Result<T, FrameError> {
    decode(&read_line(r, max).await?)
}

pub async fn write_msg<T: Serialize, W: AsyncWrite + Unpin>(w: &mut W, msg: &T) -> Result<(), FrameError> {
    w.write_all(&encode(msg)).await.map_err(FrameError::Io)?;
    w.flush().await.map_err(FrameError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{BridgeError, ErrorCode, ServerMsg};

    #[tokio::test]
    async fn reads_lines_one_at_a_time() {
        let mut input: &[u8] = b"{\"a\":1}\n{\"b\":2}\r\n";
        assert_eq!(read_line(&mut input, 100).await.unwrap(), b"{\"a\":1}");
        assert_eq!(read_line(&mut input, 100).await.unwrap(), b"{\"b\":2}");
        assert!(matches!(read_line(&mut input, 100).await, Err(FrameError::Closed)));
    }

    #[tokio::test]
    async fn refuses_overlong_lines() {
        let long = format!("{}\n", "x".repeat(50));
        let mut input = long.as_bytes();
        assert!(matches!(read_line(&mut input, 10).await, Err(FrameError::TooLong)));
        let mut unterminated: &[u8] = &[b'y'; 64];
        assert!(matches!(
            read_line(&mut unterminated, 10).await,
            Err(FrameError::TooLong)
        ));
    }

    #[tokio::test]
    async fn half_a_line_is_closed_not_a_message() {
        let mut input: &[u8] = b"{\"a\":";
        assert!(matches!(read_line(&mut input, 100).await, Err(FrameError::Closed)));
    }

    #[tokio::test]
    async fn messages_round_trip_through_a_pipe() {
        let msg = ServerMsg::Error(BridgeError::new(ErrorCode::Busy, "line one\nline two"));
        let bytes = encode(&msg);
        assert_eq!(bytes.iter().filter(|&&b| b == b'\n').count(), 1, "one line per message");
        let mut r = bytes.as_slice();
        let back: ServerMsg = read_msg(&mut r, 1000).await.unwrap();
        assert_eq!(back, msg);
        let mut junk: &[u8] = b"not json\n";
        assert!(matches!(
            read_msg::<ServerMsg, _>(&mut junk, 100).await,
            Err(FrameError::BadJson(_))
        ));
    }
}
