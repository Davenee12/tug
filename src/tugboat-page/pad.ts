// The phone as a game's controller (Tugboat Run or Harbor Stack): the pure parts. What an input
// says (where the slider sits and whether Boost or Drop is held, plus an optional latency probe),
// which buttons each game gets, where a finger on the pad puts the slider, how often to send, what
// to do about an error, which layout fits the phone's orientation, and whether tilt steering can
// work.

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
  /** Harbor Stack's Drop. Never sent for Tugboat Run, so its inputs are exactly what they were. */
  drop?: boolean;
}

// --- Which game ---

/** The game on the PC (`tugboat/pad.rs` `GameId`). */
export type PadGame = "tugboat-run" | "harbor-stack";

/** The game the PC names (in `/api/state` or a reply); anything else is Tugboat Run, the first. */
export function gameFrom(value: unknown): PadGame {
  return value === "harbor-stack" ? "harbor-stack" : "tugboat-run";
}

export interface GameControls {
  title: string;
  /** The right thumb's button, and the line under it. */
  action: string;
  actionHint: string;
  /** The left thumb's slider: what it's called, the words at each end, and the line under it. */
  steerLabel: string;
  ends: [string, string] | null;
  steerHint: string;
  /** The knob springs back to the middle when let go (a nudge), rather than staying put. */
  springBack: boolean;
  /** One line for the "turn sideways" card. */
  thumbs: string;
}

/** Each game's buttons. */
export function controlsFor(game: PadGame): GameControls {
  if (game === "harbor-stack") {
    return {
      title: "Harbor Stack",
      action: "Drop",
      actionHint: "Let the container go",
      steerLabel: "Crane speed",
      ends: ["Slower", "Faster"],
      steerHint: "Hold to nudge the crane",
      springBack: true,
      thumbs: "Left thumb nudges the crane, right thumb drops.",
    };
  }
  return {
    title: "Tugboat Run",
    action: "Boost",
    actionHint: "Hop over buoys and logs",
    steerLabel: "Steer",
    ends: null,
    steerHint: "Slide: the boat follows",
    springBack: false,
    thumbs: "Left thumb steers, right thumb boosts.",
  };
}

// --- Holding the phone ---

export type ControllerLayout = "landscape" | "rotate" | "portrait";

/**
 * Held sideways like a gamepad: steering under the left thumb, the button under the right. Held
 * upright, the page asks to be turned (Safari can't lock the orientation for it) unless the player
 * chose to play upright anyway.
 */
