import { describe, expect, it } from "vitest";
import {
  BOAT_Y,
  FIELD_H,
  FIELD_W,
  GRACE_S,
  HOP_S,
  LIVES,
  MARGIN,
  MAX_VX,
  MIN_GAP,
  SPEED_MAX,
  SPEED_START,
  FOLLOW_TAU,
  CALM,
  LANE_GAP,
  RESPONSE,
  advance,
  distanceAt,
  laneLayouts,
  follow,
  layout,
  mergeInput,
  newRun,
  padTarget,
  random,
  score,
  spawnGap,
  spawnRow,
  speedAt,
  step,
  steerVelocity,
  timeAtDistance,
  toFieldX,
  touches,
  waveAt,
  widestGap,
  type Controls,
  type Handling,
  type Run,
  type Thing,
} from "./tugboatRun";

const still = { steer: 0, boost: false };

function obstacle(kind: Thing["kind"], x: number, y: number, w = 6, h = 6): Thing {
  return { id: 999, kind, x, y, w, h, vx: 0, vy: 0, hit: false };
}

/** Play `seconds` in 1/60 s frames. */
function runFor(run: Run, input: { steer: number; boost: boolean }, seconds: number) {
  for (let i = 0; i < Math.round(seconds * 60); i++) advance(run, input, 1 / 60);
}

/** A run with nothing spawning, for placing things by hand. */
function quiet(seed = 1): Run {
  const run = newRun(seed);
  run.nextSpawn = Infinity;
  return run;
}

describe("random numbers", () => {
  it("are repeatable from a seed and stay in [0, 1)", () => {
    const a = { seed: 42 };
    const b = { seed: 42 };
    const xs = Array.from({ length: 1000 }, () => random(a));
    expect(Array.from({ length: 1000 }, () => random(b))).toEqual(xs);
    expect(xs.every((x) => x >= 0 && x < 1)).toBe(true);
    // Roughly uniform.
    const mean = xs.reduce((s, x) => s + x, 0) / xs.length;
    expect(mean).toBeGreaterThan(0.45);
    expect(mean).toBeLessThan(0.55);
  });
});

describe("collisions", () => {
  it("hits a buoy dead ahead and misses one off to the side", () => {
    expect(touches(50, obstacle("buoy", 50, BOAT_Y))).toBe(true);
    expect(touches(50, obstacle("buoy", 50 + 10, BOAT_Y))).toBe(false);
    // Just above the bow: not yet.
    expect(touches(50, obstacle("buoy", 50, BOAT_Y - 12))).toBe(false);
  });

  it("grazing the hull's corner with a round thing is a near miss, not a hit", () => {
    // The circle's box overlaps the hitbox corner, but the circle itself doesn't.
    expect(touches(50, obstacle("buoy", 50 + 5.4, BOAT_Y + 8))).toBe(false);
  });

  it("logs and boats are boxes", () => {
    expect(touches(50, obstacle("log", 62, BOAT_Y, 22, 4.5))).toBe(true);
    expect(touches(50, obstacle("log", 70, BOAT_Y, 22, 4.5))).toBe(false);
    expect(touches(50, obstacle("boat", 50, BOAT_Y - 10, 8, 15))).toBe(true);
  });
});

describe("the speed curve", () => {
  it("starts gentle, only rises, and never passes the top speed", () => {
    expect(speedAt(0)).toBe(SPEED_START);
    let last = speedAt(0);
    for (let t = 1; t <= 600; t++) {
      const v = speedAt(t);
      expect(v).toBeGreaterThan(last);
      expect(v).toBeLessThan(SPEED_MAX);
      last = v;
    }
    // Noticeably faster after a minute, nearly flat out after five.
    expect(speedAt(60)).toBeGreaterThan(SPEED_START + 0.5 * (SPEED_MAX - SPEED_START) - 10);
    expect(speedAt(300)).toBeGreaterThan(SPEED_MAX - 5);
  });

  it("rows come closer together as the run goes on", () => {
    expect(spawnGap(0)).toBeGreaterThan(spawnGap(60));
    expect(spawnGap(60)).toBeGreaterThan(spawnGap(180));
    expect(spawnGap(10_000)).toBeGreaterThanOrEqual(30);
  });
});

