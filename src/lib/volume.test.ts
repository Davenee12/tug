import { describe, expect, it } from "vitest";
import type { HoldTimers } from "./media";
import {
  createVolumeSeeker,
  keyAction,
  MAX_STEPS,
  nudgeTarget,
  STEP_WAIT_MS,
  stepsToward,
  VOLUME_STEP,
  volumePercent,
  volumeShown,
  wheelDirection,
} from "./volume";

describe("VOLUME_STEP", () => {
  it("is the 1/16 step seen on a real iPhone", () => {
    // Consecutive reports after one VolumeUp each, from tug's log.
    const seen = [0.75, 0.8125, 0.875, 0.9375, 1];
    for (let i = 1; i < seen.length; i++) expect(seen[i] - seen[i - 1]).toBeCloseTo(VOLUME_STEP, 6);
    // And off the grid, after the phone's own slider.
    expect(0.3337765 - 0.2712765).toBeCloseTo(VOLUME_STEP, 6);
  });
});

describe("stepsToward", () => {
  it("counts whole steps, up positive and down negative", () => {
    expect(stepsToward(0.625, 0.625)).toBe(0);
    expect(stepsToward(0.625, 0.75)).toBe(2);
    expect(stepsToward(0.625, 0.25)).toBe(-6);
    expect(stepsToward(0.5, 0.53)).toBe(0); // under half a step: already the closest
    expect(stepsToward(0.5, 0.6)).toBe(2); // 1.6 steps rounds to 2
  });

  it("stays put exactly halfway, in either direction", () => {
    expect(stepsToward(0.5, 0.5 + VOLUME_STEP / 2)).toBe(0);
    expect(stepsToward(0.5, 0.5 - VOLUME_STEP / 2)).toBe(0);
    expect(Object.is(stepsToward(0.5, 0.49), 0)).toBe(true); // never -0
  });

  it("goes all the way to an end, even from a level off the 1/16 grid", () => {
    expect(stepsToward(0.2712765, 1)).toBe(12); // 11.66 steps: 12 reach full
    expect(stepsToward(0.2712765, 0)).toBe(-5); // 4.34 steps: 5 reach silent
    expect(stepsToward(0.9375, 0.99)).toBe(1); // within half a step of the end counts as the end
    expect(stepsToward(0.0625, 0.01)).toBe(-1);
    expect(stepsToward(1, 1)).toBe(0);
    expect(stepsToward(0, 0)).toBe(0);
  });

  it("clamps targets and levels outside 0–1", () => {
    expect(stepsToward(0.5, 3)).toBe(8);
    expect(stepsToward(0.5, -1)).toBe(-8);
    expect(stepsToward(1.2, 1)).toBe(0);
  });

  it("isn't thrown by the float noise in the phone's text values", () => {
    expect(stepsToward(0.3499999, 0.35)).toBe(0);
    expect(stepsToward(1 - 1e-9, 1)).toBe(0);
  });
});

describe("small rules", () => {
  it("shows the bar only when connected, playing and reporting a volume", () => {
    expect(volumeShown({ volume: 0.5 }, true, true)).toBe(true);
    expect(volumeShown({ volume: 0 }, true, true)).toBe(true); // silent is still a level
    expect(volumeShown({ volume: null }, true, true)).toBe(false);
    expect(volumeShown({ volume: 0.5 }, false, true)).toBe(false);
    expect(volumeShown({ volume: 0.5 }, true, false)).toBe(false);
    expect(volumeShown({ volume: Number.NaN }, true, true)).toBe(false);
  });

  it("reads as a whole percentage", () => {
    expect(volumePercent(0.625)).toBe(63);
    expect(volumePercent(0)).toBe(0);
    expect(volumePercent(1.4)).toBe(100);
  });

  it("nudges one step and stops at the ends", () => {
    expect(nudgeTarget(0.5, 1)).toBe(0.5625);
    expect(nudgeTarget(0.5, -1)).toBe(0.4375);
    expect(nudgeTarget(1, 1)).toBe(1);
    expect(nudgeTarget(0, -1)).toBe(0);
  });

  it("maps the wheel and keys", () => {
    expect(wheelDirection(-100)).toBe(1);
    expect(wheelDirection(3)).toBe(-1);
    expect(wheelDirection(0)).toBe(0);
    expect(keyAction("ArrowUp")).toEqual({ nudge: 1 });
    expect(keyAction("ArrowRight")).toEqual({ nudge: 1 });
    expect(keyAction("ArrowDown")).toEqual({ nudge: -1 });
    expect(keyAction("ArrowLeft")).toEqual({ nudge: -1 });
    expect(keyAction("Home")).toEqual({ to: 0 });
    expect(keyAction("End")).toEqual({ to: 1 });
    expect(keyAction("a")).toBeNull();
  });
});

// --- The seeker, against a pretend phone -------------------------------------

