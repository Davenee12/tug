// The phone as Tugboat Run's controller: the pure parts. What an input says (where the slider
// sits and whether Boost is held, plus an optional latency probe), where a finger on the pad puts
// the slider, how often to send, what to do about an error, and whether tilt steering can work.

/** Send at most this often while the input is changing (~60 a second): a change goes out at once
 * unless one went less than this long ago, then at this mark. */
export const MIN_GAP_MS = 16;
/** Requests in flight at once. Two, so a slow reply doesn't hold up the next input; the PC keeps
 * only the newest (by sequence number) if they land out of order. */
export const MAX_IN_FLIGHT = 2;
/** Send a heartbeat this often while nothing changes, so the PC knows the phone is still here. */
export const HEARTBEAT_MS = 200;
/** After "busy" from the PC, wait this long before the next input. */
export const BUSY_BACKOFF_MS = 250;

export interface PadInput {
  /** Where the slider sits: -100 (full left) to 100 (full right), a whole number, exactly what the
   * PC accepts. */
  steer: number;
  boost: boolean;
}

/** The latency probe sent with an input that carries a new touch (milliseconds, whole, 0–10000). */
export interface PadProbe {
  /** How long the newest touch waited on the phone before this input went out. */
  age: number;
  /** The last round trip to the PC. */
  rtt: number;
}

/** A probe the PC will accept: whole milliseconds, 0 to 10 s. */
export function probe(ageMs: number, rttMs: number): PadProbe {
  const ms = (v: number) => (Number.isFinite(v) ? Math.min(10_000, Math.max(0, Math.round(v))) : 0);
  return { age: ms(ageMs), rtt: ms(rttMs) };
}

/** Steering in [-1, 1] → what goes on the wire. Out of range or not a number becomes safe. */
export function encodePad(steer: number, boost: boolean): PadInput {
  const s = Number.isFinite(steer) ? Math.max(-1, Math.min(1, steer)) : 0;
  // `|| 0` turns -0 into 0, so the JSON never says "-0".
  return { steer: Math.round(s * 100) || 0, boost: boost === true };
}

export function samePad(a: PadInput | null, b: PadInput): boolean {
  return a !== null && a.steer === b.steer && a.boost === b.boost;
}

/**
 * Where a finger on the pad puts the slider: straight across, left edge to right edge, with a thin
 * margin at each side so full left and full right are easy to reach. The slider stays where it was
 * let go, like a real one, and the boat follows it.
 */
export function positionFromTouch(x: number, width: number, margin = 0.06): number {
  if (!(width > 0) || !Number.isFinite(x)) return 0;
  const t = (x / width - margin) / (1 - 2 * margin);
  return Math.max(-1, Math.min(1, t * 2 - 1));
}

/** How long to wait before the next send: soon when the input changed, a heartbeat otherwise. */
export function nextSendDelay(sinceLastMs: number, changed: boolean): number {
  return Math.max(0, (changed ? MIN_GAP_MS : HEARTBEAT_MS) - sinceLastMs);
}

/**
 * What a failed input means for the controller. "leave": the game closed or the session's over,
 * so go back to the Tugboat page. "slow": the PC asked for fewer. "retry": a dropped packet or a
 * stale sequence number; carry on.
 */
export function padErrorAction(code: string): "leave" | "slow" | "retry" {
  if (["no-game", "closed", "unauthorized", "in-use"].includes(code)) return "leave";
  if (code === "busy") return "slow";
  return "retry";
}

/**
 * Whether tilt steering can work here. Safari (iOS 13+) and Chrome only give a page the phone's
 * orientation in a secure context (HTTPS), and Tugboat's page is plain HTTP on your Wi-Fi, so on
 * Tugboat this is "needs-secure" and the controller says so instead of offering a switch that
 * can't work.
 */
export function tiltSupport(env: { secure: boolean; hasOrientation: boolean }): "available" | "needs-secure" | "unsupported" {
  if (!env.hasOrientation) return env.secure ? "unsupported" : "needs-secure";
  return env.secure ? "available" : "needs-secure";
}
