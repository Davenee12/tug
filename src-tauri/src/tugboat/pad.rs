//! The phone as a game controller (Tugboat Run and Harbor Stack): the input channel's rules, kept
//! pure so they're tested without a socket.
//!
//! While tug's game has asked for a controller (`Session::pad_open`), the phone bound to the
//! Tugboat session may `POST /api/pad`. The request is signed like every Tugboat request (same
//! secret, same MAC, same replay window, first phone binds) and its body is sealed to that request
//! like every other JSON body. Inside is the whole vocabulary an input has:
//!
//! ```json
//! {"steer": -100..=100, "boost": true|false, "drop"?: true|false, "age"?: 0..=10000, "rtt"?: 0..=10000}
//! ```
//!
//! `steer` is where the phone's slider sits (full left to full right; in Harbor Stack it nudges the
//! crane slower or faster), `boost` whether Boost is held (Tugboat Run's hop) and `drop` whether
//! Drop is held (Harbor Stack's). Which game is open is the PC's to say (`GameId`, chosen when the
//! game opens the channel): the page hears it in `/api/state` and in every reply, so it shows that
//! game's buttons, and an action that isn't the open game's is dropped before the game sees it. `age` and `rtt` are an optional latency probe, in milliseconds: how long the newest touch
//! waited on the phone before this input went out, and the phone's last round trip. They're only
//! counted for a per-session summary (logged at debug level when the controller closes); nothing
//! acts on them.
//!
//! Anything else (another field, a number out of range, a bigger body) is refused. An input
//! never reaches the files, the clipboard or the panel: it only updates the one `PadInput` the
//! game reads, which steers a boat or drops a container. The phone sends at most ~60 inputs a
//! second (up to two in flight, the newest state wins) plus a heartbeat, so a token bucket caps
//! the rate and silence means the phone has gone.
//!
//! A two-player game has two seats: seat 1 is the phone the session belongs to, seat 2 the one
//! guest phone `auth.rs` lets join while the game asks for two. Each seat has its own ordering,
//! rate limit, silence watch, reply and latency summary, so one phone can't slow or steer the other.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::crypto::OVERHEAD;

/// The longest input plaintext accepted.
/// `{"steer":-100,"boost":false,"drop":false,"age":10000,"rtt":10000}` is 65 bytes; a little room
/// for whitespace, no room for anything else.
pub const MAX_PLAIN: usize = 80;
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
    /// Harbor Stack's Drop. Absent means not held, so Tugboat Run's inputs are exactly what they
    /// always were.
    drop: Option<bool>,
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

/// One controller state: steering and whether boost (or drop) is held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PadInput {
    /// -100 (full left) to 100 (full right).
    pub steer: i16,
    pub boost: bool,
    pub drop: bool,
}

/// Which of tug's games has the channel open. The page shows that game's controls; an input can
/// only ever carry that game's action (`allow`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GameId {
    /// Steer and Boost (hop).
    #[default]
    TugboatRun,
    /// Nudge the crane and Drop.
    HarborStack,
}

impl GameId {
    /// The input as this game may see it: Tugboat Run never sees a drop, Harbor Stack never a
    /// boost. (A phone still showing the other game's buttons gets the right ones from its next
    /// reply.)
    pub fn allow(self, input: PadInput) -> PadInput {
        match self {
            GameId::TugboatRun => PadInput { drop: false, ..input },
            GameId::HarborStack => PadInput { boost: false, ..input },
        }
    }
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
            drop: wire.drop.unwrap_or(false),
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

/// Which phone: 1 is the phone the session belongs to, 2 the guest of a two-player game.
pub type SeatNo = u8;
pub const FIRST: SeatNo = 1;
pub const SECOND: SeatNo = 2;

/// What the game sees (the `game-pad` event). Mirrored in `src/types/protocol.ts` as `GamePad`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PadEvent {
    /// Which phone this is about.
    pub seat: SeatNo,
    /// That phone is sending inputs right now.
    pub connected: bool,
    /// -1 (full left) to 1 (full right); 0 whenever not connected.
    pub steer: f32,
    pub boost: bool,
    /// Harbor Stack's Drop is held (always false in Tugboat Run).
    pub drop: bool,
}

