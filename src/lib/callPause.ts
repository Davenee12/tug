// Pause music and videos on this PC while the iPhone rings, and play them again afterwards. Which
// sessions to pause, and whether one is still as tug left it, is decided in Rust
// (src-tauri/src/pc_media); this decides *when*, from what the phone tells tug about the call.
//
// What tug can see: ANCS shows a ringing call as an incomingCall notification and removes it when
// the call stops ringing, whichever way it stopped (answered, declined, rang out). A call that was
// missed then comes back as a missedCall notification within a moment. Nothing on any of the
// phone's Bluetooth services says when an answered call ends: ANCS has no notification for a call
// in progress, and the recent calls list isn't a reliable clock for it. So:
//
// - Declined in tug, or a missed call follows: play again at once (nobody is talking).
// - Answered in tug, or the ringing stopped and no missed call came within MISSED_WAIT_MS: treat
//   it as answered. tug can't see the hang-up, and starting music in someone's ear mid-call is
//   worse than leaving it paused, so it doesn't play anything by itself: it offers Resume instead,
//   and whatever the person starts themselves is theirs (Rust never resumes what they touched).
//
// Pure: the store passes the time and carries out the step.

/** How long after the ringing stops a missed-call notification can still say it was missed. */
export const MISSED_WAIT_MS = 10_000;
/** How long Resume stays on offer after an answered call; after that tug lets go. */
export const OFFER_MS = 2 * 60 * 60 * 1000;

export type CallPause =
  | { phase: "idle" }
  /** A call is ringing and tug asked Windows to pause what was playing. */
  | { phase: "ringing"; call: number }
  /** The ringing stopped; waiting until `until` for a missed call before treating it as answered. */
  | { phase: "settling"; until: number }
  /** Answered (or so it seems): Resume is on offer until `until`. */
  | { phase: "answered"; until: number };

export const IDLE: CallPause = { phase: "idle" };

/** What the store does next: pause playing sessions, play tug's paused ones again, offer Resume, or let them go. */
export type CallPauseStep = "pause" | "resume" | "offer" | "release" | null;

export interface CallPauseNext {
  state: CallPause;
  step: CallPauseStep;
}

/** How a ringing call stopped, as far as tug knows. */
export type RingEnd = "declined" | "answered" | "unknown";

/** A call started ringing on the phone. With the setting off nothing is paused (and nothing already paused is let go). */
export function ringStarted(s: CallPause, call: number, enabled: boolean): CallPauseNext {
  if (!enabled) return { state: s, step: null };
  if (s.phase === "ringing" && s.call === call) return { state: s, step: null };
  // A second call (call waiting) pauses anything that started since; Rust keeps what it paused before.
  return { state: { phase: "ringing", call }, step: "pause" };
}

/** The ringing call stopped ringing. */
export function ringStopped(s: CallPause, call: number, how: RingEnd, now: number): CallPauseNext {
  if (s.phase !== "ringing" || s.call !== call) return { state: s, step: null };
  if (how === "declined") return { state: IDLE, step: "resume" };
  if (how === "answered") return { state: { phase: "answered", until: now + OFFER_MS }, step: "offer" };
  return { state: { phase: "settling", until: now + MISSED_WAIT_MS }, step: null };
}

/**
 * A missed call arrived (new to tug, not replayed after a reconnect). It can come just before the
 * ringing call's removal, or after it, so it counts while ringing too.
 */
export function missedCall(s: CallPause): CallPauseNext {
  if (s.phase === "ringing" || s.phase === "settling") return { state: IDLE, step: "resume" };
  return { state: s, step: null };
}

/** Time passed: no missed call came, so it was answered; or Resume has been on offer long enough. */
export function tick(s: CallPause, now: number): CallPauseNext {
  if (s.phase === "settling" && now >= s.until) {
    return { state: { phase: "answered", until: now + OFFER_MS }, step: "offer" };
  }
  if (s.phase === "answered" && now >= s.until) return { state: IDLE, step: "release" };
  return { state: s, step: null };
}

/** When `tick` next has something to do, or null. */
export function nextTick(s: CallPause): number | null {
  return s.phase === "settling" || s.phase === "answered" ? s.until : null;
}

/** The person pressed Resume. */
export function resumePressed(s: CallPause): CallPauseNext {
  if (s.phase !== "answered") return { state: s, step: null };
  return { state: IDLE, step: "resume" };
}

/** What the Resume offer says (with a Resume button). */
export const OFFER_TEXT = "tug paused what was playing on this PC for your call.";
