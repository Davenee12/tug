//! Who may drive a Tugboat session: the request-authentication state machine.
//!
//! Every API request carries `Authorization: Tugboat <client>.<seq>.<mac>`: a client id the page
//! made up, a sequence number that only goes up (the page starts it from the clock), and a MAC of
//! both plus the method and path under the session's auth key. The first request that passes
//! binds the session to that client; any other client gets "in use" even with the secret (a
//! second phone that scanned the same code). A sequence number seen before, or far older than
//! the newest, is a replay and is refused.
//!
//! The one exception is a two-player game: while tug's game asks for two controllers, one second
//! phone (a "guest") may join with the same secret and the same signing, so exactly two phones can
//! drive the session. The session (not this module) limits a guest to steering: it never sees files
//! or text. A third phone is still another device. A guest has its own replay window, since its
//! sequence numbers come from its own clock.

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
    /// True for the request that bound the session (the phone just connected), or that let a
    /// guest join.
    pub first: bool,
    /// The second phone of a two-player game, not the phone the session belongs to.
    pub guest: bool,
}

/// Whether a phone that isn't the session's own may join as the guest with this request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GuestPolicy {
    /// A two-player game is asking for a second controller, and this is a controller request.
    pub may_join: bool,
    /// The guest there is has gone quiet: a new phone may take its place.
    pub may_replace: bool,
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

/// The sequence numbers one client has used (see `REPLAY_WINDOW`).
#[derive(Default)]
struct Replay {
    newest: u64,
    seen: BTreeSet<u64>,
}

#[derive(Default)]
pub struct Auth {
    bound: Option<String>,
    replay: Replay,
    /// The second phone of a two-player game, and its own replay window.
    guest: Option<(String, Replay)>,
}

impl Auth {
    pub fn new() -> Auth {
        Auth::default()
    }

    #[cfg(test)]
    pub fn bound(&self) -> bool {
        self.bound.is_some()
    }

    #[cfg(test)]
    pub fn has_guest(&self) -> bool {
        self.guest.is_some()
    }

    /// Forget which phone the session belongs to, so the next one to scan the code binds it. Used
    /// when the advertised address changes: that's a new browser origin, so even the same phone
    /// comes back with a new client id. Replay state is kept, so nothing seen before works again.
    /// A guest goes too (it comes back with a new client id as well, and may join again).
    pub fn unbind(&mut self) {
        self.bound = None;
        self.guest = None;
    }

    /// Check a request from the session's own phone (no guest may join).
    #[cfg(test)]
    pub fn check(
        &mut self,
        keys: &Keys,
        header: Option<&str>,
        method: &str,
        path: &str,
    ) -> Result<Authorized, Rejected> {
        self.check_with(keys, header, method, path, GuestPolicy::default())
    }

    /// Check a request. The session's own phone as always; another phone is the guest if it
    /// already is, or if `policy` lets it join (or take a quiet guest's place); otherwise it's
    /// another device.
    pub fn check_with(
        &mut self,
        keys: &Keys,
        header: Option<&str>,
        method: &str,
        path: &str,
        policy: GuestPolicy,
    ) -> Result<Authorized, Rejected> {
        let (client, seq, mac) = parse(header.ok_or(Rejected::Malformed)?).ok_or(Rejected::Malformed)?;
        if !keys.verify_request_mac(client, seq, method, path, &mac) {
            return Err(Rejected::BadMac);
        }
        let ok = |first: bool, guest: bool| Authorized {
            client: client.to_string(),
            seq,
            first,
            guest,
        };
        match &self.bound {
            None => {
                if !self.replay.fresh(seq) {
                    return Err(Rejected::Replayed);
                }
                self.bound = Some(client.to_string());
                return Ok(ok(true, false));
            }
            Some(bound) if bound == client => {
                if !self.replay.fresh(seq) {
                    return Err(Rejected::Replayed);
                }
                return Ok(ok(false, false));
            }
            Some(_) => {}
        }
        if let Some((guest, replay)) = self.guest.as_mut() {
            if guest == client {
                return if replay.fresh(seq) {
                    Ok(ok(false, true))
                } else {
                    Err(Rejected::Replayed)
                };
            }
        }
        let may = match self.guest {
            None => policy.may_join,
            Some(_) => policy.may_join && policy.may_replace,
        };
        if !may {
            return Err(Rejected::OtherDevice);
        }
        let mut replay = Replay::default();
        replay.fresh(seq);
        self.guest = Some((client.to_string(), replay));
        Ok(ok(true, true))
    }
}