impl PadEvent {
    /// The phone in `seat` is gone (nothing held, nothing steering).
    pub fn gone(seat: SeatNo) -> PadEvent {
        PadEvent {
            seat,
            connected: false,
            steer: 0.0,
            boost: false,
            drop: false,
        }
    }
}

/// What the PC tells a phone in each reply, so it can say "Paused", buzz on a hit and, in a
/// two-player game, show which player it is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PadReply {
    pub paused: bool,
    /// Times this phone's boat has been hit (or its container missed the stack) this run; the phone
    /// buzzes when it goes up.
    pub hits: u32,
    /// The player this phone is in a two-player game (1 or 2, its colour); absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player: Option<u8>,
    /// The game being played, so the phone shows its buttons. Filled in by the channel
    /// (`Pad::reply`), never taken from the game's feedback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<GameId>,
}

/// One phone's side of the channel.
#[derive(Debug)]
struct Seat {
    /// The newest request sequence number applied. Requests run in parallel and can land out of
    /// order; an older input arriving late would steer backwards in time, so it's ignored.
    last_seq: u64,
    input: PadInput,
    last_input: Option<Instant>,
    connected: bool,
    bucket: Bucket,
    reply: PadReply,
    latency: Latency,
    /// Inputs since the last summary (`Pad::summary`), those that arrived after a newer one, and
    /// presses of the game's button (Boost or Drop going down) among them.
    received: u32,
    late: u32,
    presses: u32,
}

impl Seat {
    fn new(now: Instant) -> Seat {
        Seat {
            last_seq: 0,
            input: PadInput::default(),
            last_input: None,
            connected: false,
            bucket: Bucket::new(now),
            reply: PadReply::default(),
            latency: Latency::default(),
            received: 0,
            late: 0,
            presses: 0,
        }
    }
}

/// How often the controller's traffic is summarised in the debug log (never per input).
pub const SUMMARY_EVERY: Duration = Duration::from_secs(1);

/// One open controller channel: one seat, or two in a two-player game.
#[derive(Debug)]
pub struct Pad {
    players: u8,
    game: GameId,
    seats: [Seat; 2],
    /// When the traffic was last summarised.
    summary_at: Instant,
}

fn index(seat: SeatNo) -> usize {
    usize::from(seat == SECOND)
}

impl Pad {
    pub fn new(now: Instant, players: u8, game: GameId) -> Pad {
        Pad {
            players: players.clamp(1, 2),
            game,
            seats: [Seat::new(now), Seat::new(now)],
            summary_at: now,
        }
    }

    /// At most once every `SUMMARY_EVERY`: one line per phone that sent anything since the last
    /// summary ("controller 1: inputs 58, late 0, presses 2 in 1.0 s"), for the debug log, so a
    /// controller that doesn't seem to work can be told apart from one whose inputs never arrive.
    /// Counts only: never what was steered.
    pub fn summary(&mut self, now: Instant) -> Vec<String> {
        let span = now.saturating_duration_since(self.summary_at);
        if span < SUMMARY_EVERY {
            return Vec::new();
        }
        self.summary_at = now;
        let mut lines = Vec::new();
        for (i, s) in self.seats.iter_mut().enumerate() {
            if s.received > 0 {
                lines.push(format!(
                    "controller {}: inputs {}, late {}, presses {} in {:.1} s",
                    i + 1,
                    s.received,
                    s.late,
                    s.presses,
                    span.as_secs_f64()
                ));
            }
            s.received = 0;
            s.late = 0;
            s.presses = 0;
        }
        lines
    }

    /// The game that has the channel open.
    pub fn game(&self) -> GameId {
        self.game
    }

