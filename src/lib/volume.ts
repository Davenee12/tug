// The phone volume bar's rules, kept out of the component so they can be tested.
//
// AMS reports the player's volume as a 0–1 fraction but only offers VolumeUp/VolumeDown, one step
// at a time: there is no "set volume to 40%". So clicking or dragging the bar to a level means
// working out how many steps that is and sending them one by one, each after the phone has
// reported the step before (or a short wait has passed), so the phone never gets a burst.
import type { NowPlaying } from "../types/protocol";
import type { HoldTimers } from "./media";

/**
 * One VolumeUp/VolumeDown moves the iPhone's volume by 1/16. Seen in tug's own logs on a real
 * iPhone: each step reported 0.0625 apart (0.75 → 0.8125 → 0.875 → 0.9375 → 1), and after the
 * phone's own slider the steps keep that size from wherever it was left (0.2712765 → 0.3337765).
 */
export const VOLUME_STEP = 1 / 16;

/** How long to wait for the phone to report a step before sending the next one anyway. */
export const STEP_WAIT_MS = 700;

/** Steps in a row the phone didn't report (at a limit, or the player ignored them) before stopping. */
export const MAX_SILENT_STEPS = 2;

/**
 * Most steps one click, drag or key press may send: a full sweep is 16, with room for a correction.
 * A guard for a player whose steps are smaller than the phone's, so a seek can't run on and on.
 */
export const MAX_STEPS = 20;

// Volume arrives as text from the phone ("0.625"); keep float noise from deciding a step.
const EPSILON = 1e-6;

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

/** The bar shows only with the phone connected and the player reporting a volume. */
export function volumeShown(np: Pick<NowPlaying, "volume">, connected: boolean, mediaAvailable: boolean): boolean {
  return connected && mediaAvailable && np.volume != null && Number.isFinite(np.volume);
}

/** The volume as a whole percentage, for the slider's value and its spoken text. */
export function volumePercent(volume: number): number {
  return Math.round(clamp01(volume) * 100);
}

/**
 * How many steps take the volume from `current` to `target`: positive for up, negative for down,
 * 0 when it's already the closest it can get. Exactly halfway between two steps stays put.
 * A target at an end (within half a step) goes all the way there, even from a level that isn't on
 * the 1/16 grid, so dragging to the end really reaches silent or full.
 */
export function stepsToward(current: number, target: number, step: number = VOLUME_STEP): number {
  const c = clamp01(current);
  const t = clamp01(target);
  if (t >= 1 - step / 2) return Math.max(0, Math.ceil((1 - c) / step - EPSILON));
  if (t <= step / 2) {
    const down = Math.max(0, Math.ceil(c / step - EPSILON));
    return down === 0 ? 0 : -down;
  }
  const steps = (t - c) / step;
  const n = Math.round(Math.abs(steps) - EPSILON);
  return n === 0 ? 0 : Math.sign(steps) * n;
}

/** One step up or down from `current`, as a target (wheel and arrow keys). */
export function nudgeTarget(current: number, direction: 1 | -1, step: number = VOLUME_STEP): number {
  return clamp01(current + direction * step);
}

/** The direction a wheel event means: scrolling up is louder. 0 for a sideways scroll. */
export function wheelDirection(deltaY: number): 1 | -1 | 0 {
  if (deltaY < 0) return 1;
  if (deltaY > 0) return -1;
  return 0;
}

/** What an arrow, Home or End key does on the bar, or null for keys it doesn't handle. */
export function keyAction(key: string): { nudge: 1 | -1 } | { to: number } | null {
  switch (key) {
    case "ArrowUp":
    case "ArrowRight":
      return { nudge: 1 };
    case "ArrowDown":
    case "ArrowLeft":
      return { nudge: -1 };
    case "Home":
      return { to: 0 };
    case "End":
      return { to: 1 };
    default:
      return null;
  }
}

export interface VolumeSeekerOptions {
  /** Send one step; resolves true once the phone accepted the write. */
  send: (direction: 1 | -1) => Promise<boolean>;
  timers: HoldTimers;
  /** Called whenever the level being headed for changes (null when idle), so the UI can mark it. */
  onTarget?: (target: number | null) => void;
  step?: number;
  waitMs?: number;
  maxSilentSteps?: number;
  maxSteps?: number;
}

