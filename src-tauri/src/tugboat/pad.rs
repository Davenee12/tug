//! The phone as a game controller (Tugboat Run): the input channel's rules, kept pure so they're
//! tested without a socket.
//!
//! While tug's game has asked for a controller (`Session::pad_open`), the phone bound to the
//! Tugboat session may `POST /api/pad`. The request is signed like every Tugboat request (same
//! secret, same MAC, same replay window, first phone binds) and its body is sealed to that request
//! like every other JSON body. Inside is the whole vocabulary an input has:
//!
//! ```json
//! {"steer": -100..=100, "boost": true|false, "age"?: 0..=10000, "rtt"?: 0..=10000}
//! ```
//!
//! `steer` is where the phone's slider sits (full left to full right) and `boost` whether Boost is
//! held. `age` and `rtt` are an optional latency probe, in milliseconds: how long the newest touch
//! waited on the phone before this input went out, and the phone's last round trip. They're only
//! counted for a per-session summary (logged at debug level when the controller closes); nothing
//! acts on them.
//!
//! Anything else (another field, a number out of range, a bigger body) is refused. An input
//! never reaches the files, the clipboard or the panel: it only updates the one `PadInput` the
//! game reads, which steers a boat. The phone sends at most ~60 inputs a second (up to two in
//! flight, the newest state wins) plus a heartbeat, so a token bucket caps the rate and silence
//! means the phone has gone.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::crypto::OVERHEAD;

/// The longest input plaintext accepted. `{"steer":-100,"boost":false,"age":10000,"rtt":10000}` is
/// 52 bytes; a little room for whitespace, no room for anything else.
pub const MAX_PLAIN: usize = 64;
/// The longest sealed input body read off the network.
pub const MAX_BODY: usize = MAX_PLAIN + OVERHEAD;
/// Steering is a whole number from full left (-100) to full right (100).
pub const STEER_MAX: i16 = 100;
/// No input for this long and the phone counts as gone: the game drops its steering and says so.
/// The phone sends a heartbeat every 200 ms while it's on screen.
pub const QUIET: Duration = Duration::from_millis(700);
/// Inputs a second the bucket refills by. The phone sends at most ~60.
pub const RATE_PER_SEC: f64 = 90.0;
/// Inputs that may arrive back to back (a burst after the Wi-Fi hiccups).
pub const BURST: f64 = 30.0;
/// The latency probe's numbers are milliseconds up to this.
pub const PROBE_MAX_MS: u16 = 10_000;

/// What the phone says, exactly. Unknown fields are refused rather than ignored, so the channel
/// can't quietly grow a meaning.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    steer: i16,
    boost: bool,
    age: Option<u16>,
    rtt: Option<u16>,
}

/// The phone's latency probe for one input (see the module comment).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    /// How long the newest touch waited on the phone before this input was sent.
    pub age_ms: u16,
    /// The phone's last round trip to the PC.
    pub rtt_ms: u16,
}

impl Probe {
    /// Touch on the phone to arrival at the PC: the wait on the phone plus half a round trip.
    pub fn touch_to_pc_ms(self) -> u32 {
        u32::from(self.age_ms) + u32::from(self.rtt_ms) / 2
    }
}

/// One decoded input: the state the game reads, and the probe if the phone sent one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decoded {
    pub input: PadInput,
    pub probe: Option<Probe>,
}

/// One controller state: steering and whether boost is held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PadInput {
    /// -100 (full left) to 100 (full right).
    pub steer: i16,
    pub boost: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadError {
    TooBig,
    /// Not JSON, the wrong shape, or a field it doesn't know.
    Malformed,
    /// Steering beyond full lock, or a probe number out of range.
    OutOfRange,
}

/// Parse and check one input's plaintext.
pub fn decode(plain: &[u8]) -> Result<Decoded, PadError> {
    if plain.len() > MAX_PLAIN {
        return Err(PadError::TooBig);
    }
    // An object, by name: serde would otherwise also take `[steer, boost]`.
    let object: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(plain).map_err(|_| PadError::Malformed)?;
    let wire: Wire = serde_json::from_value(serde_json::Value::Object(object)).map_err(|_| PadError::Malformed)?;
    if !(-STEER_MAX..=STEER_MAX).contains(&wire.steer) {
        return Err(PadError::OutOfRange);
    }
    if wire.age.is_some_and(|v| v > PROBE_MAX_MS) || wire.rtt.is_some_and(|v| v > PROBE_MAX_MS) {
        return Err(PadError::OutOfRange);
    }
    Ok(Decoded {
        input: PadInput {
            steer: wire.steer,
            boost: wire.boost,
        },
        // Both or neither: half a probe can't say anything.
        probe: wire.age.zip(wire.rtt).map(|(age_ms, rtt_ms)| Probe { age_ms, rtt_ms }),
    })
}

