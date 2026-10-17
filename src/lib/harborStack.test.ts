import { describe, expect, it } from "vitest";
import {
  BOX_H,
  CALM_STACK,
  CRANE_GAP,
  GUST_FROM,
  HOLD_S,
  LIVES,
  MILESTONES,
  NUDGE,
  PERFECT,
  RESTORE_W,
  SPEED_MAX,
  SPEED_START,
  START_W,
  SWING,
  advance,
  cameraFor,
  craneSpeed,
  craneX,
  driftAt,
  gustAt,
  gustWarning,
  kindFor,
  milestoneLine,
  newStack,
  nudgeFrom,
  resolveDrop,
  restoredWidth,
  settled,
  stackOffKey,
  stackOutlook,
  stackScore,
  stackWeather,
  step,
  swayAmp,
  swayAt,
  topHeight,
  type Stack,
} from "./harborStack";
import { CALM } from "./tugboatRun";
import { CALM_SEAS, conditionsLine, dailySeed, finishDaily, currentStreak, EMPTY_DAILY, handlingFor, type Conditions } from "./tugboatDaily";

const DT = 1 / 60;
const idle = { drop: false, nudge: 0 };

/**
 * Drop the hanging container so it lands `offset` from the centre of the container below, whatever
 * the swell is doing: a rehearsal on a copy finds when it lands (that doesn't depend on where), then
 * the crane is put where it needs to be for the real drop.
 */
function dropAt(s: Stack, offset: number): Stack {
  const rehearsal = structuredClone(s);
  advance(rehearsal, { drop: true, nudge: 0 }, DT);
  while (rehearsal.falling) step(rehearsal, idle, DT);
  const level = s.blocks.length;
  const h = topHeight(s);
  const top = s.blocks[s.blocks.length - 1];
  const under = 50 + driftAt(s.seed, rehearsal.t) + h * swayAt(s.seed, rehearsal.t, level, s.weather) + top.x;
  // A miss (`offset` Infinity) goes to whichever side the crane can reach, just clear of the stack.
  const clear = top.w / 2 + s.crane!.w / 2 + 0.5;
  const want = Number.isFinite(offset) ? under + offset : under + clear <= 50 + SWING ? under + clear : under - clear;
  if (want < 50 - SWING || want > 50 + SWING) throw new Error(`the crane can't reach ${want}`);
  // The crane moves once more in the step that lets go; on its rising half x = 50 - SWING + s.
  const c = s.crane!;
  c.s = want - 50 + SWING - craneSpeed(level) * s.weather.crane * DT;
  advance(s, { drop: true, nudge: 0 }, DT);
  while (s.falling) step(s, idle, DT);
  return s;
}

/** Let the next container swing in. */
function nextContainer(s: Stack): Stack {
  while (!s.crane && !s.over) step(s, idle, DT);
  return s;
}

describe("slicing", () => {
  const top = { x: 0, w: 36, kind: "slate" as const };

  it("cuts off whatever overhangs, on either side", () => {
    expect(resolveDrop(top, 10, 36)).toEqual({ kind: "slice", block: { x: 5, w: 26, kind: "slate" }, cut: { x: 23, w: 10 } });
    expect(resolveDrop(top, -10, 36)).toEqual({ kind: "slice", block: { x: -5, w: 26, kind: "slate" }, cut: { x: -23, w: 10 } });
  });

  it("misses when nothing (or only a sliver) is on the stack", () => {
    expect(resolveDrop(top, 36, 36)).toEqual({ kind: "miss" });
    expect(resolveDrop(top, -35.5, 36)).toEqual({ kind: "miss" });
    const sliver = resolveDrop(top, 34.5, 36);
    expect(sliver.kind).toBe("slice");
    if (sliver.kind === "slice") expect(sliver.block.w).toBeCloseTo(1.5);
  });

  it("narrows the stack in play: the next container is the width that held", () => {
    const s = newStack(7);
    dropAt(s, 6);
    expect(s.events).toContain("slice");
    expect(stackScore(s)).toBe(1);
    expect(s.blocks[1].w).toBeCloseTo(START_W - 6, 5);
    expect(s.pieces).toHaveLength(1);
    expect(s.pieces[0].w).toBeCloseTo(6, 5);
    nextContainer(s);
    expect(s.crane!.w).toBeCloseTo(START_W - 6, 5);
  });
});