export function chooseLayout(env: { landscape: boolean; portraitAnyway: boolean }): ControllerLayout {
  if (env.landscape) return "landscape";
  return env.portraitAnyway ? "portrait" : "rotate";
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

/** Harbor Stack: the crane nudge in [-1, 1] and Drop → what goes on the wire (never a boost). */
export function encodeStackPad(nudge: number, drop: boolean): PadInput {
  return { steer: encodePad(nudge, false).steer, boost: false, drop: drop === true };
}

/** The input for `game`: Tugboat Run's slider and Boost, or Harbor Stack's nudge and Drop. */
export function encodeFor(game: PadGame, steer: number, pressed: boolean): PadInput {
  return game === "harbor-stack" ? encodeStackPad(steer, pressed) : encodePad(steer, pressed);
}

/**
 * What goes in the sealed body: exactly the fields the PC accepts, in a fixed order, whatever else
 * the objects carry. `drop` only when the input has one (Harbor Stack), so Tugboat Run's body is
 * what it always was. (Leaving `drop` out here is how Harbor Stack's Drop once never reached the
 * PC: padVector.json now pins this against the Rust decoder.)
 */
export function padBody(input: PadInput, probe?: PadProbe): Record<string, number | boolean> {
  const body: Record<string, number | boolean> = { steer: input.steer, boost: input.boost };
  if (input.drop !== undefined) body.drop = input.drop;
  if (probe) {
    body.age = probe.age;
    body.rtt = probe.rtt;
  }
  return body;
}

/** How long a press (or release) may wait for the PC to hear it. Past this it's dropped: a press
 * held back by a Wi-Fi blip and delivered seconds later would drop a container nobody meant to drop
 * then. A round trip on the same Wi-Fi is a few milliseconds. */
export const PRESS_EXPIRE_MS = 500;
/** Button changes waiting behind the one on its way (a burst of taps), at most. */
const MAX_QUEUED = 6;

/**
 * The big button's presses and releases, each latched until the PC has heard it. Only the newest
 * state goes out, and up to two inputs are in flight (the PC ignores the older if they land out of
 * order), so a quick tap whose press and release go out back to back could lose its press, and two
 * quick taps could reach the PC as one long press. Instead each change stays in every input until a
 * reply to one that carried it comes back, and only then does the next change go: a tap is a press
 * and then a release, and two taps are two presses with a release between them. The first press
 * reaches the PC just as fast; each change after it waits one round trip.
 *
 * A change the PC hasn't heard within `PRESS_EXPIRE_MS`, or when the connection drops (`clear`), is
 * forgotten, and a finger still on the button doesn't count until it presses again, so nothing
 * fires late.
 */
export class PressLatch {
  /** What the PC has confirmed: up until a press is heard. */
  private heardDown = false;
  /** The change on its way, until a reply to an input carrying it comes back. */
  private frame: { down: boolean; id: number; since: number } | null = null;
  /** Changes to send after it, in order. */
  private queue: boolean[] = [];
  private ids = 0;

  /** Where the button is heading: the last change asked for. */
  private get wanted(): boolean {
    if (this.queue.length) return this.queue[this.queue.length - 1];
    return this.frame ? this.frame.down : this.heardDown;
  }

  private push(down: boolean, now: number) {
    this.expire(now);
    if (this.wanted === down) return;
    if (this.frame === null) this.frame = { down, id: ++this.ids, since: now };
    else if (this.queue.length < MAX_QUEUED) this.queue.push(down);
  }

  private expire(now: number) {
    if (this.frame && now - this.frame.since > PRESS_EXPIRE_MS) this.clear();
  }

  /** A finger came down on the button. */
  press(now: number) {
    this.push(true, now);
  }

  /** The finger lifted. */
  release(now: number) {
    this.push(false, now);
  }

  /** The button as it goes on the wire now. */
  down(now: number): boolean {
    this.expire(now);
    return this.frame ? this.frame.down : this.heardDown;
  }

  /** A change the PC hasn't confirmed yet. */
  get pending(): boolean {
    return this.frame !== null;
  }

  /** An input is going out: the mark to confirm when its reply comes back (0: nothing waiting). */
  mark(): number {
    return this.frame?.id ?? 0;
  }

  /** The reply to an input sent with `mark` came back: the PC has that change; the next can go. */
  heardUpTo(mark: number, now: number) {
    if (!this.frame || mark !== this.frame.id) return;
    this.heardDown = this.frame.down;
    const next = this.queue.shift();
    this.frame = next === undefined ? null : { down: next, id: ++this.ids, since: now };
  }

  /** Forget every waiting change (the page hid, the game changed, or the connection dropped). */
  clear() {
    this.frame = null;
    this.queue = [];
    this.heardDown = false;
  }
}

export function samePad(a: PadInput | null, b: PadInput): boolean {
  return a !== null && a.steer === b.steer && a.boost === b.boost && (a.drop ?? false) === (b.drop ?? false);
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
 * What a failed input means for the controller. "leave": the game closed, the session's over, or
 * a two-player race already has its two phones, so go back to the Tugboat page. "slow": the PC
 * asked for fewer. "retry": a dropped packet or a stale sequence number; carry on.
 */
export function padErrorAction(code: string): "leave" | "slow" | "retry" {
  if (["no-game", "closed", "unauthorized", "in-use", "full", "no-race", "player-2-code"].includes(code)) return "leave";
  if (code === "busy") return "slow";
  return "retry";
}

/**
 * Whether tilt steering can work here. Safari (iOS 13+) and Chrome only give a page the phone's
 * motion and orientation in a secure context (HTTPS), and Tugboat's page is plain HTTP on your
 * Wi-Fi, so on Tugboat this is "needs-secure" and the controller offers no tilt switch at all
 * (nothing that can't work, and no promise). Only where it really is available is tilt offered.
 */
export function tiltSupport(env: { secure: boolean; hasOrientation: boolean }): "available" | "needs-secure" | "unsupported" {
  if (!env.hasOrientation) return env.secure ? "unsupported" : "needs-secure";
  return env.secure ? "available" : "needs-secure";
}

/**
 * Tilt steering, where it's available: the phone held sideways and turned like a steering wheel.
 * `angle` is the screen's rotation (90 or -90/270 sideways, 0 upright); `beta` and `gamma` are the
 * orientation event's degrees. `range` degrees of tilt is full lock.
 */
export function tiltToSteer(beta: number | null, gamma: number | null, angle: number, range = 25): number {
  const a = ((angle % 360) + 360) % 360;
  const raw = a === 90 ? beta : a === 270 ? (beta === null ? null : -beta) : gamma;
  if (raw === null || !Number.isFinite(raw)) return 0;
  return Math.max(-1, Math.min(1, raw / range)) || 0;
}
