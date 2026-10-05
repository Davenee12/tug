//! OBEX framing (IrDA OBEX 1.5 as profiled by Bluetooth GOEP/MAP).
//!
//! Packet: `[opcode or response code][length: u16 BE, whole packet][fields][headers]`.
//! Header identifier's top two bits give the encoding:
//! `00` UTF-16BE text with NUL terminator, `01` byte sequence, `10` one byte, `11` four bytes.
//! Pure encode/decode; the transport lives in `map::session`.

use thiserror::Error;

pub const OP_CONNECT: u8 = 0x80;
pub const OP_DISCONNECT: u8 = 0x81;
pub const OP_PUT_FINAL: u8 = 0x82;
pub const OP_GET_FINAL: u8 = 0x83;
pub const OP_SETPATH: u8 = 0x85;

pub const RSP_CONTINUE: u8 = 0x90;
pub const RSP_SUCCESS: u8 = 0xA0;
pub const RSP_BAD_REQUEST: u8 = 0xC0;
pub const RSP_UNAUTHORIZED: u8 = 0xC1;
pub const RSP_FORBIDDEN: u8 = 0xC3;
pub const RSP_NOT_FOUND: u8 = 0xC4;
pub const RSP_INTERNAL_ERROR: u8 = 0xD0;
pub const RSP_UNAVAILABLE: u8 = 0xD3;

pub const HI_NAME: u8 = 0x01;
pub const HI_TYPE: u8 = 0x42;
pub const HI_TARGET: u8 = 0x46;
pub const HI_BODY: u8 = 0x48;
pub const HI_END_OF_BODY: u8 = 0x49;
pub const HI_WHO: u8 = 0x4A;
pub const HI_APP_PARAMS: u8 = 0x4C;
pub const HI_CONNECTION_ID: u8 = 0xCB;

/// SETPATH flags: bit 0 = go to parent first, bit 1 = don't create the folder.
pub const SETPATH_DONT_CREATE: u8 = 0x02;

/// Largest packet we advertise in CONNECT; the peer may answer with less.
pub const MAX_PACKET: u16 = 0x2000;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ObexError {
    #[error("OBEX packet truncated")]
    Truncated,
    #[error("OBEX header {0:#04x} is malformed")]
    BadHeader(u8),
    #[error("OBEX packet declares impossible length {0}")]
    BadLength(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Header {
    /// Name (0x01). `None` is the empty Name used to mean "root" or "this folder".
    Name(Option<String>),
    Bytes(u8, Vec<u8>),
    Byte(u8, u8),
    U32(u8, u32),
}

impl Header {
    pub fn id(&self) -> u8 {
        match self {
            Header::Name(_) => HI_NAME,
            Header::Bytes(id, _) | Header::Byte(id, _) | Header::U32(id, _) => *id,
        }
    }

    /// `Type` headers are NUL-terminated ASCII byte sequences.
    pub fn type_(mime: &str) -> Header {
        let mut v = mime.as_bytes().to_vec();
        v.push(0);
        Header::Bytes(HI_TYPE, v)
    }

    pub fn connection_id(id: u32) -> Header {
        Header::U32(HI_CONNECTION_ID, id)
    }

    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Header::Name(text) => {
                out.push(HI_NAME);
                match text {
                    None => out.extend_from_slice(&3u16.to_be_bytes()),
                    Some(t) => {
                        let mut units: Vec<u8> = t.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
                        units.extend_from_slice(&[0, 0]);
                        out.extend_from_slice(&((units.len() + 3) as u16).to_be_bytes());
                        out.extend_from_slice(&units);
                    }
                }
            }
            Header::Bytes(id, v) => {
                out.push(*id);
                out.extend_from_slice(&((v.len() + 3) as u16).to_be_bytes());
                out.extend_from_slice(v);
            }
            Header::Byte(id, b) => out.extend_from_slice(&[*id, *b]),
            Header::U32(id, v) => {
                out.push(*id);
                out.extend_from_slice(&v.to_be_bytes());
            }
        }
    }
}