function harness() {
  let seq = 1;
  let clock = 0;
  const pending = new Map<number, { fn: () => void; due: number }>();
  const timers: HoldTimers = {
    set: (fn, ms) => {
      const id = seq++;
      pending.set(id, { fn, due: clock + ms });
      return id;
    },
    clear: (id) => void pending.delete(id),
  };
  return {
    timers,
    get now() {
      return clock;
    },
    get scheduled() {
      return pending.size;
    },
    /** Run every timer due by `ms` from now, letting promises settle between them. */
    async advance(ms: number) {
      const end = clock + ms;
      for (;;) {
        await settle();
        const next = [...pending.entries()].filter(([, t]) => t.due <= end).sort((a, b) => a[1].due - b[1].due)[0];
        if (!next) break;
        clock = next[1].due;
        pending.delete(next[0]);
        next[1].fn();
      }
      clock = end;
      await settle();
    },
  };
}

async function settle() {
  for (let i = 0; i < 10; i++) await Promise.resolve();
}

/**
 * A phone: each accepted step changes the volume by 1/16 (clamped) and reports it `latency` ms
 * later; at an end nothing changes and nothing is reported, as on the real thing. Records every
 * send and how many were in flight at once.
 */
function phone(h: ReturnType<typeof harness>, start: number, opts: { latency?: number; silent?: boolean; refuse?: boolean } = {}) {
  const latency = opts.latency ?? 120;
  const p = {
    volume: start,
    sends: [] as { dir: 1 | -1; at: number }[],
    inFlight: 0,
    maxInFlight: 0,
    seeker: null as unknown as ReturnType<typeof createVolumeSeeker>,
    targets: [] as (number | null)[],
  };
  p.seeker = createVolumeSeeker({
    timers: h.timers,
    onTarget: (t) => p.targets.push(t),
    send: (dir) => {
      p.sends.push({ dir, at: h.now });
      p.inFlight++;
      p.maxInFlight = Math.max(p.maxInFlight, p.inFlight);
      return new Promise((resolve) => {
        // The write is acknowledged quickly; the volume report follows later.
        h.timers.set(() => {
          p.inFlight--;
          resolve(!opts.refuse);
        }, 20);
        if (opts.refuse || opts.silent) return;
        const next = Math.min(1, Math.max(0, p.volume + dir * VOLUME_STEP));
        if (next === p.volume) return;
        h.timers.set(() => {
          p.volume = next;
          p.seeker.report(next);
        }, latency);
      });
    },
  });
  p.seeker.report(start);
  return p;
}

