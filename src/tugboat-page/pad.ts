// The phone as Tugboat Run's controller: the pure parts. What an input says (steering and boost,
// nothing else), where a finger on the pad steers, how often to send, what to do about an error,
// and whether tilt steering can work on this page.

/** Send at most this often while the input is changing (~30 a second). */
export const MIN_GAP_MS = 33;
/** Send a heartbeat this often while nothing changes, so the PC knows the phone is still here. */
export const HEARTBEAT_MS = 200;
/** After "busy" from the PC, wait this long before the next input. */
export const BUSY_BACKOFF_MS = 250;

export interface PadInput {
  /** -100 (full left) to 100 (full right), a whole number: exactly what the PC accepts. */
  steer: number;
  boost: boolean;
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
 * Where a finger on the steering pad steers: its distance from the middle, as a share of half the
 * pad's width, with a small dead zone so a resting thumb goes straight, reaching full lock a bit
 * before the edge.
 */
export function steerFromTouch(x: number, width: number, deadZone = 0.08): number {
  if (!(width > 0) || !Number.isFinite(x)) return 0;
  const offset = (x - width / 2) / (width / 2);
  const mag = Math.abs(offset);
  if (mag <= deadZone) return 0;
  const scaled = Math.min(1, (mag - deadZone) / (0.85 - deadZone));
  return Math.sign(offset) * scaled;
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
