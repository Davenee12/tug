//! OAuth Authorization Code + PKCE: the pure pieces (code verifier/challenge, the authorize
//! URL, and parsing the redirect that comes back). No I/O here, so it is all unit-tested.
//!
//! Spotify requires PKCE with `S256` and a loopback redirect URI; see
//! <https://developer.spotify.com/documentation/web-api/tutorials/code-pkce-flow>.

use base64::Engine as _;

/// Spotify's OAuth endpoints.
pub const AUTHORIZE_URL: &str = "https://accounts.spotify.com/authorize";
pub const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";

/// The scopes tug asks for: read the user's playlists (incl. private/followed) and add songs to
/// their own, read and control playback, read/modify the library (for Like), and read top items
/// and recently played (the panel's Your top and Recent). Space-separated in the request.
pub const SCOPES: &[&str] = &[
    "playlist-read-private",
    "playlist-read-collaborative",
    "playlist-modify-private",
    "playlist-modify-public",
    "user-read-playback-state",
    "user-modify-playback-state",
    "user-read-currently-playing",
    "user-library-read",
    "user-library-modify",
    "user-top-read",
    "user-read-recently-played",
];

/// The scopes in [`SCOPES`] that a granted `scope` string (space-separated, as the token
/// endpoint returns it) lacks. A connection made before tug asked for a scope keeps working for
/// everything else, but needs reconnecting for the features that use the missing ones.
pub fn missing_scopes(granted: &str) -> Vec<&'static str> {
    let have: Vec<&str> = granted.split_whitespace().collect();
    SCOPES.iter().copied().filter(|s| !have.contains(s)).collect()
}

/// base64url without padding, as PKCE (and OAuth state) want it.
fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// A PKCE code verifier from random bytes: base64url of the bytes (unreserved characters only,
/// 43–128 chars for 32–96 input bytes), per RFC 7636 §4.1.
pub fn code_verifier(random: &[u8]) -> String {
    b64url(random)
}

/// The `S256` code challenge for a verifier: base64url(SHA-256(verifier)), per RFC 7636 §4.2.
pub fn code_challenge(verifier: &str) -> String {
    b64url(&sha256(verifier.as_bytes()))
}

/// SHA-256 (FIPS 180-4). Implemented here rather than pulling in a crypto crate: it's only used
/// for the PKCE code challenge, which is public (not secret key material), and a self-contained
/// implementation keeps tug's dependency set small and is tested against NIST/RFC vectors below.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
        0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
        0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
        0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    let mut w = [0u32; 64];
    for block in msg.as_chunks::<64>().0 {
        for (i, chunk) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*chunk);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for (i, &ki) in K.iter().enumerate() {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(ki).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (dst, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *dst = dst.wrapping_add(v);
        }
    }
    let mut out = [0u8; 32];
    for (chunk, hi) in out.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        *chunk = hi.to_be_bytes();
    }
    out
}

/// An opaque OAuth `state` value from random bytes (guards against a forged redirect).
pub fn oauth_state(random: &[u8]) -> String {
    b64url(random)
}

/// Percent-encode a query component, leaving only the RFC 3986 unreserved characters.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Decode a percent-encoded query component. Unknown/short escapes are left verbatim.
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => match (hex(b[i + 1]), hex(b[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push(h << 4 | l);
                    i += 3;
                }
                _ => {
                    out.push(b'%');
                    i += 1;
                }
            },
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Build the authorize URL the browser is sent to. `redirect_uri` is the full loopback URI
/// (with the port tug actually bound); `scopes` are space-joined and encoded.
pub fn authorize_url(client_id: &str, redirect_uri: &str, challenge: &str, state: &str) -> String {
    let scope = SCOPES.join(" ");
    let q = [
        ("response_type", "code"),
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("code_challenge_method", "S256"),
        ("code_challenge", challenge),
        ("state", state),
        ("scope", &scope),
    ]
    .iter()
    .map(|(k, v)| format!("{k}={}", percent_encode(v)))
    .collect::<Vec<_>>()
    .join("&");
    format!("{AUTHORIZE_URL}?{q}")
}

/// What the redirect carried back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callback {
    /// The authorization code, with the `state` to check against the one tug sent.
    Code { code: String, state: String },
    /// The user (or Spotify) denied access, e.g. `access_denied`.
    Denied(String),
}