describe("inertia", () => {
  it("eases towards full helm without overshooting, at any frame rate", () => {
    let fast = 0;
    let slow = 0;
    for (let i = 0; i < 120; i++) fast = steerVelocity(fast, 1, 1 / 120);
    for (let i = 0; i < 30; i++) slow = steerVelocity(slow, 1, 1 / 30);
    // One second either way lands in the same place.
    expect(fast).toBeCloseTo(slow, 6);
    expect(fast).toBeGreaterThan(0.99 * MAX_VX);
    expect(fast).toBeLessThanOrEqual(MAX_VX);
    // A tenth of a second in, it's still building up: that's the inertia.
    expect(steerVelocity(0, 1, 0.1)).toBeLessThan(0.6 * MAX_VX);
  });

  it("drifts to a stop when the helm is let go, and ignores nonsense steering", () => {
    let v = MAX_VX;
    for (let i = 0; i < 60; i++) v = steerVelocity(v, 0, 1 / 60);
    expect(Math.abs(v)).toBeLessThan(1);
    expect(steerVelocity(0, 5, 10)).toBeCloseTo(MAX_VX, 6);
    expect(steerVelocity(0, Number.NaN, 1)).toBe(0);
  });

  it("the banks stop the boat", () => {
    const run = quiet();
    for (let i = 0; i < 300; i++) step(run, { steer: -1, boost: false }, 1 / 60);
    expect(run.x).toBe(MARGIN);
    expect(run.vx).toBeGreaterThanOrEqual(0);
  });
});