describe("perfect drops", () => {
  const top = { x: 2, w: 30, kind: "teal" as const };

  it("forgive a few pixels either way and sit square on the one below", () => {
    expect(resolveDrop(top, 2 + PERFECT, 30)).toEqual({ kind: "perfect", block: { x: 2, w: 30, kind: "teal" } });
    expect(resolveDrop(top, 2 - PERFECT, 30).kind).toBe("perfect");
    expect(resolveDrop(top, 2 + PERFECT + 0.01, 30).kind).toBe("slice");
  });

  it("build a combo that a slice or a miss resets", () => {
    const s = newStack(11);
    for (let i = 1; i <= 3; i++) {
      dropAt(s, 0.5);
      expect(s.combo).toBe(i);
      nextContainer(s);
    }
    expect(s.blocks.every((b) => b.w === START_W)).toBe(true);
    dropAt(s, 5);
    expect(s.combo).toBe(0);
    expect(s.bestCombo).toBe(3);
    expect(s.perfects).toBe(3);
  });

  it("win width back every four in a row, never past the starting width", () => {
    expect(restoredWidth(20, 4)).toBe(20 + RESTORE_W);
    expect(restoredWidth(20, 3)).toBe(20);
    expect(restoredWidth(20, 8)).toBe(20 + RESTORE_W);
    expect(restoredWidth(START_W - 1, 4)).toBe(START_W);
    expect(restoredWidth(START_W, 4)).toBe(START_W);
    expect(restoredWidth(20, 0)).toBe(20);

    const s = newStack(3);
    dropAt(s, 10); // narrower: 26 wide
    nextContainer(s);
    for (let i = 0; i < 4; i++) {
      dropAt(s, -0.4);
      nextContainer(s);
    }
    expect(s.events).not.toContain("miss");
    const widths = s.blocks.map((b) => Math.round(b.w * 100) / 100);
    expect(widths).toEqual([36, 26, 26, 26, 26, 30]);
  });
});

describe("misses and lives", () => {
  it("cost a life each, and the third ends the game once the container has splashed", () => {
    const s = newStack(5);
    for (let i = 1; i <= LIVES; i++) {
      dropAt(s, Infinity);
      expect(s.events).toContain("miss");
      expect(s.lives).toBe(LIVES - i);
      expect(s.misses).toBe(i);
      if (i < LIVES) nextContainer(s);
    }
    expect(s.over).toBe(true);
    expect(stackScore(s)).toBe(0);
    // The last container is still in the air: the results wait for the splash.
    expect(settled(s)).toBe(false);
    for (let i = 0; i < 120 && !settled(s); i++) step(s, idle, DT);
    expect(settled(s)).toBe(true);
  });
});

describe("milestones", () => {
  it("celebrate 10, 25, 50 and 100 with a gold container", () => {
    expect(MILESTONES).toEqual([10, 25, 50, 100]);
    for (const m of MILESTONES) {
      expect(kindFor(1, m, "coral")).toBe("gold");
      expect(milestoneLine(m)).toMatch(/\S/);
    }
    expect(milestoneLine(11)).toBeNull();
    const s = newStack(21);
    for (let i = 0; i < 10; i++) {
      dropAt(s, 0);
      if (i < 9) expect(s.events).not.toContain("milestone");
      nextContainer(s);
    }
    expect(stackScore(s)).toBe(10);
    expect(s.cheer?.score).toBe(10);
    expect(s.blocks[10].kind).toBe("gold");
  });

  it("colours each container differently from the one below", () => {
    for (let level = 1; level < 60; level++) {
      for (const below of ["coral", "teal", "amber", "slate", "green"] as const) {
        const k = kindFor(99, level, below);
        if (k !== "gold") expect(k).not.toBe(below);
      }
    }
  });
});

