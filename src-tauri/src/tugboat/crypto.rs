//! Tugboat's crypto, shared byte-for-byte with the phone page (`src/tugboat-page/crypto.ts`).
//!
//! The QR carries a 128-bit secret after the `#`, which browsers never send over the network.
//! Both sides derive two keys from it with HKDF-SHA256: one seals content with XChaCha20-Poly1305
//! (a random 24-byte nonce per message), the other MACs every API request so only the phone that
//! scanned the code can drive the session. Associated data names the direction, the file and the
//! chunk index, so a chunk can't be swapped, replayed into another file, or reflected back.
//!
//! Honest limits: this stops anyone passively listening on the Wi-Fi. The page itself is served
//! over plain HTTP, so an active attacker who tampers with the first page load could still win.
//! The secret also lives, for that session only, wherever the link went: the phone's browser
//! history may keep the first URL (the page strips the `#` right away, but the history entry can
//! remain), and "Copy link" puts it on the PC clipboard (kept out of clipboard history and sync).
//! Never call it end-to-end secure.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// Protocol label: the HKDF salt and the prefix of every associated-data and MAC string.
pub const VERSION: &str = "tugboat/1";
pub const SECRET_LEN: usize = 16;
pub const NONCE_LEN: usize = 24;
pub const TAG_LEN: usize = 16;
/// Bytes a sealed message adds to its plaintext (nonce in front, tag at the end).
pub const OVERHEAD: usize = NONCE_LEN + TAG_LEN;
/// Request MACs are HMAC-SHA256 truncated to 128 bits.
pub const MAC_LEN: usize = 16;

/// The two keys derived from one Tugboat session's secret.
pub struct Keys {
    enc: [u8; 32],
    auth: [u8; 32],
}

impl Keys {
    pub fn derive(secret: &[u8]) -> Keys {
        let hk = Hkdf::<Sha256>::new(Some(VERSION.as_bytes()), secret);
        let mut enc = [0u8; 32];
        let mut auth = [0u8; 32];
        // 32 bytes is far below HKDF-SHA256's 8160-byte limit, so expand can't fail.
        hk.expand(b"encrypt", &mut enc).expect("HKDF length");
        hk.expand(b"authenticate", &mut auth).expect("HKDF length");
        Keys { enc, auth }
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new((&self.enc).into())
    }

    /// Seal with a fresh random nonce: `nonce || ciphertext || tag`.
    pub fn seal(&self, ad: &[u8], plaintext: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        self.seal_with_nonce(&nonce, ad, plaintext)
    }

    /// Seal with a given nonce. Only for the shared test vector; real messages use `seal`.
    pub fn seal_with_nonce(&self, nonce: &[u8; NONCE_LEN], ad: &[u8], plaintext: &[u8]) -> Vec<u8> {
        let ct = self
            .cipher()
            .encrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: plaintext,
                    aad: ad,
                },
            )
            // Only fails for plaintexts beyond ~256 GB, far above any chunk.
            .expect("XChaCha20-Poly1305 seal");
        let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
        out.extend_from_slice(nonce);
        out.extend_from_slice(&ct);
        out
    }

    /// Open `nonce || ciphertext || tag`; `None` if it's too short, tampered with, or the
    /// associated data doesn't match (a chunk moved to another file or index).
    pub fn open(&self, ad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
        if sealed.len() < OVERHEAD {
            return None;
        }
        let (nonce, ct) = sealed.split_at(NONCE_LEN);
        self.cipher()
            .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad: ad })
            .ok()
    }

    fn mac(&self) -> Hmac<Sha256> {
        <Hmac<Sha256> as Mac>::new_from_slice(&self.auth).expect("HMAC takes any key length")
    }

    /// The MAC the phone sends with a request (see `auth_message`). The server only verifies;
    /// the tests that play the phone sign.
    #[cfg(test)]
    pub fn request_mac(&self, client: &str, seq: u64, method: &str, path: &str) -> [u8; MAC_LEN] {
        let mut m = self.mac();
        m.update(auth_message(client, seq, method, path).as_bytes());
        let full = m.finalize().into_bytes();
        let mut out = [0u8; MAC_LEN];
        out.copy_from_slice(&full[..MAC_LEN]);
        out
    }

    /// Constant-time check of a request MAC.
    pub fn verify_request_mac(&self, client: &str, seq: u64, method: &str, path: &str, mac: &[u8]) -> bool {
        if mac.len() != MAC_LEN {
            return false;
        }
        let mut m = self.mac();
        m.update(auth_message(client, seq, method, path).as_bytes());
        m.verify_truncated_left(mac).is_ok()
    }
}