describe("spawning", () => {
  it("is the same for the same seed", () => {
    const a = newRun(7);
    const b = newRun(7);
    for (let i = 0; i < 60 * 30; i++) {
      step(a, still, 1 / 60);
      step(b, still, 1 / 60);
    }
    expect(a.things).toEqual(b.things);
    expect(newRun(8).seed).not.toBe(a.seed);
  });

  it("paces rows by distance travelled, getting busier over time", () => {
    const rowsIn = (fromT: number, seconds: number) => {
      const run = newRun(3);
      run.t = fromT;
      run.distance = distanceAt(fromT);
      run.nextSpawn = run.distance;
      let rows = 0;
      for (let i = 0; i < seconds * 60; i++) {
        const before = run.nextSpawn;
        step(run, still, 1 / 60);
        if (run.nextSpawn !== before) rows++;
        run.lives = LIVES; // never end
      }
      return rows;
    };
    const early = rowsIn(0, 20);
    const late = rowsIn(180, 20);
    expect(early).toBeGreaterThan(5);
    expect(late).toBeGreaterThan(early * 1.8);
  });

  it("always leaves a way through", () => {
    for (let seed = 1; seed <= 200; seed++) {
      const run = newRun(seed);
      for (const t of [0, 30, 90, 240, 600]) {
        run.nextSpawn = distanceAt(t);
        for (let i = 0; i < 20; i++) {
          run.things = [];
          const row = spawnRow(run);
          expect(widestGap(row)).toBeGreaterThanOrEqual(MIN_GAP);
          expect(row.every((th) => th.x >= 0 && th.x <= FIELD_W)).toBe(true);
        }
      }
    }
  });

  it("only offers a life ring when a life has been lost", () => {
    for (let seed = 1; seed <= 300; seed++) {
      const run = newRun(seed);
      expect(spawnRow(run).some((th) => th.kind === "ring")).toBe(false);
    }
    let rings = 0;
    for (let seed = 1; seed <= 300; seed++) {
      const run = newRun(seed);
      run.lives = 1;
      if (spawnRow(run).some((th) => th.kind === "ring")) rings++;
    }
    expect(rings).toBeGreaterThan(0);
  });

  it("difficulty follows distance: the distance curve and its inverse agree", () => {
    for (const t of [0, 1, 10, 45, 90, 300, 900]) expect(timeAtDistance(distanceAt(t))).toBeCloseTo(t, 6);
    expect(timeAtDistance(-5)).toBe(0);
    expect(distanceAt(60)).toBeGreaterThan(distanceAt(30));
  });

  it("the same seed is the same course at any frame rate, whatever the boat does", () => {
    /** Each thing as it first appears: what it is, where across, how big. */
    const course = (fps: number, steer: number, hop: boolean) => {
      const run = newRun(20261016);
      const seen: Array<[string, number, number]> = [];
      let last = 0;
      for (let i = 0; i < 40 * fps; i++) {
        step(run, { steer, boost: hop && i % 30 === 0 }, 1 / fps);
        for (const th of run.things) if (th.id > last) seen.push([th.kind, th.x, th.w]);
        last = run.nextId - 1;
        // Never sink and never be hurt, so no life ring takes a row's place.
        run.lives = LIVES;
        run.over = false;
      }
      return seen;
    };
    const steady = course(60, 0, false);
    expect(steady.length).toBeGreaterThan(30);
    // 144 frames a second, steering hard and hopping: the same things in the same places.
    expect(course(144, 1, true)).toEqual(steady);
    expect(course(30, -1, false)).toEqual(steady);
  });

  it("a boat that's been hit may find a life ring, but the rest of the course is the same", () => {
    const fine = newRun(99);
    const hurt = newRun(99);
    hurt.lives = 1;
    let rings = 0;
    for (let i = 0; i < 400; i++) {
      const a = spawnRow(fine).map((th) => [th.kind, th.x, th.w]);
      const b = spawnRow(hurt).map((th) => [th.kind, th.x, th.w]);
      if (b.length === 1 && b[0][0] === "ring") rings++;
      else expect(b).toEqual(a);
      // The next row comes at the same distance either way.
      expect(hurt.nextSpawn).toBe(fine.nextSpawn);
    }
    expect(rings).toBeGreaterThan(5);
  });

  it("different seeds are different courses", () => {
    const rows = (seed: number) => {
      const run = newRun(seed);
      return Array.from({ length: 12 }, () => spawnRow(run).map((th) => `${th.kind}@${th.x.toFixed(2)}`)).join("|");
    };
    expect(rows(1)).toBe(rows(1));
    expect(rows(1)).not.toBe(rows(2));
  });

  it("forgets things once they've floated off the bottom", () => {
    const run = quiet();
    run.things.push(obstacle("buoy", 10, FIELD_H + 30));
    step(run, still, 1 / 60);
    expect(run.things).toHaveLength(0);
  });
});

describe("hits, lives and grace", () => {
  it("a hit costs a life, then grace forgives the next few", () => {
    const run = quiet();
    run.things.push(obstacle("buoy", run.x, BOAT_Y));
    step(run, still, 1 / 60);
    expect(run.lives).toBe(LIVES - 1);
    expect(run.events).toContain("hit");
    expect(run.hits).toBe(1);
    run.things.push(obstacle("buoy", run.x, BOAT_Y));
    step(run, still, 1 / 60);
    expect(run.lives).toBe(LIVES - 1);
    // Once grace is over, it counts again.
    run.things = [];
    runFor(run, still, GRACE_S + 0.05);
    run.things.push(obstacle("buoy", run.x, BOAT_Y));
    step(run, still, 1 / 60);
    expect(run.lives).toBe(LIVES - 2);
  });

  it("an obstacle struck once never hits again", () => {
    const run = quiet();
    const b = obstacle("log", run.x, BOAT_Y, 20, 4.5);
    run.things.push(b);
    step(run, still, 1 / 60);
    expect(b.hit).toBe(true);
    run.grace = 0;
    step(run, still, 1 / 60);
    expect(run.lives).toBe(LIVES - 1);
  });

  it("the last life ends the run", () => {
    const run = quiet();
    run.lives = 1;
    run.things.push(obstacle("buoy", run.x, BOAT_Y));
    step(run, still, 1 / 60);
    expect(run.over).toBe(true);
    expect(run.events).toContain("over");
    const t = run.t;
    step(run, still, 1);
    expect(run.t).toBe(t);
  });

  it("a hop clears buoys and logs but not boats", () => {
    const run = quiet();
    step(run, { steer: 0, boost: true }, 1 / 60);
    expect(run.events).toContain("hop");
    run.things.push(obstacle("buoy", run.x, BOAT_Y), obstacle("log", run.x, BOAT_Y, 20, 4.5));
    step(run, { steer: 0, boost: true }, 1 / 60);
    expect(run.lives).toBe(LIVES);
    run.things.push(obstacle("boat", run.x, BOAT_Y, 8, 15));
    step(run, { steer: 0, boost: true }, 1 / 60);
    expect(run.lives).toBe(LIVES - 1);
  });

  it("holding boost hops once; it has to be pressed again after the cooldown", () => {
    const run = quiet();
    runFor(run, { steer: 0, boost: true }, 3);
    expect(run.events.filter((e) => e === "hop")).toHaveLength(1);
    expect(run.hop).toBe(0);
    run.events = [];
    step(run, still, 1 / 60);
    step(run, { steer: 0, boost: true }, 1 / 60);
    expect(run.events).toContain("hop");
    expect(run.hop).toBeGreaterThan(HOP_S - 0.05);
  });
});

