//! Who may drive a Tugboat session: the request-authentication state machine.
//!
//! Every API request carries `Authorization: Tugboat <client>.<seq>.<mac>`: a client id the page
//! made up, a sequence number that only goes up (the page starts it from the clock), and a MAC of
//! both plus the method and path under the session's auth key. The first request that passes
//! binds the session to that client; any other client gets "in use" even with the secret (a
//! second phone that scanned the same code). A sequence number seen before, or far older than
//! the newest, is a replay and is refused.

use std::collections::BTreeSet;

use super::crypto::{self, Keys};

/// How far behind the newest sequence number a request may still arrive (requests run in
/// parallel, so they can land out of order). The page's numbers are `ms * 1024 + counter`, so
/// this is about a minute.
pub const REPLAY_WINDOW: u64 = 60_000 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorized {
    pub client: String,
    pub seq: u64,
    /// True for the request that bound the session (the phone just connected).
    pub first: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejected {
    /// No header, or not in the expected shape.
    Malformed,
    /// The MAC doesn't verify: not made with this session's secret.
    BadMac,
    /// A sequence number already used, or too old.
    Replayed,
    /// Valid, but the session belongs to another device.
    OtherDevice,
}

#[derive(Default)]
pub struct Auth {
    bound: Option<String>,
    newest: u64,
    seen: BTreeSet<u64>,
}

impl Auth {
    pub fn new() -> Auth {
        Auth::default()
    }

    #[cfg(test)]
    pub fn bound(&self) -> bool {
        self.bound.is_some()
    }

    /// The client id the session is bound to, if a phone has connected.
    pub fn bound_client(&self) -> Option<String> {
        self.bound.clone()
    }

    pub fn check(
        &mut self,
        keys: &Keys,
        header: Option<&str>,
        method: &str,
        path: &str,
    ) -> Result<Authorized, Rejected> {
        let (client, seq, mac) = parse(header.ok_or(Rejected::Malformed)?).ok_or(Rejected::Malformed)?;
        if !keys.verify_request_mac(client, seq, method, path, &mac) {
            return Err(Rejected::BadMac);
        }
        if let Some(bound) = &self.bound {
            if bound != client {
                return Err(Rejected::OtherDevice);
            }
        }
        if !self.fresh(seq) {
            return Err(Rejected::Replayed);
        }
        let first = self.bound.is_none();
        if first {
            self.bound = Some(client.to_string());
        }
        Ok(Authorized {
            client: client.to_string(),
            seq,
            first,
        })
    }

    /// Record `seq` if it hasn't been seen and isn't older than the window.
    fn fresh(&mut self, seq: u64) -> bool {
        if seq.saturating_add(REPLAY_WINDOW) < self.newest || self.seen.contains(&seq) {
            return false;
        }
        self.seen.insert(seq);
        if seq > self.newest {
            self.newest = seq;
            // Anything older than the window is refused by the first test, so forget it.
            let floor = self.newest.saturating_sub(REPLAY_WINDOW);
            self.seen = self.seen.split_off(&floor);
        }
        true
    }
}

/// `Tugboat <client>.<seq>.<mac>` → its parts, with the client id and MAC validated.
fn parse(header: &str) -> Option<(&str, u64, Vec<u8>)> {
    let rest = header.strip_prefix("Tugboat ")?;
    let mut parts = rest.split('.');
    let client = parts.next()?;
    let seq = parts.next()?.parse::<u64>().ok()?;
    let mac = crypto::unb64(parts.next()?)?;
    if parts.next().is_some() || !crypto::valid_id(client) {
        return None;
    }
    Some((client, seq, mac))
}

/// Build the header the page sends (used by tests that play the phone).
#[cfg(test)]
pub fn header(keys: &Keys, client: &str, seq: u64, method: &str, path: &str) -> String {
    let mac = keys.request_mac(client, seq, method, path);
    format!("Tugboat {client}.{seq}.{}", crypto::b64(&mac))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHONE: &str = "phoneAAAAAAAAAAAAAAAAA";
    const OTHER: &str = "otherBBBBBBBBBBBBBBBBB";

    fn keys() -> Keys {
        Keys::derive(&[9u8; 16])
    }

    #[test]
    fn first_valid_request_binds_the_session() {
        let k = keys();
        let mut a = Auth::new();
        assert!(!a.bound());
        let h = header(&k, PHONE, 1000, "GET", "/api/state");
        let ok = a.check(&k, Some(&h), "GET", "/api/state").unwrap();
        assert!(ok.first);
        assert!(a.bound());
        let h = header(&k, PHONE, 1001, "GET", "/api/state");
        assert!(!a.check(&k, Some(&h), "GET", "/api/state").unwrap().first);
        // A second phone with the same secret is turned away.
        let h = header(&k, OTHER, 1002, "GET", "/api/state");
        assert_eq!(a.check(&k, Some(&h), "GET", "/api/state"), Err(Rejected::OtherDevice));
    }

    #[test]
    fn bad_requests_never_bind() {
        let k = keys();
        let mut a = Auth::new();
        assert_eq!(a.check(&k, None, "GET", "/api/state"), Err(Rejected::Malformed));
        assert_eq!(
            a.check(&k, Some("Bearer x"), "GET", "/api/state"),
            Err(Rejected::Malformed)
        );
        assert_eq!(
            a.check(&k, Some("Tugboat a.b.c"), "GET", "/api/state"),
            Err(Rejected::Malformed)
        );
        // Signed with another secret.
        let wrong = header(&Keys::derive(&[1u8; 16]), OTHER, 5, "GET", "/api/state");
        assert_eq!(a.check(&k, Some(&wrong), "GET", "/api/state"), Err(Rejected::BadMac));
        // Signed for another path or method.
        let h = header(&k, OTHER, 6, "GET", "/api/state");
        assert_eq!(a.check(&k, Some(&h), "POST", "/api/state"), Err(Rejected::BadMac));
        assert_eq!(a.check(&k, Some(&h), "GET", "/api/text"), Err(Rejected::BadMac));
        assert!(!a.bound());
        // The real phone can still claim it afterwards.
        let h = header(&k, PHONE, 7, "GET", "/api/state");
        assert!(a.check(&k, Some(&h), "GET", "/api/state").unwrap().first);
    }

    #[test]
    fn replays_are_refused_but_reordering_is_fine() {
        let k = keys();
        let mut a = Auth::new();
        let base = 1_800_000_000_000 * 1024;
        let req = |a: &mut Auth, seq: u64| {
            let h = header(&k, PHONE, seq, "PUT", "/api/up/x/1");
            a.check(&k, Some(&h), "PUT", "/api/up/x/1")
        };
        assert!(req(&mut a, base + 10).is_ok());
        assert_eq!(req(&mut a, base + 10), Err(Rejected::Replayed));
        // Parallel requests can land out of order within the window.
        assert!(req(&mut a, base + 30).is_ok());
        assert!(req(&mut a, base + 20).is_ok());
        assert_eq!(req(&mut a, base + 20), Err(Rejected::Replayed));
        // Far older than the newest: refused even if never seen.
        assert!(req(&mut a, base + 30 + 2 * REPLAY_WINDOW).is_ok());
        assert_eq!(req(&mut a, base + 25), Err(Rejected::Replayed));
        // The seen set doesn't grow without bound.
        assert!(a.seen.len() <= 2);
    }
}