/// A request: opcode, opcode-specific fields (CONNECT/SETPATH), headers.
pub fn request(opcode: u8, fields: &[u8], headers: &[Header]) -> Vec<u8> {
    let mut out = vec![opcode, 0, 0];
    out.extend_from_slice(fields);
    for h in headers {
        h.encode(&mut out);
    }
    let len = out.len() as u16;
    out[1..3].copy_from_slice(&len.to_be_bytes());
    out
}

pub fn connect(target: &[u8; 16]) -> Vec<u8> {
    let fields = [0x10, 0x00, (MAX_PACKET >> 8) as u8, MAX_PACKET as u8];
    request(OP_CONNECT, &fields, &[Header::Bytes(HI_TARGET, target.to_vec())])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub code: u8,
    /// CONNECT responses only: the peer's maximum packet size.
    pub max_packet: Option<u16>,
    pub headers: Vec<Header>,
}

impl Response {
    pub fn is_success(&self) -> bool {
        self.code == RSP_SUCCESS
    }

    pub fn connection_id(&self) -> Option<u32> {
        self.headers.iter().find_map(|h| match h {
            Header::U32(HI_CONNECTION_ID, v) => Some(*v),
            _ => None,
        })
    }

    /// Body and End-of-Body bytes, concatenated.
    pub fn body(&self) -> Vec<u8> {
        self.headers
            .iter()
            .filter_map(|h| match h {
                Header::Bytes(HI_BODY | HI_END_OF_BODY, v) => Some(v.as_slice()),
                _ => None,
            })
            .flatten()
            .copied()
            .collect()
    }

    pub fn name(&self) -> Option<&str> {
        self.headers.iter().find_map(|h| match h {
            Header::Name(Some(n)) => Some(n.as_str()),
            _ => None,
        })
    }
}

/// Total packet length from the first three bytes, once they've arrived.
pub fn packet_len(prefix: &[u8]) -> Option<usize> {
    (prefix.len() >= 3).then(|| u16::from_be_bytes([prefix[1], prefix[2]]) as usize)
}

/// Parse one complete response packet. `connect` selects the CONNECT layout,
/// which carries version/flags/max-packet before the headers.
pub fn parse_response(b: &[u8], connect: bool) -> Result<Response, ObexError> {
    let len = packet_len(b).ok_or(ObexError::Truncated)?;
    // Every packet has at least opcode + length; a smaller declared length is corrupt.
    if len < 3 {
        return Err(ObexError::BadLength(len));
    }
    if b.len() < len {
        return Err(ObexError::Truncated);
    }
    let (max_packet, mut rest) = if connect && len >= 7 {
        (Some(u16::from_be_bytes([b[5], b[6]])), &b[7..len])
    } else {
        (None, &b[3..len])
    };
    let mut headers = Vec::new();
    while let Some(&id) = rest.first() {
        let (header, used) = match id >> 6 {
            0b00 | 0b01 => {
                if rest.len() < 3 {
                    return Err(ObexError::BadHeader(id));
                }
                let hlen = u16::from_be_bytes([rest[1], rest[2]]) as usize;
                if hlen < 3 || rest.len() < hlen {
                    return Err(ObexError::BadHeader(id));
                }
                let value = &rest[3..hlen];
                let h = if id >> 6 == 0 {
                    Header::Name(decode_utf16(value))
                } else {
                    Header::Bytes(id, value.to_vec())
                };
                (h, hlen)
            }
            0b10 => (Header::Byte(id, *rest.get(1).ok_or(ObexError::BadHeader(id))?), 2),
            _ => {
                let v = rest.get(1..5).ok_or(ObexError::BadHeader(id))?;
                (Header::U32(id, u32::from_be_bytes([v[0], v[1], v[2], v[3]])), 5)
            }
        };
        headers.push(header);
        rest = &rest[used..];
    }
    Ok(Response {
        code: b[0],
        max_packet,
        headers,
    })
}

fn decode_utf16(b: &[u8]) -> Option<String> {
    let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
    let trimmed: &[u16] = match units.iter().position(|&u| u == 0) {
        Some(end) => &units[..end],
        None => &units,
    };
    (!trimmed.is_empty()).then(|| String::from_utf16_lossy(trimmed))
}