describe("scoring", () => {
  it("counts distance, coins and spare life rings", () => {
    const run = quiet();
    runFor(run, still, 1);
    const base = score(run);
    expect(base).toBe(Math.floor(run.distance / 6));
    run.things.push(obstacle("coin", run.x, BOAT_Y, 4.4, 4.4));
    step(run, still, 1 / 60);
    expect(run.coins).toBe(1);
    expect(run.events).toContain("coin");
    expect(run.things).toHaveLength(0);
    // A life ring at full lives is points instead.
    run.things.push(obstacle("ring", run.x, BOAT_Y, 6.5, 6.5));
    step(run, still, 1 / 60);
    expect(run.lives).toBe(LIVES);
    expect(score(run)).toBeGreaterThanOrEqual(base + 10 + 50);
  });

  it("a life ring gives back a lost life", () => {
    const run = quiet();
    run.lives = 1;
    run.things.push(obstacle("ring", run.x, BOAT_Y, 6.5, 6.5));
    step(run, still, 1 / 60);
    expect(run.lives).toBe(2);
  });
});

describe("a whole run", () => {
  it("someone who never steers is done within three minutes", () => {
    for (const seed of [1, 2, 3, 4, 5, 6, 7, 8]) {
      const run = newRun(seed);
      while (!run.over && run.t < 600) advance(run, still, 1 / 60);
      expect(run.over).toBe(true);
      expect(run.t).toBeGreaterThan(5);
      expect(run.t).toBeLessThan(180);
    }
  });

  it("a careful dodger lasts longer than someone who never steers", () => {
    let idle = 0;
    let dodger = 0;
    for (const seed of [11, 12, 13, 14, 15]) {
      const a = newRun(seed);
      while (!a.over && a.t < 600) advance(a, still, 1 / 60);
      idle += a.t;
      const b = newRun(seed);
      while (!b.over && b.t < 600) advance(b, { steer: dodge(b), boost: false }, 1 / 60);
      dodger += b.t;
    }
    expect(dodger).toBeGreaterThan(idle);
  });
});

/** A simple bot: steer away from the nearest obstacle coming at the boat. */
function dodge(run: Run): number {
  const ahead = run.things
    .filter((th) => !th.hit && th.kind !== "coin" && th.kind !== "ring" && th.y < BOAT_Y && th.y > BOAT_Y - 45)
    .sort((a, b) => b.y - a.y)[0];
  if (!ahead || Math.abs(ahead.x - run.x) > ahead.w / 2 + 8) return 0;
  const goLeft = ahead.x > run.x ? run.x - MARGIN > 10 : run.x < FIELD_W - MARGIN - 10 ? false : true;
  return goLeft ? -1 : 1;
}