/// Touch-to-PC latency over one controller session, in buckets so it costs a few bytes however long
/// the session runs. Only ever summarised (debug log, the manual test harness).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Latency {
    /// Inputs counted, per bucket (upper bounds in `BUCKETS_MS`, the last one open-ended).
    counts: [u32; BUCKETS_MS.len() + 1],
    max_ms: u32,
    total: u32,
}

const BUCKETS_MS: [u32; 9] = [5, 10, 15, 20, 30, 50, 75, 100, 150];

impl Latency {
    pub fn add(&mut self, ms: u32) {
        let i = BUCKETS_MS.iter().position(|&b| ms <= b).unwrap_or(BUCKETS_MS.len());
        self.counts[i] = self.counts[i].saturating_add(1);
        self.total = self.total.saturating_add(1);
        self.max_ms = self.max_ms.max(ms);
    }

    /// The bucket bound under which `p` of the inputs fell (`None` with no samples).
    pub fn percentile_ms(&self, p: f64) -> Option<u32> {
        if self.total == 0 {
            return None;
        }
        let want = (f64::from(self.total) * p).ceil().max(1.0) as u32;
        let mut seen = 0u32;
        for (i, n) in self.counts.iter().enumerate() {
            seen += n;
            if seen >= want {
                // Never claim a bound above the slowest input actually seen.
                return Some(BUCKETS_MS.get(i).copied().unwrap_or(self.max_ms).min(self.max_ms));
            }
        }
        Some(self.max_ms)
    }

    /// "n=412, half within 10 ms, 90% within 20 ms, slowest 34 ms"; `None` with no samples.
    pub fn summary(&self) -> Option<String> {
        Some(format!(
            "n={}, half within {} ms, 90% within {} ms, slowest {} ms",
            self.total,
            self.percentile_ms(0.5)?,
            self.percentile_ms(0.9)?,
            self.max_ms
        ))
    }
}

/// A token bucket: `RATE_PER_SEC` sustained, `BURST` at once.
#[derive(Debug, Clone, Copy)]
pub struct Bucket {
    tokens: f64,
    at: Instant,
}

impl Bucket {
    pub fn new(now: Instant) -> Bucket {
        Bucket { tokens: BURST, at: now }
    }

    /// Spend a token if there is one.
    pub fn take(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.at).as_secs_f64();
        self.tokens = (self.tokens + elapsed * RATE_PER_SEC).min(BURST);
        self.at = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// What the game sees (the `game-pad` event). Mirrored in `src/types/protocol.ts` as `GamePad`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PadEvent {
    /// A phone is sending inputs right now.
    pub connected: bool,
    /// -1 (full left) to 1 (full right); 0 whenever not connected.
    pub steer: f32,
    pub boost: bool,
}

/// What the PC tells the phone in each reply, so the phone can say "Paused" and buzz on a hit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PadReply {
    pub paused: bool,
    /// Times the boat has been hit this run; the phone buzzes when it goes up.
    pub hits: u32,
}

/// One open controller channel.
#[derive(Debug)]
pub struct Pad {
    /// The newest request sequence number applied. Requests run in parallel and can land out of
    /// order; an older input arriving late would steer backwards in time, so it's ignored.
    last_seq: u64,
    input: PadInput,
    last_input: Option<Instant>,
    connected: bool,
    bucket: Bucket,
    reply: PadReply,
    latency: Latency,
}

impl Pad {
    pub fn new(now: Instant) -> Pad {
        Pad {
            last_seq: 0,
            input: PadInput::default(),
            last_input: None,
            connected: false,
            bucket: Bucket::new(now),
            reply: PadReply::default(),
            latency: Latency::default(),
        }
    }

    /// Whether another input may come in now (rate limit).
    pub fn admit(&mut self, now: Instant) -> bool {
        self.bucket.take(now)
    }