impl Replay {
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
        assert!(a.replay.seen.len() <= 2);
    }

    const THIRD: &str = "thirdCCCCCCCCCCCCCCCCC";
    const JOIN: GuestPolicy = GuestPolicy {
        may_join: true,
        may_replace: false,
    };

    #[test]
    fn a_two_player_game_lets_exactly_one_second_phone_join() {
        let k = keys();
        let mut a = Auth::new();
        let base = 1_800_000_000_000 * 1024;
        let req = |a: &mut Auth, client: &str, seq: u64, policy: GuestPolicy| {
            let h = header(&k, client, seq, "POST", "/api/pad");
            a.check_with(&k, Some(&h), "POST", "/api/pad", policy)
        };
        // The first phone binds the session the usual way, even when a guest could join.
        let first = req(&mut a, PHONE, base + 1, JOIN).unwrap();
        assert!(first.first && !first.guest);
        // Without a two-player game, a second phone is another device.
        assert_eq!(
            req(&mut a, OTHER, base + 2, GuestPolicy::default()),
            Err(Rejected::OtherDevice)
        );
        assert!(!a.has_guest(), "a refused phone never joins");
        // With one, it joins as the guest, once.
        let joined = req(&mut a, OTHER, base + 3, JOIN).unwrap();
        assert!(joined.first && joined.guest);
        let again = req(&mut a, OTHER, base + 4, JOIN).unwrap();
        assert!(!again.first && again.guest);
        // The guest stays itself even after the game stops asking (the session limits what it may do).
        assert!(req(&mut a, OTHER, base + 5, GuestPolicy::default()).unwrap().guest);
        // A third phone is turned away while the guest is there.
        assert_eq!(req(&mut a, THIRD, base + 6, JOIN), Err(Rejected::OtherDevice));
        // The session's own phone is unaffected.
        assert!(!req(&mut a, PHONE, base + 7, JOIN).unwrap().guest);
        // A wrong signature never joins, however the policy reads.
        let wrong = header(&Keys::derive(&[1u8; 16]), THIRD, base + 8, "POST", "/api/pad");
        assert_eq!(
            a.check_with(&k, Some(&wrong), "POST", "/api/pad", JOIN),
            Err(Rejected::BadMac)
        );
    }

    #[test]
    fn a_quiet_guest_gives_its_place_to_the_next_phone() {
        let k = keys();
        let mut a = Auth::new();
        let base = 1_800_000_000_000 * 1024;
        let req = |a: &mut Auth, client: &str, seq: u64, policy: GuestPolicy| {
            let h = header(&k, client, seq, "GET", "/api/state");
            a.check_with(&k, Some(&h), "GET", "/api/state", policy)
        };
        req(&mut a, PHONE, base + 1, JOIN).unwrap();
        req(&mut a, OTHER, base + 2, JOIN).unwrap();
        let replace = GuestPolicy {
            may_join: true,
            may_replace: true,
        };
        let third = req(&mut a, THIRD, base + 3, replace).unwrap();
        assert!(third.first && third.guest);
        // The old guest is now another device.
        assert_eq!(req(&mut a, OTHER, base + 4, JOIN), Err(Rejected::OtherDevice));
    }

    #[test]
    fn each_phone_has_its_own_replay_window() {
        let k = keys();
        let mut a = Auth::new();
        let base = 1_800_000_000_000 * 1024;
        let req = |a: &mut Auth, client: &str, seq: u64| {
            let h = header(&k, client, seq, "POST", "/api/pad");
            a.check_with(&k, Some(&h), "POST", "/api/pad", JOIN)
        };
        assert!(req(&mut a, PHONE, base + 10 * REPLAY_WINDOW).is_ok());
        // The second phone's clock is minutes behind the first's: still fine, it's its own window.
        assert!(req(&mut a, OTHER, base + 1).is_ok());
        assert!(req(&mut a, OTHER, base + 2).is_ok());
        // Its replays are refused, and its numbers don't open a window for the first phone.
        assert_eq!(req(&mut a, OTHER, base + 2), Err(Rejected::Replayed));
        assert_eq!(req(&mut a, PHONE, base + 3), Err(Rejected::Replayed));
        // An origin change lets go of the guest as well.
        a.unbind();
        assert!(!a.has_guest());
    }

    #[test]
    fn after_an_origin_change_the_next_phone_binds_and_replays_stay_refused() {
        let k = keys();
        let mut a = Auth::new();
        let base = 1_800_000_000_000 * 1024;
        let req = |a: &mut Auth, client: &str, seq: u64| {
            let h = header(&k, client, seq, "GET", "/api/state");
            a.check(&k, Some(&h), "GET", "/api/state")
        };
        assert!(req(&mut a, PHONE, base + 1).unwrap().first);
        // The address changed: whoever scans the new code next is the phone (a new origin means a
        // new client id, even for the same phone).
        a.unbind();
        assert!(!a.bound());
        assert!(req(&mut a, OTHER, base + 100).unwrap().first);
        // The old client is now the other device.
        assert_eq!(req(&mut a, PHONE, base + 200), Err(Rejected::OtherDevice));
        // Replay protection survived the unbind: a sequence number already used is still refused.
        assert_eq!(req(&mut a, OTHER, base + 1), Err(Rejected::Replayed));
    }
}