describe("the phone's slider", () => {
  it("puts the boat anywhere across the open water", () => {
    expect(padTarget(0)).toBe(FIELD_W / 2);
    expect(padTarget(-1)).toBe(MARGIN);
    expect(padTarget(1)).toBe(FIELD_W - MARGIN);
    expect(padTarget(7)).toBe(FIELD_W - MARGIN);
    expect(padTarget(Number.NaN)).toBe(FIELD_W / 2);
  });

  it("is followed quickly and without overshoot, at any frame rate", () => {
    const run = (fps: number, seconds: number) => {
      let x = 20;
      let v = 0;
      let peak = x;
      for (let i = 0; i < Math.round(seconds * fps); i++) {
        [x, v] = follow(x, v, 80, 1 / fps);
        peak = Math.max(peak, x);
      }
      return { x, peak };
    };
    // Most of the way there in a few time constants (~0.15 s), all the way soon after.
    expect(run(60, 3 * FOLLOW_TAU).x).toBeGreaterThan(20 + 0.75 * 60);
    expect(run(60, 0.3).x).toBeGreaterThan(78.5);
    expect(run(60, 0.5).x).toBeCloseTo(80, 1);
    // Critically damped: never past the target.
    expect(run(120, 1).peak).toBeLessThanOrEqual(80 + 1e-9);
    // The same at 30, 60 and 120 frames a second.
    expect(run(30, 0.1).x).toBeCloseTo(run(120, 0.1).x, 6);
  });

  it("moves the boat much more directly than the keyboard's helm", () => {
    const pad = quiet();
    const keys = quiet();
    for (let i = 0; i < 6; i++) {
      step(pad, { steer: 0, boost: false, target: FIELD_W - MARGIN }, 1 / 60);
      step(keys, { steer: 1, boost: false }, 1 / 60);
    }
    // A tenth of a second in: the slider has the boat well on its way; the helm is still building.
    expect(pad.x - FIELD_W / 2).toBeGreaterThan(2.5 * (keys.x - FIELD_W / 2));
    // And it settles exactly where the knob is.
    for (let i = 0; i < 60; i++) step(pad, { steer: 0, boost: false, target: 30 }, 1 / 60);
    expect(pad.x).toBeCloseTo(30, 3);
  });
});

describe("the water's handling (the daily challenge's weather)", () => {
  const withHandling = (h: Partial<Handling>) => {
    const run = newRun(1, { ...CALM, ...h });
    run.nextSpawn = Infinity;
    return run;
  };

  it("calm water is the game as it always was", () => {
    const a = quiet();
    const b = withHandling({});
    for (let i = 0; i < 120; i++) {
      step(a, { steer: 0.5, boost: false }, 1 / 60);
      step(b, { steer: 0.5, boost: false }, 1 / 60);
    }
    expect(b.x).toBe(a.x);
    expect(b.distance).toBe(a.distance);
  });

  it("snow slows the water and makes the helm slide", () => {
    const calm = quiet();
    const snow = withHandling({ speedScale: 0.82, response: RESPONSE * 0.38 });
    for (let i = 0; i < 6; i++) {
      step(calm, { steer: 1, boost: false }, 1 / 60);
      step(snow, { steer: 1, boost: false }, 1 / 60);
    }
    expect(snow.distance).toBeCloseTo(calm.distance * 0.82, 6);
    // A tenth of a second in, the icy boat has barely started to turn.
    expect(snow.vx).toBeLessThan(calm.vx * 0.6);
    // It still turns, just later.
    for (let i = 0; i < 30; i++) step(snow, { steer: 1, boost: false }, 1 / 60);
    expect(snow.vx).toBeGreaterThan(0.5 * MAX_VX);
  });

  it("wind pushes an idle boat downwind, and the slider holds against it", () => {
    const run = withHandling({ drift: 6 });
    for (let i = 0; i < 60; i++) step(run, still, 1 / 60);
    expect(run.x).toBeCloseTo(FIELD_W / 2 + 6, 1);
    const left = withHandling({ drift: -6 });
    for (let i = 0; i < 60; i++) step(left, still, 1 / 60);
    expect(left.x).toBeLessThan(FIELD_W / 2 - 5);
    // The phone's slider keeps the boat within a unit of where the knob is.
    const pad = withHandling({ drift: 9 });
    for (let i = 0; i < 120; i++) step(pad, { steer: 0, boost: false, target: 30 }, 1 / 60);
    expect(Math.abs(pad.x - 30)).toBeLessThan(1);
  });

  it("waves rock the boat from side to side without carrying it away", () => {
    const run = withHandling({ waves: 8 });
    let lo = run.x;
    let hi = run.x;
    for (let i = 0; i < 60 * 13; i++) {
      step(run, still, 1 / 60);
      lo = Math.min(lo, run.x);
      hi = Math.max(hi, run.x);
    }
    expect(hi - lo).toBeGreaterThan(3);
    expect(Math.abs(run.x - FIELD_W / 2)).toBeLessThan(8);
    expect(Math.abs(waveAt(1.234))).toBeLessThanOrEqual(1);
  });
});