/// What a request MAC covers: who (client id), which request (sequence), and what (method + path).
/// Bodies are sealed separately, with associated data that binds them to this same request.
pub fn auth_message(client: &str, seq: u64, method: &str, path: &str) -> String {
    format!("{VERSION}|auth|{client}|{seq}|{method}|{path}")
}

/// Associated data for each kind of sealed message. Direction is part of it, so nothing the PC
/// sends can be fed back to it as if the phone had sent it.
pub mod ad {
    use super::VERSION;

    /// A chunk of a file the phone is sending to the PC.
    pub fn up(file_id: &str, index: u32) -> Vec<u8> {
        format!("{VERSION}|up|{file_id}|{index}").into_bytes()
    }
    /// A chunk of a file the PC is offering to the phone.
    pub fn down(offer_id: &str, index: u32) -> Vec<u8> {
        format!("{VERSION}|down|{offer_id}|{index}").into_bytes()
    }
    /// A JSON request body, bound to the authenticated request that carried it.
    pub fn request(client: &str, seq: u64) -> Vec<u8> {
        format!("{VERSION}|req|{client}|{seq}").into_bytes()
    }
    /// A JSON response body, bound to the request it answers.
    pub fn response(client: &str, seq: u64) -> Vec<u8> {
        format!("{VERSION}|res|{client}|{seq}").into_bytes()
    }
}

/// A fresh session secret for the QR code.
pub fn new_secret() -> [u8; SECRET_LEN] {
    let mut s = [0u8; SECRET_LEN];
    OsRng.fill_bytes(&mut s);
    s
}

/// A random 128-bit id (offers, text ids), base64url.
pub fn new_id() -> String {
    let mut b = [0u8; 16];
    OsRng.fill_bytes(&mut b);
    b64(&b)
}

pub fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn unb64(s: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(s).ok()
}