    /// Another game took the channel over (one game closed and the other opened before the phones
    /// noticed): inputs are read as that game's from now on, and the replies say so.
    pub fn set_game(&mut self, game: GameId) {
        if self.game == game {
            return;
        }
        self.game = game;
        for s in &mut self.seats {
            s.input = game.allow(s.input);
            s.reply = PadReply::default();
        }
    }

    /// How many phones the game asks for (1, or 2 for a two-player game).
    pub fn players(&self) -> u8 {
        self.players
    }

    /// Whether `seat` is open: seat 2 only in a two-player game.
    pub fn open_for(&self, seat: SeatNo) -> bool {
        seat == FIRST || (seat == SECOND && self.players == 2)
    }

    /// The game changed how many phones it wants. Going back to one closes seat 2: returns the
    /// event saying its phone is gone, if it was there.
    pub fn set_players(&mut self, players: u8, now: Instant) -> Option<PadEvent> {
        self.players = players.clamp(1, 2);
        if self.players == 2 {
            return None;
        }
        let was = std::mem::replace(&mut self.seats[1], Seat::new(now));
        was.connected.then(|| PadEvent::gone(SECOND))
    }

    /// Whether another input from `seat` may come in now (its own rate limit).
    pub fn admit(&mut self, seat: SeatNo, now: Instant) -> bool {
        self.seats[index(seat)].bucket.take(now)
    }

    /// Apply an authenticated input from `seat`. Returns the event to send the game when anything
    /// it shows changed (steering, boost, or the phone just arriving).
    pub fn input(&mut self, seat: SeatNo, seq: u64, decoded: Decoded, now: Instant) -> Option<PadEvent> {
        let s = &mut self.seats[index(seat)];
        s.received = s.received.saturating_add(1);
        if seq <= s.last_seq {
            s.late = s.late.saturating_add(1);
            return None;
        }
        if let Some(p) = decoded.probe {
            s.latency.add(p.touch_to_pc_ms());
        }
        let input = self.game.allow(decoded.input);
        let held = |i: PadInput| i.boost || i.drop;
        if held(input) && !(s.connected && held(s.input)) {
            s.presses = s.presses.saturating_add(1);
        }
        s.last_seq = seq;
        s.last_input = Some(now);
        let changed = !s.connected || input != s.input;
        s.connected = true;
        s.input = input;
        changed.then(|| self.event(seat))
    }

    /// Called a few times a second while the channel is open: after `QUIET` without an input a
    /// phone has gone (locked, left the page, dropped off the Wi-Fi). Returns the events to send.
    pub fn tick(&mut self, now: Instant) -> Vec<PadEvent> {
        let mut gone = Vec::new();
        for (i, s) in self.seats.iter_mut().enumerate() {
            let quiet = s.last_input.is_none_or(|t| now.saturating_duration_since(t) >= QUIET);
            if s.connected && quiet {
                s.connected = false;
                s.input = PadInput::default();
                gone.push(PadEvent::gone(if i == 0 { FIRST } else { SECOND }));
            }
        }
        gone
    }

    /// The session moved to a new address and the next phones to scan bind it: their sequence
    /// numbers come from their own clocks, so start ordering afresh. Replay protection is still
    /// `auth.rs`'s job and carries on.
    pub fn rebound(&mut self) {
        for s in &mut self.seats {
            s.last_seq = 0;
        }
    }

    /// A new phone took seat 2 (the last guest went quiet): order its inputs afresh.
    pub fn rebound_seat(&mut self, seat: SeatNo) {
        self.seats[index(seat)].last_seq = 0;
    }

    pub fn connected(&self, seat: SeatNo) -> bool {
        self.seats[index(seat)].connected
    }

    pub fn event(&self, seat: SeatNo) -> PadEvent {
        let s = &self.seats[index(seat)];
        if s.connected {
            PadEvent {
                seat,
                connected: true,
                steer: f32::from(s.input.steer) / f32::from(STEER_MAX),
                boost: s.input.boost,
                drop: s.input.drop,
            }
        } else {
            PadEvent::gone(seat)
        }
    }