describe("controls", () => {
  const none: Controls = { keyLeft: false, keyRight: false, keyBoost: false, pointerX: null, pointerBoost: false, pad: null, padLeads: false };
  const pad = { connected: true, steer: 0.5, boost: false };

  it("the keyboard wins whenever a key is held", () => {
    expect(mergeInput({ ...none, keyLeft: true, pad, padLeads: true }, 50)).toEqual({ steer: -1, boost: false });
    expect(mergeInput({ ...none, keyLeft: true, pointerX: 90 }, 50).steer).toBe(-1);
  });

  it("a mouse drag steers towards the pointer, gently near the boat", () => {
    expect(mergeInput({ ...none, pointerX: 90 }, 50).steer).toBe(1);
    expect(mergeInput({ ...none, pointerX: 45 }, 50).steer).toBeCloseTo(-0.5);
  });

  it("the phone's slider sets where the boat goes, once it has moved last", () => {
    expect(mergeInput({ ...none, pad, padLeads: true }, 50)).toEqual({ steer: 0, boost: false, target: padTarget(0.5) });
    // The keys were used after the slider last moved: the slider waits its turn.
    expect(mergeInput({ ...none, pad, padLeads: false }, 50)).toEqual({ steer: 0, boost: false });
    // Not connected: nothing from the phone at all.
    expect(mergeInput({ ...none, pad: { connected: false, steer: 1, boost: true }, padLeads: true }, 50)).toEqual({ steer: 0, boost: false });
  });

  it("Boost from the phone hops whoever has the helm", () => {
    expect(mergeInput({ ...none, keyRight: true, pad: { ...pad, boost: true } }, 50)).toEqual({ steer: 1, boost: true });
  });
});
describe("layout", () => {
  it("fits the field in the panel, centred, and maps the pointer back", () => {
    const wide = layout(1200, 640);
    expect(wide.scale).toBe(4);
    expect(wide.left).toBe((1200 - FIELD_W * 4) / 2);
    expect(wide.top).toBe(0);
    expect(toFieldX(wide.left + 50 * 4, wide)).toBe(50);
    const narrow = layout(300, 900);
    expect(narrow.scale).toBe(3);
    expect(narrow.left).toBe(0);
    expect(layout(0, 0).scale).toBeGreaterThan(0);
  });

  it("two lanes sit side by side at the size one would be, at a usual window shape", () => {
    const [a, b] = laneLayouts(1200, 640);
    expect(a.scale).toBe(4);
    expect(b.scale).toBe(a.scale);
    expect(b.left - a.left).toBe((FIELD_W + LANE_GAP) * a.scale);
    // Centred together.
    expect(a.left).toBeCloseTo(1200 - (b.left + FIELD_W * b.scale), 6);
    // A narrow window shrinks them to fit across.
    const [n] = laneLayouts(500, 900);
    expect(n.scale).toBeCloseTo(500 / (2 * FIELD_W + LANE_GAP), 6);
    expect(n.left).toBeCloseTo(0, 6);
  });
});