export interface VolumeSeeker {
  /** Head for `target` (0–1). While a seek runs, this just moves the goal (a drag); no extra sends. */
  seek(target: number): void;
  /**
   * One step up or down; false when nothing will be sent (at the end, or no volume yet). While a
   * seek runs it's ignored, so a fast wheel or a held key can't pile up steps, unless `queue` (a
   * separate key press): then the goal moves one step further, so three quick taps are three steps.
   */
  nudge(direction: 1 | -1, queue?: boolean): boolean;
  /** The phone reported a volume (or stopped reporting one: null). */
  report(volume: number | null): void;
  /** Stop after the step in flight (a write can't be taken back). Safe to call when idle. */
  cancel(): void;
  readonly busy: boolean;
  readonly target: number | null;
}

/**
 * Walks the volume to a target one step at a time. Never has more than one step in flight: after
 * each, it waits for the phone to report a new volume (or `waitMs`), then re-plans from the volume
 * the phone actually reported rather than counting down a fixed plan, so a step the phone dropped
 * is made up and a drag just moves the goal. Stops at the target, when the phone stops answering steps
 * (`maxSilentSteps` in a row), when a write fails, or after `maxSteps`. No timer runs while idle.
 */
export function createVolumeSeeker(opts: VolumeSeekerOptions): VolumeSeeker {
  const step = opts.step ?? VOLUME_STEP;
  const waitMs = opts.waitMs ?? STEP_WAIT_MS;
  const maxSilent = opts.maxSilentSteps ?? MAX_SILENT_STEPS;
  const maxSteps = opts.maxSteps ?? MAX_STEPS;

  let current: number | null = null;
  let target: number | null = null;
  let running = false;
  /** Steps sent since the last user action; reset by seek() so each action gets its own budget. */
  let sent = 0;
  let waiting: { before: number; done: (changed: boolean) => void; handle: number } | null = null;

  function setTarget(t: number | null) {
    if (t === target) return;
    target = t;
    opts.onTarget?.(t);
  }

  function endWait(changed: boolean) {
    if (!waiting) return;
    const w = waiting;
    waiting = null;
    opts.timers.clear(w.handle);
    w.done(changed);
  }

  /** Resolves true when the phone reports a volume other than `before`, false after `waitMs`. */
  function waitForChange(before: number): Promise<boolean> {
    if (current == null || Math.abs(current - before) > EPSILON) return Promise.resolve(true);
    return new Promise((done) => {
      const handle = opts.timers.set(() => endWait(false), waitMs);
      waiting = { before, done, handle };
    });
  }

  async function run() {
    running = true;
    let silent = 0;
    try {
      while (target != null && current != null) {
        const n = stepsToward(current, target, step);
        if (n === 0 || silent >= maxSilent || sent >= maxSteps) break;
        const before = current;
        sent += 1;
        if (!(await opts.send(n > 0 ? 1 : -1).catch(() => false))) break;
        silent = (await waitForChange(before)) ? 0 : silent + 1;
      }
    } finally {
      running = false;
      endWait(false);
      setTarget(null);
    }
  }

  function seek(t: number) {
    if (current == null) return;
    sent = 0;
    setTarget(clamp01(t));
    if (!running) void run();
  }

  return {
    seek,
    nudge(direction, queue = false) {
      if (current == null || (running && !queue)) return false;
      const from = running ? (target ?? current) : current;
      const t = nudgeTarget(from, direction, step);
      if (running) {
        if (t === target) return false;
        seek(t);
        return true;
      }
      if (stepsToward(current, t, step) === 0) return false;
      seek(t);
      return true;
    },
    report(volume) {
      current = volume != null && Number.isFinite(volume) ? volume : null;
      if (!waiting) return;
      if (current == null || Math.abs(current - waiting.before) > EPSILON) endWait(true);
    },
    cancel() {
      setTarget(null);
      endWait(false);
    },
    get busy() {
      return running;
    },
    get target() {
      return target;
    },
  };
}