describe("the difficulty curve", () => {
  it("speeds the crane up smoothly, towards a top speed", () => {
    expect(craneSpeed(0)).toBe(SPEED_START);
    let last = 0;
    for (let level = 0; level <= 200; level += 5) {
      const v = craneSpeed(level);
      expect(v).toBeGreaterThan(last);
      expect(v).toBeLessThan(SPEED_MAX);
      last = v;
    }
    expect(craneSpeed(60)).toBeGreaterThan(80);
  });

  it("rocks a taller stack harder, and harder still in the wind", () => {
    expect(swayAmp(1)).toBeLessThan(0.007);
    expect(swayAmp(40)).toBeGreaterThan(2 * swayAmp(1));
    const windy = stackWeather({ ...CALM, drift: 9 });
    expect(swayAmp(40, windy)).toBeGreaterThan(swayAmp(40));
    // A low stack barely notices the wind.
    expect(swayAmp(0, windy)).toBe(swayAmp(0));
  });

  it("sends gusts only once the stack is tall, with a warning first", () => {
    for (let t = 0; t < 120; t += 0.05) {
      expect(gustAt(42, t, GUST_FROM - 1)).toBe(0);
      expect(gustWarning(42, t, GUST_FROM - 1)).toBe(0);
    }
    let first = -1;
    for (let t = 0; t < 30 && first < 0; t += 0.01) if (gustAt(42, t, 30) !== 0) first = t;
    expect(first).toBeGreaterThan(5);
    const dir = Math.sign(gustAt(42, first + 0.5, 30));
    expect(gustWarning(42, first - 0.5, 30)).toBe(dir);
  });

  it("lets a nudge slow the crane down or speed it up, a little", () => {
    const run = (nudge: number) => {
      const s = newStack(1);
      const s0 = s.crane!.s;
      for (let i = 0; i < 60; i++) step(s, { drop: false, nudge }, DT);
      return s.crane!.s - s0;
    };
    expect(run(1) / run(0)).toBeCloseTo(1 + NUDGE, 5);
    expect(run(-1) / run(0)).toBeCloseTo(1 - NUDGE, 5);
    expect(run(9)).toBeCloseTo(run(1), 5);
  });

  it("lets go by itself if nobody drops (nobody can stall a stack-off)", () => {
    const s = newStack(1);
    for (let t = 0; t < HOLD_S - 0.1; t += DT) step(s, idle, DT);
    expect(s.crane).not.toBeNull();
    for (let i = 0; i < 12; i++) step(s, idle, DT);
    expect(s.crane).toBeNull();
    expect(s.falling).not.toBeNull();
  });
});

describe("determinism", () => {
  it("sways the same for the same seed, and differently for another", () => {
    for (const t of [0, 1.3, 7.77, 60]) expect(swayAt(9, t, 20)).toBe(swayAt(9, t, 20));
    const a = Array.from({ length: 20 }, (_, i) => swayAt(9, i * 0.37, 20));
    const b = Array.from({ length: 20 }, (_, i) => swayAt(10, i * 0.37, 20));
    expect(a).not.toEqual(b);
  });

  it("plays out exactly the same from the same seed and the same presses", () => {
    const play = (seed: number) => {
      const s = newStack(seed);
      for (let i = 0; i < 60 * 45 && !s.over; i++) step(s, { drop: i % 83 === 0, nudge: i % 300 < 50 ? 0.5 : 0 }, DT);
      return s;
    };
    const one = play(2024);
    const two = play(2024);
    expect(JSON.stringify(two)).toBe(JSON.stringify(one));
    expect(stackScore(one) + one.misses).toBeGreaterThanOrEqual(4);
    expect(JSON.stringify(play(2025))).not.toBe(JSON.stringify(one));
  });

  it("counts a drop press once, however the frame is split", () => {
    const s = newStack(4);
    advance(s, { drop: true, nudge: 0 }, 0.1);
    expect(s.events.filter((e) => e === "drop")).toHaveLength(1);
  });
});