describe("createVolumeSeeker", () => {
  it("walks to the target one step at a time, each after the phone reported the last", async () => {
    const h = harness();
    const p = phone(h, 0.625);
    p.seeker.seek(0.25);
    expect(p.seeker.busy).toBe(true);
    expect(p.seeker.target).toBe(0.25);
    await h.advance(5000);
    expect(p.volume).toBe(0.25);
    expect(p.sends.map((s) => s.dir)).toEqual([-1, -1, -1, -1, -1, -1]);
    expect(p.maxInFlight).toBe(1);
    // Spaced by the phone's answer (120 ms), not fired together.
    const gaps = p.sends.slice(1).map((s, i) => s.at - p.sends[i].at);
    expect(Math.min(...gaps)).toBeGreaterThanOrEqual(120);
    expect(p.seeker.busy).toBe(false);
    expect(p.seeker.target).toBeNull();
    expect(p.targets).toEqual([0.25, null]);
    expect(h.scheduled).toBe(0); // idle: no timer left running
  });

  it("does nothing when already at the closest step", async () => {
    const h = harness();
    const p = phone(h, 0.5);
    p.seeker.seek(0.52);
    await h.advance(1000);
    expect(p.sends).toEqual([]);
    expect(p.seeker.busy).toBe(false);
  });

  it("follows a drag: a new target mid-seek re-plans from the reported volume, with no extra sends", async () => {
    const h = harness();
    const p = phone(h, 0.5);
    p.seeker.seek(1);
    await h.advance(130); // first step reported (0.5625), second in flight
    expect(p.sends.length).toBe(2);
    p.seeker.seek(0.375); // dragged back down
    expect(p.sends.length).toBe(2); // nothing sent just for moving the goal
    await h.advance(5000);
    expect(p.volume).toBe(0.375);
    expect(p.maxInFlight).toBe(1);
    // Up twice (the second was already in flight), then back down to 0.375.
    expect(p.sends.map((s) => s.dir)).toEqual([1, 1, -1, -1, -1, -1]);
  });

  it("cancel stops after the step in flight", async () => {
    const h = harness();
    const p = phone(h, 0.5);
    p.seeker.seek(1);
    await h.advance(10);
    p.seeker.cancel();
    expect(p.seeker.target).toBeNull();
    await h.advance(5000);
    expect(p.sends.length).toBe(1);
    expect(p.volume).toBe(0.5625);
    expect(p.seeker.busy).toBe(false);
    expect(h.scheduled).toBe(0);
  });

  it("moves on after a bounded wait when a report doesn't come, and stops if the phone stays silent", async () => {
    const h = harness();
    const p = phone(h, 0.5, { silent: true });
    p.seeker.seek(1);
    await h.advance(STEP_WAIT_MS * 10);
    // Two unanswered steps, the second only after the wait: then it gives up.
    expect(p.sends.length).toBe(2);
    expect(p.sends[1].at - p.sends[0].at).toBeGreaterThanOrEqual(STEP_WAIT_MS);
    expect(p.seeker.busy).toBe(false);
    expect(h.scheduled).toBe(0);
  });

  it("stops at the first refused write", async () => {
    const h = harness();
    const p = phone(h, 0.5, { refuse: true });
    p.seeker.seek(0);
    await h.advance(5000);
    expect(p.sends.length).toBe(1);
    expect(p.seeker.busy).toBe(false);
  });

  it("treats a send that throws as refused", async () => {
    const h = harness();
    let sends = 0;
    const s = createVolumeSeeker({
      timers: h.timers,
      send: () => {
        sends++;
        return Promise.reject(new Error("link down"));
      },
    });
    s.report(0.5);
    s.seek(1);
    await h.advance(1000);
    expect(sends).toBe(1);
    expect(s.busy).toBe(false);
  });

  it("caps the steps one action can send, for a player whose steps are smaller", async () => {
    const h = harness();
    let volume = 0;
    const s: ReturnType<typeof createVolumeSeeker> = createVolumeSeeker({
      timers: h.timers,
      send: (dir) => {
        volume = Math.min(1, volume + dir * 0.01); // a 1% step, as a speaker might use
        h.timers.set(() => s.report(volume), 50);
        return Promise.resolve(true);
      },
    });
    s.report(0);
    s.seek(1);
    await h.advance(60_000);
    expect(volume).toBeCloseTo(MAX_STEPS * 0.01, 6);
    expect(s.busy).toBe(false);
  });

  it("makes up a step the phone dropped, after the bounded wait", async () => {
    const h = harness();
    let volume = 0.5;
    let dropped = false;
    const sends: number[] = [];
    const s: ReturnType<typeof createVolumeSeeker> = createVolumeSeeker({
      timers: h.timers,
      send: (dir) => {
        sends.push(h.now);
        if (sends.length === 2 && !dropped) {
          dropped = true; // accepted, but the volume never moved and nothing was reported
          return Promise.resolve(true);
        }
        h.timers.set(() => {
          volume += dir * VOLUME_STEP;
          s.report(volume);
        }, 100);
        return Promise.resolve(true);
      },
    });
    s.report(volume);
    s.seek(0.75); // 4 steps
    await h.advance(10_000);
    expect(volume).toBe(0.75);
    expect(sends.length).toBe(5); // the 4 planned plus the one that went missing
    expect(sends[2] - sends[1]).toBe(STEP_WAIT_MS);
    expect(s.busy).toBe(false);
  });

  it("nudge is one step, ignored while a seek runs and at the ends", async () => {
    const h = harness();
    const p = phone(h, 0.9375);
    expect(p.seeker.nudge(1)).toBe(true);
    expect(p.seeker.nudge(1)).toBe(false); // a second wheel tick during the first: dropped
    await h.advance(1000);
    expect(p.volume).toBe(1);
    expect(p.sends.length).toBe(1);
    expect(p.seeker.nudge(1)).toBe(false); // already full: nothing to send
    expect(p.seeker.nudge(-1)).toBe(true);
    await h.advance(1000);
    expect(p.volume).toBe(0.9375);
  });

  it("queues separate key presses made during a step: three quick taps are three steps", async () => {
    const h = harness();
    const p = phone(h, 0.5);
    expect(p.seeker.nudge(1, true)).toBe(true);
    expect(p.seeker.nudge(1, true)).toBe(true);
    expect(p.seeker.nudge(1, true)).toBe(true);
    expect(p.seeker.target).toBe(0.6875);
    expect(p.sends.length).toBe(1); // still one at a time
    await h.advance(5000);
    expect(p.volume).toBe(0.6875);
    expect(p.sends.length).toBe(3);
    expect(p.maxInFlight).toBe(1);
    // At the end, a queued press past it changes nothing.
    const q = phone(harness(), 0.9375);
    expect(q.seeker.nudge(1, true)).toBe(true);
    expect(q.seeker.nudge(1, true)).toBe(false);
  });

  it("does nothing without a reported volume, and stops if the volume goes away", async () => {
    const h = harness();
    let sends = 0;
    const s = createVolumeSeeker({ timers: h.timers, send: () => (sends++, Promise.resolve(true)) });
    s.seek(1);
    expect(s.nudge(1)).toBe(false);
    expect(sends).toBe(0);

    s.report(0.5);
    s.seek(1);
    await h.advance(10);
    expect(sends).toBe(1);
    s.report(null); // player stopped reporting volume
    await h.advance(5000);
    expect(sends).toBe(1);
    expect(s.busy).toBe(false);
    expect(h.scheduled).toBe(0);
  });
});
