// Now Playing rules that don't need the DOM, so they can be tested.
import type { MediaCommand, NowPlaying, RepeatMode } from "../types/protocol";

/**
 * The extra AMS controls (skip ±15 s, like/dislike) appear only when the current player lists the
 * command — unlike the transport buttons, which show on an empty list (before it arrives). A button
 * that does nothing is worse than no button, and iOS only lists what the player actually accepts.
 */
export function lists(np: { available: readonly string[] }, command: MediaCommand): boolean {
  return np.available.includes(command);
}

/** How far skipBackward/skipForward jump, for the button labels (iOS uses a 15 s step). */
export const SKIP_SECONDS = 15;

export const supportsSkipBack = (np: { available: readonly string[] }): boolean => lists(np, "skipBackward");
export const supportsSkipForward = (np: { available: readonly string[] }): boolean => lists(np, "skipForward");
export const supportsLike = (np: { available: readonly string[] }): boolean => lists(np, "likeTrack");
export const supportsDislike = (np: { available: readonly string[] }): boolean => lists(np, "dislikeTrack");

/**
 * The loop button only appears when the player lists AdvanceRepeatMode. A repeat value on its
 * own isn't enough: iOS can report the queue's repeat mode for a player that won't take the
 * command, and a button that does nothing is worse than no button.
 */
export function supportsRepeat(np: Pick<NowPlaying, "available">): boolean {
  return np.available.includes("advanceRepeatMode");
}

export function repeatLabel(mode: RepeatMode | null): string {
  switch (mode) {
    case "all":
      return "Repeating all";
    case "one":
      return "Repeating this song";
    case "off":
      return "Repeat is off";
    default:
      return "Repeat";
  }
}

/** How long to wait for the phone to report a new repeat mode before saying it didn't change. */
export const REPEAT_CONFIRM_MS = 2000;

/** Shown when a repeat press went through but the phone never reported a new mode. */
export function repeatIgnoredMessage(player: string | null): string {
  return `${player ?? "The player"} didn't change repeat from your PC`;
}

/**
 * AMS has no seek, but Back restarts the song once it's a few seconds in (and goes to the previous
 * song near the start). tug's position is an estimate from the last report, so keep a margin above
 * the phone's ~3 s cut-off: a Back that lands just under it would skip to the previous song.
 */
export const RESTART_AFTER_S = 5;

/** Whether Back would restart the song rather than go to the previous one. Unknown position: try. */
export function canRestart(elapsed: number | null): boolean {
  return elapsed == null || elapsed > RESTART_AFTER_S;
}

// --- Press-and-hold volume --------------------------------------------------
// Holding the volume up/down button should keep changing the volume: one step on press,
// then a steady (slightly accelerating) repeat. Each step is one AMS VolumeUp/VolumeDown.
// The timing is kept here, pure and with injectable timers, so the component stays thin and
// this can be unit-tested without the DOM.

/** Wait after the first press step before auto-repeat begins. */
export const HOLD_INITIAL_MS = 400;
/** Interval between the first few auto-repeat steps. */
export const HOLD_REPEAT_MS = 180;
/** Fastest interval the repeat accelerates to. */
export const HOLD_MIN_REPEAT_MS = 90;
/** Each repeat multiplies the interval by this, easing down toward HOLD_MIN_REPEAT_MS. */
export const HOLD_ACCEL = 0.85;

export interface HoldConfig {
  initialMs?: number;
  repeatMs?: number;
  minRepeatMs?: number;
  accel?: number;
}

/** Injectable timer pair, so tests drive a fake clock and the app passes window timers. */
export interface HoldTimers {
  set: (fn: () => void, ms: number) => number;
  clear: (handle: number) => void;
}

/**
 * Interval before the (step+1)-th auto-repeat, counting from step 0 (the first). Starts at
 * repeatMs and eases geometrically toward minRepeatMs so a long hold speeds up a little.
 */
export function holdDelay(step: number, cfg: HoldConfig = {}): number {
  const base = cfg.repeatMs ?? HOLD_REPEAT_MS;
  const min = cfg.minRepeatMs ?? HOLD_MIN_REPEAT_MS;
  const accel = cfg.accel ?? HOLD_ACCEL;
  return Math.max(min, Math.round(base * accel ** Math.max(0, step)));
}

export interface HoldRepeater {
  /**
   * Begin a hold: `step` performs one change (resolving when the phone has accepted the write),
   * `atLimit` reports whether the volume is already at the end in this direction. One step fires
   * immediately; auto-repeat follows after the initial delay and stops at the limit.
   */
  start(step: () => Promise<unknown>, atLimit: () => boolean): void;
  /** End a hold (pointer up/leave/cancel, key up, blur, unmount). Safe to call when idle. */
  stop(): void;
  /** True between start() and stop(). */
  readonly active: boolean;
}

/**
 * A press-and-hold repeater. Never lets more than one step be in flight: a tick that lands while
 * the previous step is still resolving is skipped and retried next interval, so a slow AMS write
 * can't stack up queued commands. Stops itself once the limit is reached so it won't spam there.
 */
export function createHoldRepeater(timers: HoldTimers, cfg: HoldConfig = {}): HoldRepeater {
  const initialMs = cfg.initialMs ?? HOLD_INITIAL_MS;
  let handle: number | null = null;
  let active = false;
  let busy = false;
  let repeats = 0;
  let stepFn: () => Promise<unknown> = () => Promise.resolve();
  let atLimitFn: () => boolean = () => false;

  function schedule(ms: number) {
    handle = timers.set(tick, ms);
  }

  function send() {
    busy = true;
    // Resolve order doesn't matter; the finally just frees the next tick to run.
    void Promise.resolve(stepFn()).finally(() => {
      busy = false;
    });
  }

  function tick() {
    handle = null;
    if (!active) return;
    if (atLimitFn()) return stop();
    // A write is still in flight: skip this beat and try again, rather than queue a second one.
    if (busy) return schedule(holdDelay(repeats, cfg));
    send();
    schedule(holdDelay(repeats, cfg));
    repeats += 1;
  }

  function stop() {
    active = false;
    busy = false;
    if (handle != null) {
      timers.clear(handle);
      handle = null;
    }
  }

  return {
    start(step, atLimit) {
      if (active) return;
      active = true;
      busy = false;
      repeats = 0;
      stepFn = step;
      atLimitFn = atLimit;
      // One step on press even at the limit (a normal tap); the first repeat tick stops there.
      send();
      schedule(initialMs);
    },
    stop,
    get active() {
      return active;
    },
  };
}