describe("the crane", () => {
  it("swings back and forth across the middle", () => {
    expect(craneX(0)).toBe(50 - SWING);
    expect(craneX(SWING)).toBe(50);
    expect(craneX(2 * SWING)).toBe(50 + SWING);
    expect(craneX(3 * SWING)).toBe(50);
    expect(craneX(4 * SWING)).toBe(50 - SWING);
  });

  it("hangs the container a fixed drop above the stack, and the view rises with it", () => {
    const s = newStack(8);
    advance(s, { drop: true, nudge: 0 }, DT);
    expect(s.falling!.h).toBe(topHeight(s) + CRANE_GAP);
    expect(cameraFor(BOX_H)).toBe(0);
    expect(cameraFor(200)).toBeGreaterThan(0);
  });
});

describe("weather", () => {
  it("is calm without a forecast, and Tugboat Run's wind, rain and snow with one", () => {
    expect(stackWeather(CALM)).toEqual(CALM_STACK);
    const gale = stackWeather({ ...CALM, drift: -9, waves: 11, speedScale: 0.82 });
    expect(gale.lean).toBeLessThan(0);
    expect(gale.wind).toBe(1);
    expect(gale.chop).toBeGreaterThan(0);
    expect(gale.crane).toBe(0.82);
  });

  it("says calm seas when the weather is off", () => {
    expect(conditionsLine(CALM_SEAS, "c", stackOutlook)).toBe("Calm seas today");
    const night: Conditions = { live: true, sky: "rain", windKmh: 40, windFrom: 270, night: true, rain: 2, snow: false };
    const line = conditionsLine(night, "c", stackOutlook);
    expect(line).toMatch(/^Tonight: rainy/);
    expect(line).toContain("choppy water");
    expect(line).toContain("wind leaning on the stack");
    expect(line).toContain("a lighthouse");
    // Tugboat Run's own line is unchanged.
    expect(conditionsLine(night, "c")).toContain("a crosswind");
    expect(stackWeather(handlingFor(night)).wind).toBeGreaterThan(0);
  });
});

describe("the daily stack", () => {
  it("has its own seed for the day, the same on every PC", () => {
    expect(dailySeed("2026-10-17", "harbor-stack")).toBe(dailySeed("2026-10-17", "harbor-stack"));
    expect(dailySeed("2026-10-17", "harbor-stack")).not.toBe(dailySeed("2026-10-17"));
    expect(dailySeed("2026-10-17", "harbor-stack")).not.toBe(dailySeed("2026-10-18", "harbor-stack"));
    // Tugboat Run's course is untouched by the change.
    expect(dailySeed("2026-10-17")).toBe(dailySeed("2026-10-17", "tugboat-run"));
    // The same seed is the same stack for everyone.
    expect(JSON.stringify(newStack(dailySeed("2026-10-17", "harbor-stack")))).toBe(JSON.stringify(newStack(dailySeed("2026-10-17", "harbor-stack"))));
  });

  it("keeps a streak with Tugboat Run's rules", () => {
    let rec = finishDaily(EMPTY_DAILY, "2026-10-16", 12).record;
    rec = finishDaily(rec, "2026-10-17", 20).record;
    expect(currentStreak(rec, "2026-10-17")).toBe(2);
    expect(rec.bests["2026-10-17"]).toBe(20);
  });
});

describe("controls", () => {
  it("nudges from held keys first, then the phone", () => {
    expect(nudgeFrom({ keyLeft: true, keyRight: false, pad: { connected: true, steer: 0.5 } })).toBe(-1);
    expect(nudgeFrom({ keyLeft: false, keyRight: false, pad: { connected: true, steer: 0.5 } })).toBe(0.5);
    expect(nudgeFrom({ keyLeft: false, keyRight: false, pad: { connected: false, steer: 0.5 } })).toBe(0);
    expect(nudgeFrom({ keyLeft: false, keyRight: false, pad: null })).toBe(0);
  });

  it("gives each player of a stack-off their own drop key", () => {
    expect(stackOffKey("ArrowDown")).toBe(1);
    expect(stackOffKey("Enter")).toBe(1);
    expect(stackOffKey("s")).toBe(2);
    expect(stackOffKey("S")).toBe(2);
    expect(stackOffKey(" ")).toBeNull();
  });
});