/// Ids the phone makes up (client and file ids) are base64url of 16–32 random bytes. Checked
/// before they're used in a path or a file name.
pub fn valid_id(s: &str) -> bool {
    (16..=43).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    /// The same file is asserted by vitest (`src/tugboat-page/crypto.test.ts`), which is what proves
    /// the page's @noble/ciphers and this RustCrypto code interoperate.
    const VECTOR: &str = include_str!("../../../src/tugboat-page/testVector.json");

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Vector {
        secret: String,
        enc_key: String,
        auth_key: String,
        nonce: String,
        ads: AdVector,
        plaintext: String,
        sealed: String,
        mac: MacVector,
    }

    /// The associated-data strings each builder must produce, byte for byte.
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct AdVector {
        file_id: String,
        index: u32,
        offer_id: String,
        offer_index: u32,
        up: String,
        down: String,
        req: String,
        res: String,
    }

    #[derive(Deserialize)]
    struct MacVector {
        client: String,
        seq: u64,
        method: String,
        path: String,
        mac: String,
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn to_hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn shared_test_vector_matches() {
        let v: Vector = serde_json::from_str(VECTOR).unwrap();
        let keys = Keys::derive(&hex(&v.secret));
        assert_eq!(to_hex(&keys.enc), v.enc_key);
        assert_eq!(to_hex(&keys.auth), v.auth_key);
        // Every associated-data builder, byte for byte (the page asserts the same strings).
        let a = &v.ads;
        assert_eq!(ad::up(&a.file_id, a.index), a.up.as_bytes());
        assert_eq!(ad::down(&a.offer_id, a.offer_index), a.down.as_bytes());
        assert_eq!(ad::request(&v.mac.client, v.mac.seq), a.req.as_bytes());
        assert_eq!(ad::response(&v.mac.client, v.mac.seq), a.res.as_bytes());
        let nonce: [u8; NONCE_LEN] = hex(&v.nonce).try_into().unwrap();
        let up = ad::up(&a.file_id, a.index);
        let sealed = keys.seal_with_nonce(&nonce, &up, v.plaintext.as_bytes());
        assert_eq!(to_hex(&sealed), v.sealed);
        assert_eq!(keys.open(&up, &sealed).unwrap(), v.plaintext.as_bytes());
        let mac = keys.request_mac(&v.mac.client, v.mac.seq, &v.mac.method, &v.mac.path);
        assert_eq!(b64(&mac), v.mac.mac);
    }

    #[test]
    fn open_rejects_tampering_and_moved_chunks() {
        let keys = Keys::derive(&[7u8; 16]);
        let sealed = keys.seal(&ad::up("fileAAAAAAAAAAAAAAAA", 2), b"hello");
        assert_eq!(
            keys.open(&ad::up("fileAAAAAAAAAAAAAAAA", 2), &sealed).unwrap(),
            b"hello"
        );
        // Another index, another file, or the other direction: all refused.
        assert!(keys.open(&ad::up("fileAAAAAAAAAAAAAAAA", 3), &sealed).is_none());
        assert!(keys.open(&ad::up("fileBBBBBBBBBBBBBBBB", 2), &sealed).is_none());
        assert!(keys.open(&ad::down("fileAAAAAAAAAAAAAAAA", 2), &sealed).is_none());
        let mut flipped = sealed.clone();
        *flipped.last_mut().unwrap() ^= 1;
        assert!(keys.open(&ad::up("fileAAAAAAAAAAAAAAAA", 2), &flipped).is_none());
        assert!(keys.open(b"", &sealed[..OVERHEAD - 1]).is_none());
        // A different secret can't open it either.
        assert!(Keys::derive(&[8u8; 16])
            .open(&ad::up("fileAAAAAAAAAAAAAAAA", 2), &sealed)
            .is_none());
    }

    #[test]
    fn nonces_are_random() {
        let keys = Keys::derive(&[1u8; 16]);
        assert_ne!(keys.seal(b"", b"x"), keys.seal(b"", b"x"));
    }

    #[test]
    fn request_mac_covers_every_field() {
        let keys = Keys::derive(&[3u8; 16]);
        let mac = keys.request_mac("clientAAAAAAAAAAAAAA", 5, "PUT", "/api/up/x/1");
        assert!(keys.verify_request_mac("clientAAAAAAAAAAAAAA", 5, "PUT", "/api/up/x/1", &mac));
        assert!(!keys.verify_request_mac("clientBBBBBBBBBBBBBB", 5, "PUT", "/api/up/x/1", &mac));
        assert!(!keys.verify_request_mac("clientAAAAAAAAAAAAAA", 6, "PUT", "/api/up/x/1", &mac));
        assert!(!keys.verify_request_mac("clientAAAAAAAAAAAAAA", 5, "GET", "/api/up/x/1", &mac));
        assert!(!keys.verify_request_mac("clientAAAAAAAAAAAAAA", 5, "PUT", "/api/up/x/2", &mac));
        assert!(!keys.verify_request_mac("clientAAAAAAAAAAAAAA", 5, "PUT", "/api/up/x/1", &mac[..8]));
    }

    #[test]
    fn ids_are_checked() {
        assert!(valid_id(&new_id()));
        assert!(valid_id("AAAAAAAAAAAAAAAAAAAAAA"));
        assert!(!valid_id("short"));
        assert!(!valid_id("../../../../etc/passwd!!"));
        assert!(!valid_id("AAAAAAAA.AAAAAAAAAAAAA"));
        assert!(!valid_id(&"A".repeat(44)));
    }
}
