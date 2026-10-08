/**
 * Keep the newest of a burst of updates and apply it once. The iPhone reports a song change as
 * separate updates for the title, artist, album and length, a few milliseconds apart, and each one
 * used to re-render the sidebar; held for one frame they land as one.
 *
 * `schedule` arranges for the flush to run later and returns a function that calls it off (the app
 * uses the next animation frame, with a short timer behind it so a value still lands while frames
 * are paused). It's injectable so this is tested without a browser.
 */
export interface Coalescer<T> {
  /** Remember `value` as the newest; apply it at the next flush. */
  push(value: T): void;
  /** Apply a pending value now (before something reads the state directly). */
  flush(): void;
  /** Drop a pending value without applying it. */
  cancel(): void;
}

export function coalesce<T>(apply: (value: T) => void, schedule: (flush: () => void) => () => void): Coalescer<T> {
  let pending: { value: T } | null = null;
  let unschedule: (() => void) | null = null;
  const cancel = () => {
    unschedule?.();
    unschedule = null;
    pending = null;
  };
  const flush = () => {
    const p = pending;
    cancel();
    if (p) apply(p.value);
  };
  return {
    push(value) {
      pending = { value };
      unschedule ??= schedule(flush);
    },
    flush,
    cancel,
  };
}

/** The next animation frame, or `maxMs` if frames are paused (a hidden window), whichever is first. */
export function nextFrame(fn: () => void, maxMs = 100): () => void {
  let done = false;
  const run = () => {
    if (done) return;
    done = true;
    window.cancelAnimationFrame(frame);
    window.clearTimeout(timer);
    fn();
  };
  const frame = window.requestAnimationFrame(run);
  const timer = window.setTimeout(run, maxMs);
  return () => {
    done = true;
    window.cancelAnimationFrame(frame);
    window.clearTimeout(timer);
  };
}