/// Parse the query part of the redirect (`code=…&state=…`, or `error=…&state=…`). Values are
/// percent-decoded. `None` when neither a code nor an error is present.
pub fn parse_callback(query: &str) -> Option<Callback> {
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = percent_decode(v);
        match k {
            "code" => code = Some(v),
            "state" => state = Some(v),
            "error" => error = Some(v),
            _ => {}
        }
    }
    if let Some(error) = error {
        return Some(Callback::Denied(error));
    }
    Some(Callback::Code {
        code: code?,
        state: state.unwrap_or_default(),
    })
}

/// Check the returned callback against the state tug sent and pull out the code. A mismatched
/// or missing state is rejected (a forged or stale redirect).
pub fn verify(callback: &Callback, expected_state: &str) -> Result<String, String> {
    match callback {
        Callback::Denied(e) => Err(format!("Spotify sign-in was cancelled ({e})")),
        Callback::Code { code, state } if state == expected_state && !code.is_empty() => Ok(code.clone()),
        Callback::Code { .. } => Err("Spotify sign-in couldn't be verified; please try again".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn missing_scopes_compares_with_what_was_granted() {
        // Everything granted (in any order): nothing missing.
        let all: Vec<&str> = SCOPES.iter().rev().copied().collect();
        assert!(missing_scopes(&all.join(" ")).is_empty());
        // A connection from before Your top, Recent and Add to playlist.
        let old = "playlist-read-private playlist-read-collaborative user-read-playback-state \
                   user-modify-playback-state user-read-currently-playing user-library-read user-library-modify";
        assert_eq!(
            missing_scopes(old),
            vec![
                "playlist-modify-private",
                "playlist-modify-public",
                "user-top-read",
                "user-read-recently-played"
            ]
        );
        assert_eq!(missing_scopes("").len(), SCOPES.len());
        // Whole words only.
        assert!(missing_scopes("user-top-read-extra").contains(&"user-top-read"));
    }

    #[test]
    fn sha256_matches_nist_vectors() {
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // A message long enough to span two 64-byte blocks (exercises the padding path).
        assert_eq!(
            hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn challenge_matches_the_rfc_7636_test_vector() {
        // RFC 7636 Appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(code_challenge(verifier), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn verifier_is_url_safe_and_the_right_length() {
        let v = code_verifier(&[0u8; 32]);
        // 32 bytes → 43 base64url chars (no padding), within the RFC's 43..=128 range.
        assert_eq!(v.len(), 43);
        assert!(v
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~')));
        // Distinct input gives a distinct verifier (sanity, not randomness).
        assert_ne!(code_verifier(&[1u8; 32]), v);
        assert!(!v.contains('=') && !v.contains('+') && !v.contains('/'));
    }

    #[test]
    fn percent_encoding_round_trips_and_escapes_reserved() {
        assert_eq!(percent_encode("a b&c=d"), "a%20b%26c%3Dd");
        assert_eq!(percent_encode("unreserved-._~AZ09"), "unreserved-._~AZ09");
        assert_eq!(percent_decode("a%20b%26c%3Dd"), "a b&c=d");
        assert_eq!(percent_decode("plus+space"), "plus space");
        // A stray percent is kept rather than panicking.
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn authorize_url_has_every_required_param_encoded() {
        let url = authorize_url("CID", "http://127.0.0.1:8899/callback", "CHAL", "STATE");
        assert!(url.starts_with("https://accounts.spotify.com/authorize?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=CID"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("code_challenge=CHAL"));
        assert!(url.contains("state=STATE"));
        // The redirect and the scope list are percent-encoded.
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8899%2Fcallback"));
        assert!(url.contains("scope=playlist-read-private%20playlist-read-collaborative"));
        assert!(url.contains("user-library-modify"));
    }

    #[test]
    fn parses_a_code_callback_and_decodes_values() {
        let cb = parse_callback("code=AQ%2Bxyz&state=st8").unwrap();
        assert_eq!(
            cb,
            Callback::Code {
                code: "AQ+xyz".into(),
                state: "st8".into()
            }
        );
        assert_eq!(verify(&cb, "st8").unwrap(), "AQ+xyz");
    }

    #[test]
    fn rejects_a_mismatched_or_missing_state() {
        let cb = parse_callback("code=abc&state=real").unwrap();
        assert!(verify(&cb, "forged").is_err());
        let no_state = parse_callback("code=abc").unwrap();
        assert!(verify(&no_state, "real").is_err());
    }

    #[test]
    fn surfaces_a_denied_callback() {
        let cb = parse_callback("error=access_denied&state=st").unwrap();
        assert_eq!(cb, Callback::Denied("access_denied".into()));
        assert!(verify(&cb, "st").unwrap_err().contains("cancelled"));
        // Neither code nor error: nothing to act on.
        assert_eq!(parse_callback("foo=bar"), None);
    }
}
