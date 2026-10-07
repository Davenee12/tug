//! The per-install token and the handshake proofs.
//!
//! The token is 32 random bytes (64 hex characters) in a file only the user can read
//! (`token_file.rs`). It never crosses the pipe: each side proves it holds the token with an
//! HMAC over both nonces, the server first. So a process that squats the pipe name while tug is
//! closed can't learn the token, and a client without the file can't call anything.

use hmac::{Hmac, Mac};
use sha2::Sha256;

pub const TOKEN_BYTES: usize = 32;
pub const NONCE_BYTES: usize = 16;

const SERVER_LABEL: &[u8] = b"tug-bridge/v1/server";
const CLIENT_LABEL: &[u8] = b"tug-bridge/v1/client";

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).expect("the OS random number generator");
    hex(&b)
}

/// A fresh token, as stored in the file.
pub fn new_token() -> String {
    random_hex(TOKEN_BYTES)
}

pub fn new_nonce() -> String {
    random_hex(NONCE_BYTES)
}

/// A token read from the file: trimmed, and exactly 64 hex characters (lowercased).
pub fn parse_token(s: &str) -> Option<String> {
    let t = s.trim().to_ascii_lowercase();
    (t.len() == TOKEN_BYTES * 2 && unhex(&t).is_some()).then_some(t)
}

/// A nonce from the other side: exactly 16 bytes of hex.
pub fn valid_nonce(s: &str) -> bool {
    s.len() == NONCE_BYTES * 2 && unhex(s).is_some()
}

fn mac(token: &str, label: &[u8], client_nonce: &str, server_nonce: &str) -> Hmac<Sha256> {
    let key = unhex(token).unwrap_or_default();
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(&key).expect("HMAC takes any key length");
    m.update(label);
    m.update(b"\0");
    m.update(client_nonce.as_bytes());
    m.update(b"\0");
    m.update(server_nonce.as_bytes());
    m
}

pub fn server_proof(token: &str, client_nonce: &str, server_nonce: &str) -> String {
    hex(&mac(token, SERVER_LABEL, client_nonce, server_nonce)
        .finalize()
        .into_bytes())
}

pub fn client_proof(token: &str, client_nonce: &str, server_nonce: &str) -> String {
    hex(&mac(token, CLIENT_LABEL, client_nonce, server_nonce)
        .finalize()
        .into_bytes())
}

fn verify(m: Hmac<Sha256>, proof: &str) -> bool {
    match unhex(proof) {
        Some(p) => m.verify_slice(&p).is_ok(),
        None => false,
    }
}

/// Constant-time check of the server's proof (done by the client).
pub fn verify_server(token: &str, client_nonce: &str, server_nonce: &str, proof: &str) -> bool {
    verify(mac(token, SERVER_LABEL, client_nonce, server_nonce), proof)
}

/// Constant-time check of the client's proof (done by tug).
pub fn verify_client(token: &str, client_nonce: &str, server_nonce: &str, proof: &str) -> bool {
    verify(mac(token, CLIENT_LABEL, client_nonce, server_nonce), proof)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_and_well_formed() {
        let a = new_token();
        let b = new_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
        assert_eq!(parse_token(&format!("  {}\r\n", a.to_uppercase())), Some(a));
        assert_eq!(parse_token("abc"), None);
        assert_eq!(parse_token(&"g".repeat(64)), None);
        assert_eq!(parse_token(""), None);
    }

    #[test]
    fn nonces_are_checked() {
        assert!(valid_nonce(&new_nonce()));
        assert!(!valid_nonce("00"));
        assert!(!valid_nonce(&"zz".repeat(16)));
    }

    #[test]
    fn both_sides_prove_the_token() {
        let token = new_token();
        let (cn, sn) = (new_nonce(), new_nonce());
        let sp = server_proof(&token, &cn, &sn);
        let cp = client_proof(&token, &cn, &sn);
        assert!(verify_server(&token, &cn, &sn, &sp));
        assert!(verify_client(&token, &cn, &sn, &cp));
        // A proof only works in its own direction, with its own nonces and its own token.
        assert!(!verify_client(&token, &cn, &sn, &sp));
        assert!(!verify_server(&token, &cn, &sn, &cp));
        assert!(!verify_client(&token, &new_nonce(), &sn, &cp));
        assert!(!verify_client(&new_token(), &cn, &sn, &cp));
        assert!(!verify_client(&token, &cn, &sn, "not hex"));
        assert!(!verify_client(&token, &cn, &sn, ""));
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(hex(&[0, 15, 255]), "000fff");
        assert_eq!(unhex("000fff"), Some(vec![0, 15, 255]));
        assert_eq!(unhex("0"), None);
    }
}