    /// Apply an authenticated input. Returns the event to send the game when anything it shows
    /// changed (steering, boost, or the phone just arriving).
    pub fn input(&mut self, seq: u64, decoded: Decoded, now: Instant) -> Option<PadEvent> {
        if seq <= self.last_seq {
            return None;
        }
        if let Some(p) = decoded.probe {
            self.latency.add(p.touch_to_pc_ms());
        }
        let input = decoded.input;
        self.last_seq = seq;
        self.last_input = Some(now);
        let changed = !self.connected || input != self.input;
        self.connected = true;
        self.input = input;
        changed.then(|| self.event())
    }

    /// Called a few times a second while the channel is open: after `QUIET` without an input the
    /// phone has gone (locked, left the page, dropped off the Wi-Fi). Returns the event to send.
    pub fn tick(&mut self, now: Instant) -> Option<PadEvent> {
        let quiet = self
            .last_input
            .is_none_or(|t| now.saturating_duration_since(t) >= QUIET);
        if self.connected && quiet {
            self.connected = false;
            self.input = PadInput::default();
            return Some(self.event());
        }
        None
    }

    /// The session moved to a new address and the next phone to scan binds it: its sequence
    /// numbers come from its own clock, so start ordering afresh. Replay protection is still
    /// `auth.rs`'s job and carries on.
    pub fn rebound(&mut self) {
        self.last_seq = 0;
    }

    pub fn connected(&self) -> bool {
        self.connected
    }

    pub fn event(&self) -> PadEvent {
        if self.connected {
            PadEvent {
                connected: true,
                steer: f32::from(self.input.steer) / f32::from(STEER_MAX),
                boost: self.input.boost,
            }
        } else {
            PadEvent {
                connected: false,
                steer: 0.0,
                boost: false,
            }
        }
    }

    pub fn set_reply(&mut self, reply: PadReply) {
        self.reply = reply;
    }

    pub fn reply(&self) -> PadReply {
        self.reply
    }