/// MAP application parameters: `[tag][len][value]` triplets.
pub fn app_params(params: &[(u8, &[u8])]) -> Header {
    let mut v = Vec::new();
    for (tag, value) in params {
        v.push(*tag);
        v.push(value.len() as u8);
        v.extend_from_slice(value);
    }
    Header::Bytes(HI_APP_PARAMS, v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_connect_with_target() {
        let target = [0xAB; 16];
        let p = connect(&target);
        assert_eq!(p[0], OP_CONNECT);
        assert_eq!(packet_len(&p), Some(p.len()));
        assert_eq!(&p[3..7], &[0x10, 0x00, 0x20, 0x00]);
        assert_eq!(&p[7..10], &[HI_TARGET, 0x00, 19]);
        assert_eq!(&p[10..], &target);
    }

    #[test]
    fn encodes_names_as_utf16be_with_terminator() {
        let p = request(
            OP_SETPATH,
            &[SETPATH_DONT_CREATE, 0],
            &[Header::Name(Some("msg".into()))],
        );
        assert_eq!(&p[5..], &[HI_NAME, 0x00, 0x0B, 0, b'm', 0, b's', 0, b'g', 0, 0]);
        let root = request(OP_SETPATH, &[SETPATH_DONT_CREATE, 0], &[Header::Name(None)]);
        assert_eq!(&root[5..], &[HI_NAME, 0x00, 0x03], "empty Name means root");
    }

    #[test]
    fn type_header_is_nul_terminated() {
        assert_eq!(
            Header::type_("x-bt/message"),
            Header::Bytes(HI_TYPE, b"x-bt/message\0".to_vec())
        );
    }

    #[test]
    fn parses_connect_response() {
        // A0, len 12, version 10, flags 00, max 0x0400, ConnectionID = 1
        let b = [0xA0, 0x00, 0x0C, 0x10, 0x00, 0x04, 0x00, HI_CONNECTION_ID, 0, 0, 0, 1];
        let r = parse_response(&b, true).unwrap();
        assert!(r.is_success());
        assert_eq!(r.max_packet, Some(0x0400));
        assert_eq!(r.connection_id(), Some(1));
    }

    #[test]
    fn parses_body_name_and_forbidden() {
        let mut b = vec![0x90, 0, 0];
        Header::Bytes(HI_BODY, b"<MAP".to_vec()).encode(&mut b);
        Header::Name(Some("20000100001".into())).encode(&mut b);
        let len = b.len() as u16;
        b[1..3].copy_from_slice(&len.to_be_bytes());
        let r = parse_response(&b, false).unwrap();
        assert_eq!(r.code, RSP_CONTINUE);
        assert_eq!(r.body(), b"<MAP");
        assert_eq!(r.name(), Some("20000100001"));
        assert_eq!(
            parse_response(&[RSP_FORBIDDEN, 0, 3], true).unwrap().code,
            RSP_FORBIDDEN
        );
    }

    #[test]
    fn rejects_declared_length_below_header_instead_of_panicking() {
        for len in 0..3u8 {
            assert_eq!(
                parse_response(&[0xA0, 0, len], false),
                Err(ObexError::BadLength(len as usize))
            );
            assert_eq!(
                parse_response(&[0xA0, 0, len], true),
                Err(ObexError::BadLength(len as usize))
            );
        }
    }

    #[test]
    fn rejects_truncated_and_bad_headers() {
        assert_eq!(parse_response(&[0xA0, 0, 10, 0], false), Err(ObexError::Truncated));
        assert_eq!(
            parse_response(&[0xA0, 0, 5, HI_BODY, 0], false),
            Err(ObexError::BadHeader(HI_BODY))
        );
    }

    #[test]
    fn encodes_app_params() {
        assert_eq!(
            app_params(&[(0x01, &[0, 10]), (0x14, &[1])]),
            Header::Bytes(HI_APP_PARAMS, vec![1, 2, 0, 10, 0x14, 1, 1])
        );
    }
}