    pub fn set_reply(&mut self, seat: SeatNo, reply: PadReply) {
        self.seats[index(seat)].reply = reply;
    }

    /// What `seat`'s phone hears back: the game's feedback, and which game it is.
    pub fn reply(&self, seat: SeatNo) -> PadReply {
        PadReply {
            game: Some(self.game),
            ..self.seats[index(seat)].reply
        }
    }

    /// Touch-to-PC latency so far this session for `seat` (from its phone's probe).
    pub fn latency(&self, seat: SeatNo) -> &Latency {
        &self.seats[index(seat)].latency
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An input without a probe.
    fn d(steer: i16, boost: bool) -> Decoded {
        Decoded {
            input: PadInput {
                steer,
                boost,
                drop: false,
            },
            probe: None,
        }
    }

    #[test]
    fn decodes_steer_boost_and_the_optional_probe() {
        assert_eq!(decode(br#"{"steer":-100,"boost":false}"#), Ok(d(-100, false)));
        assert_eq!(decode(br#"{ "boost": true, "steer": 37 }"#), Ok(d(37, true)));
        let probed = decode(br#"{"steer":5,"boost":false,"age":3,"rtt":12}"#).unwrap();
        assert_eq!(
            probed.input,
            PadInput {
                steer: 5,
                boost: false,
                drop: false
            }
        );
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
        let mut p = Pad::new(t0, 1, GameId::TugboatRun);
        assert!(!p.connected(FIRST));
        // The first input connects the phone.
        let e = p.input(FIRST, 10, d(-60, false), t0).unwrap();
        assert!(e.connected && e.seat == FIRST);
        assert!((e.steer + 0.6).abs() < 1e-6);
        // The same state again (a heartbeat) changes nothing the game needs to hear about.
        assert_eq!(p.input(FIRST, 11, d(-60, false), t0), None);
        // A newer state does.
        assert!(p.input(FIRST, 13, d(-60, true), t0).unwrap().boost);
        // An older one landing late (two in flight) is ignored, not applied.
        assert_eq!(p.input(FIRST, 12, d(0, false), t0), None);
        assert_eq!(p.event(FIRST).steer, -0.6);
        assert!(p.event(FIRST).boost);
    }

    #[test]
    fn silence_disconnects_and_zeroes_the_steering() {
        let t0 = Instant::now();
        let mut p = Pad::new(t0, 1, GameId::TugboatRun);
        assert!(p.tick(t0 + QUIET * 2).is_empty()); // never connected: nothing to say
        p.input(FIRST, 1, d(100, true), t0);
        assert!(p.tick(t0 + QUIET / 2).is_empty());
        assert_eq!(p.tick(t0 + QUIET), vec![PadEvent::gone(FIRST)]);
        assert_eq!(
            PadEvent::gone(FIRST),
            PadEvent {
                seat: FIRST,
                connected: false,
                steer: 0.0,
                boost: false,
                drop: false
            }
        );
        // Said once.
        assert!(p.tick(t0 + QUIET * 3).is_empty());
        // The phone coming back reconnects, even with the same input as before it went.
        let back = p.input(FIRST, 2, d(100, true), t0 + QUIET * 4).unwrap();
        assert!(back.connected);
        assert_eq!(back.steer, 1.0);
    }

    #[test]
    fn a_new_phone_after_a_rebind_starts_ordering_afresh() {
        let t0 = Instant::now();
        let mut p = Pad::new(t0, 2, GameId::TugboatRun);
        p.input(FIRST, 5_000, d(10, false), t0);
        p.input(SECOND, 7_000, d(10, false), t0);
        // Phones whose clocks are behind the first ones'.
        p.rebound();
        assert!(p.input(FIRST, 10, d(20, false), t0).is_some());
        assert!(p.input(SECOND, 10, d(20, false), t0).is_some());
        // A new guest in seat 2 only resets seat 2.
        p.rebound_seat(SECOND);
        assert!(p.input(SECOND, 5, d(30, false), t0).is_some());
        assert_eq!(p.input(FIRST, 9, d(30, false), t0), None);
    }

    #[test]
    fn two_seats_are_kept_apart() {
        let t0 = Instant::now();
        let mut p = Pad::new(t0, 2, GameId::TugboatRun);
        assert!(p.open_for(FIRST) && p.open_for(SECOND));
        // Each phone steers its own seat, with its own ordering.
        let a = p.input(FIRST, 100, d(-50, false), t0).unwrap();
        let b = p.input(SECOND, 3, d(80, true), t0).unwrap();
        assert_eq!((a.seat, b.seat), (FIRST, SECOND));
        assert!((p.event(FIRST).steer + 0.5).abs() < 1e-6);
        assert!(p.event(SECOND).boost && !p.event(FIRST).boost);
        // Seat 2's low sequence numbers are its own: not "late" next to seat 1's.
        assert!(p.input(SECOND, 4, d(0, false), t0).is_some());
        // Each has its own rate limit: one phone flooding doesn't starve the other.
        while p.admit(SECOND, t0) {}
        assert!(p.admit(FIRST, t0));
        // Each has its own reply (its player and its hits).
        p.set_reply(
            FIRST,
            PadReply {
                paused: false,
                hits: 1,
                player: Some(2),
                game: None,
            },
        );
        p.set_reply(
            SECOND,
            PadReply {
                paused: false,
                hits: 0,
                player: Some(1),
                game: None,
            },
        );
        assert_eq!(p.reply(FIRST).player, Some(2));
        assert_eq!(p.reply(SECOND).hits, 0);
        // Silence is per phone: seat 2 keeps talking, seat 1 goes quiet.
        p.input(SECOND, 5, d(10, false), t0 + QUIET);
        assert_eq!(p.tick(t0 + QUIET + QUIET / 2), vec![PadEvent::gone(FIRST)]);
        assert!(p.connected(SECOND));
        assert_eq!(p.tick(t0 + QUIET * 2), vec![PadEvent::gone(SECOND)]);
    }

    #[test]
    fn going_back_to_one_player_closes_seat_two() {
        let t0 = Instant::now();
        let mut p = Pad::new(t0, 1, GameId::TugboatRun);
        assert!(!p.open_for(SECOND));
        assert_eq!(p.set_players(2, t0), None);
        assert!(p.open_for(SECOND));
        p.input(SECOND, 1, d(40, false), t0);
        // The game said one player again: seat 2's phone is gone, and it starts over if it returns.
        assert_eq!(p.set_players(1, t0), Some(PadEvent::gone(SECOND)));
        assert!(!p.open_for(SECOND) && !p.connected(SECOND));
        assert_eq!(p.set_players(1, t0), None, "said once");
        // Anything else is clamped to one or two.
        p.set_players(9, t0);
        assert_eq!(p.players(), 2);
        assert_eq!(Pad::new(t0, 0, GameId::TugboatRun).players(), 1);
    }

    #[test]
    fn the_reply_names_the_player_only_in_a_two_player_game() {
        let solo = PadReply {
            paused: true,
            hits: 2,
            player: None,
            game: None,
        };
        assert_eq!(serde_json::to_string(&solo).unwrap(), r#"{"paused":true,"hits":2}"#);
        let duo = PadReply {
            player: Some(2),
            ..solo
        };
        assert_eq!(
            serde_json::to_string(&duo).unwrap(),
            r#"{"paused":true,"hits":2,"player":2}"#
        );
        assert_eq!(
            serde_json::to_string(&PadEvent::gone(SECOND)).unwrap(),
            r#"{"seat":2,"connected":false,"steer":0.0,"boost":false,"drop":false}"#
        );
    }

    #[test]
    fn the_reply_names_the_game_so_the_phone_shows_its_buttons() {
        let t0 = Instant::now();
        let mut p = Pad::new(t0, 1, GameId::TugboatRun);
        p.set_reply(
            FIRST,
            PadReply {
                paused: true,
                hits: 1,
                player: None,
                game: None,
            },
        );
        assert_eq!(
            serde_json::to_string(&p.reply(FIRST)).unwrap(),
            r#"{"paused":true,"hits":1,"game":"tugboat-run"}"#
        );
        // The game's feedback can't claim to be another game: the channel says which it is.
        p.set_reply(
            FIRST,
            PadReply {
                game: Some(GameId::HarborStack),
                ..PadReply::default()
            },
        );
        assert_eq!(p.reply(FIRST).game, Some(GameId::TugboatRun));
        // Another game takes the channel over: its name from the next reply, and its old feedback
        // (another game's hits) is forgotten.
        p.set_game(GameId::HarborStack);
        assert_eq!(
            serde_json::to_string(&p.reply(FIRST)).unwrap(),
            r#"{"paused":false,"hits":0,"game":"harbor-stack"}"#
        );
        assert_eq!(
            serde_json::from_str::<GameId>(r#""harbor-stack""#).unwrap(),
            GameId::HarborStack
        );
        assert!(serde_json::from_str::<GameId>(r#""snake""#).is_err());
    }

    #[test]
    fn drop_is_optional_so_tugboat_runs_inputs_are_unchanged() {
        // Exactly what Tugboat Run's controller has always sent.
        assert_eq!(decode(br#"{"steer":-40,"boost":true}"#), Ok(d(-40, true)));
        // Harbor Stack's Drop.
        let dropped = decode(br#"{"steer":0,"boost":false,"drop":true}"#).unwrap();
        assert!(dropped.input.drop && !dropped.input.boost);
        // The longest legal input, with every field, fits.
        assert!(decode(br#"{"steer":-100,"boost":false,"drop":false,"age":10000,"rtt":10000}"#).is_ok());
        // Drop must be a plain true or false.
        for bad in [
            &br#"{"steer":0,"boost":false,"drop":1}"#[..],
            br#"{"steer":0,"boost":false,"drop":"yes"}"#,
            br#"{"steer":0,"boost":false,"drop":null,"stack":3}"#,
        ] {
            assert_eq!(
                decode(bad),
                Err(PadError::Malformed),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
    }

    /// The phone page's own bodies (asserted against its encoder by vitest), read by the PC.
    const VECTOR: &str = include_str!("../../../src/tugboat-page/padVector.json");

    #[test]
    fn the_pages_bodies_reach_the_game_drop_and_all() {
        let v: serde_json::Value = serde_json::from_str(VECTOR).unwrap();
        let t0 = Instant::now();
        for c in v["cases"].as_array().unwrap() {
            let name = c["name"].as_str().unwrap();
            let game: GameId = serde_json::from_value(c["game"].clone()).unwrap();
            let decoded = decode(c["body"].as_str().unwrap().as_bytes()).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            let steer = (c["steer"].as_f64().unwrap() * 100.0).round() as i16;
            assert_eq!(decoded.input.steer, steer, "{name}");
            // What the game hears: its own button, pressed or not.
            let mut pad = Pad::new(t0, 1, game);
            let e = pad.input(FIRST, 1, decoded, t0).unwrap();
            assert_eq!(e.drop, c["drop"].as_bool().unwrap(), "{name}");
            assert_eq!(e.boost, c["boost"].as_bool().unwrap(), "{name}");
        }
    }

    #[test]
    fn a_quick_taps_press_and_release_both_reach_harbor_stack() {
        let v: serde_json::Value = serde_json::from_str(VECTOR).unwrap();
        let body = |i: usize| decode(v["cases"][i]["body"].as_str().unwrap().as_bytes()).unwrap();
        let t0 = Instant::now();
        let mut pad = Pad::new(t0, 1, GameId::HarborStack);
        // A heartbeat, then the tap: the press, and (after its reply) the release.
        assert!(pad.input(FIRST, 1, body(1), t0).is_some_and(|e| e.connected && !e.drop));
        let press = pad.input(FIRST, 2, body(0), t0).unwrap();
        let release = pad.input(FIRST, 3, body(1), t0).unwrap();
        assert!(press.drop && !release.drop, "the game sees the drop go down and up");
        // Counted for the debug summary: three inputs, one press.
        assert_eq!(
            pad.summary(t0 + SUMMARY_EVERY),
            vec!["controller 1: inputs 3, late 0, presses 1 in 1.0 s".to_string()]
        );
    }

    #[test]
    fn the_traffic_summary_comes_at_most_once_a_second() {
        let t0 = Instant::now();
        let mut pad = Pad::new(t0, 2, GameId::HarborStack);
        let drop = |held| Decoded {
            input: PadInput {
                steer: 0,
                boost: false,
                drop: held,
            },
            probe: None,
        };
        pad.input(FIRST, 5, drop(true), t0);
        pad.input(FIRST, 4, drop(false), t0); // late: ignored, but counted
        pad.input(FIRST, 6, drop(true), t0); // still held: not another press
        pad.input(SECOND, 1, drop(false), t0);
        assert!(pad.summary(t0 + SUMMARY_EVERY / 2).is_empty(), "not yet");
        assert_eq!(
            pad.summary(t0 + SUMMARY_EVERY),
            vec![
                "controller 1: inputs 3, late 1, presses 1 in 1.0 s".to_string(),
                "controller 2: inputs 1, late 0, presses 0 in 1.0 s".to_string()
            ]
        );
        // Counts start again; a quiet second says nothing.
        assert!(pad.summary(t0 + SUMMARY_EVERY * 2).is_empty());
    }

    #[test]
    fn each_game_sees_only_its_own_action() {
        let t0 = Instant::now();
        let both = Decoded {
            input: PadInput {
                steer: 20,
                boost: true,
                drop: true,
            },
            probe: None,
        };
        // Tugboat Run: steering and Boost, never a drop.
        let mut run = Pad::new(t0, 1, GameId::TugboatRun);
        let e = run.input(FIRST, 1, both, t0).unwrap();
        assert!(e.boost && !e.drop);
        // Harbor Stack: the crane nudge and Drop, never a boost.
        let mut stack = Pad::new(t0, 2, GameId::HarborStack);
        let e = stack.input(SECOND, 1, both, t0).unwrap();
        assert!(e.drop && !e.boost);
        assert!((e.steer - 0.2).abs() < 1e-6);
        // A Boost from a phone still showing Tugboat Run's buttons changes nothing Harbor Stack sees.
        assert_eq!(stack.input(FIRST, 1, d(0, false), t0).map(|e| e.connected), Some(true));
        assert_eq!(stack.input(FIRST, 2, d(0, true), t0), None);
        // Switching games mid-hold lets go of the other game's action.
        run.set_game(GameId::HarborStack);
        assert!(!run.event(FIRST).boost && !run.event(FIRST).drop);
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
        // Only probed inputs count, per seat.
        let t0 = Instant::now();
        let mut p = Pad::new(t0, 2, GameId::TugboatRun);
        p.input(FIRST, 1, d(0, false), t0);
        let probed = Decoded {
            input: PadInput {
                steer: 1,
                boost: false,
                drop: false,
            },
            probe: Some(Probe { age_ms: 4, rtt_ms: 6 }),
        };
        p.input(FIRST, 2, probed, t0);
        assert_eq!(
            p.latency(FIRST).summary().unwrap(),
            "n=1, half within 7 ms, 90% within 7 ms, slowest 7 ms"
        );
        assert_eq!(p.latency(SECOND).summary(), None);
    }
}