    /// Touch-to-PC latency so far this session (from the phone's probe).
    pub fn latency(&self) -> &Latency {
        &self.latency
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An input without a probe.
    fn d(steer: i16, boost: bool) -> Decoded {
        Decoded {
            input: PadInput { steer, boost },
            probe: None,
        }
    }

    #[test]
    fn decodes_steer_boost_and_the_optional_probe() {
        assert_eq!(decode(br#"{"steer":-100,"boost":false}"#), Ok(d(-100, false)));
        assert_eq!(decode(br#"{ "boost": true, "steer": 37 }"#), Ok(d(37, true)));
        let probed = decode(br#"{"steer":5,"boost":false,"age":3,"rtt":12}"#).unwrap();
        assert_eq!(probed.input, PadInput { steer: 5, boost: false });
        assert_eq!(probed.probe, Some(Probe { age_ms: 3, rtt_ms: 12 }));
        assert_eq!(probed.probe.unwrap().touch_to_pc_ms(), 9);
        // The longest legal input fits.
        assert!(decode(br#"{"steer":-100,"boost":false,"age":10000,"rtt":10000}"#).is_ok());
        // Half a probe says nothing, and is ignored rather than refused.
        assert_eq!(decode(br#"{"steer":5,"boost":false,"age":3}"#).unwrap().probe, None);
    }

    #[test]
    fn refuses_anything_else() {
        // Beyond full lock, or probe numbers out of range.
        for bad in [
            &br#"{"steer":101,"boost":false}"#[..],
            br#"{"steer":-101,"boost":false}"#,
            br#"{"steer":0,"boost":false,"age":10001,"rtt":1}"#,
            br#"{"steer":0,"boost":false,"age":1,"rtt":10001}"#,
        ] {
            assert_eq!(
                decode(bad),
                Err(PadError::OutOfRange),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
        // Not a whole number, too big for the type, the wrong type, missing, or extra fields.
        for bad in [
            &br#"{"steer":0.5,"boost":false}"#[..],
            br#"{"steer":99999,"boost":false}"#,
            br#"{"steer":"1","boost":false}"#,
            br#"{"steer":1,"boost":1}"#,
            br#"{"steer":1}"#,
            br#"{"steer":1,"boost":false,"text":"hi"}"#,
            br#"{"steer":1,"boost":false,"path":"C:\\"}"#,
            br#"{"steer":1,"boost":false,"age":-1,"rtt":2}"#,
            br#"{"steer":1,"boost":false,"age":1.5,"rtt":2}"#,
            br#"[1,false]"#,
            b"",
            b"not json",
        ] {
            assert_eq!(
                decode(bad),
                Err(PadError::Malformed),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
        // Too long, whatever it says.
        let padded = format!(r#"{{"steer":1,"boost":false}}{}"#, " ".repeat(MAX_PLAIN));
        assert_eq!(decode(padded.as_bytes()), Err(PadError::TooBig));
    }

    #[test]
    fn rate_is_limited_but_a_steady_60_hz_gets_through() {
        let t0 = Instant::now();
        let mut b = Bucket::new(t0);
        // A burst is allowed, then refused.
        for _ in 0..BURST as usize {
            assert!(b.take(t0));
        }
        assert!(!b.take(t0));
        // A steady 60 a second, for a minute, always gets through.
        let mut b = Bucket::new(t0);
        for i in 1..=3600u64 {
            assert!(b.take(t0 + Duration::from_micros(i * 1_000_000 / 60)), "input {i}");
        }
        // A flood of 1,000 a second gets roughly the sustained rate, never more.
        let mut b = Bucket::new(t0);
        let allowed = (1..=1000u64).filter(|i| b.take(t0 + Duration::from_millis(*i))).count();
        assert!(allowed <= (RATE_PER_SEC + BURST) as usize + 1, "{allowed}");
        assert!(allowed >= RATE_PER_SEC as usize - 1, "{allowed}");
    }

    #[test]
    fn newest_input_wins_and_late_ones_are_ignored() {
        let t0 = Instant::now();
        let mut p = Pad::new(t0);
        assert!(!p.connected());
        // The first input connects the phone.
        let e = p.input(10, d(-60, false), t0).unwrap();
        assert!(e.connected);
        assert!((e.steer + 0.6).abs() < 1e-6);
        // The same state again (a heartbeat) changes nothing the game needs to hear about.
        assert_eq!(p.input(11, d(-60, false), t0), None);
        // A newer state does.
        assert!(p.input(13, d(-60, true), t0).unwrap().boost);
        // An older one landing late (two in flight) is ignored, not applied.
        assert_eq!(p.input(12, d(0, false), t0), None);
        assert_eq!(p.event().steer, -0.6);
        assert!(p.event().boost);
    }

    #[test]
    fn silence_disconnects_and_zeroes_the_steering() {
        let t0 = Instant::now();
        let mut p = Pad::new(t0);
        assert_eq!(p.tick(t0 + QUIET * 2), None); // never connected: nothing to say
        p.input(1, d(100, true), t0);
        assert_eq!(p.tick(t0 + QUIET / 2), None);
        let gone = p.tick(t0 + QUIET).unwrap();
        assert_eq!(
            gone,
            PadEvent {
                connected: false,
                steer: 0.0,
                boost: false
            }
        );
        // Said once.
        assert_eq!(p.tick(t0 + QUIET * 3), None);
        // The phone coming back reconnects, even with the same input as before it went.
        let back = p.input(2, d(100, true), t0 + QUIET * 4).unwrap();
        assert!(back.connected);
        assert_eq!(back.steer, 1.0);
    }

    #[test]
    fn a_new_phone_after_a_rebind_starts_ordering_afresh() {
        let t0 = Instant::now();
        let mut p = Pad::new(t0);
        p.input(5_000, d(10, false), t0);
        // A phone whose clock is behind the first one's.
        p.rebound();
        assert!(p.input(10, d(20, false), t0).is_some());
    }

    #[test]
    fn latency_is_summarised_in_buckets() {
        let mut l = Latency::default();
        assert_eq!(l.summary(), None);
        for ms in [2, 3, 4, 4, 6, 8, 9, 12, 25, 180] {
            l.add(ms);
        }
        assert_eq!(l.percentile_ms(0.5), Some(10));
        assert_eq!(l.percentile_ms(0.9), Some(30));
        assert_eq!(l.percentile_ms(1.0), Some(180));
        assert_eq!(
            l.summary().unwrap(),
            "n=10, half within 10 ms, 90% within 30 ms, slowest 180 ms"
        );
        // Only probed inputs count.
        let t0 = Instant::now();
        let mut p = Pad::new(t0);
        p.input(1, d(0, false), t0);
        let probed = Decoded {
            input: PadInput { steer: 1, boost: false },
            probe: Some(Probe { age_ms: 4, rtt_ms: 6 }),
        };
        p.input(2, probed, t0);
        assert_eq!(
            p.latency().summary().unwrap(),
            "n=1, half within 7 ms, 90% within 7 ms, slowest 7 ms"
        );
    }
}
