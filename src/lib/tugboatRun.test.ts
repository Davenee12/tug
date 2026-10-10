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
  advance,
  layout,
  mergeInput,
  newRun,
  random,
  score,
  spawnGap,
  spawnRow,
  speedAt,
  step,
  steerVelocity,
  toFieldX,
  touches,
  widestGap,
  type Controls,
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
      let rows = 0;
      run.nextSpawn = 0;
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
        run.t = t;
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

describe("controls", () => {
  const none: Controls = { keyLeft: false, keyRight: false, keyBoost: false, pointerX: null, pointerBoost: false, pad: null };

  it("the keyboard wins whenever a key is held", () => {
    const pad = { connected: true, steer: 0.8, boost: false };
    expect(mergeInput({ ...none, keyLeft: true, pad }, 50).steer).toBe(-1);
    expect(mergeInput({ ...none, keyLeft: true, pointerX: 90 }, 50).steer).toBe(-1);
    // Both arrows held cancel out, so the next control gets its say.
    expect(mergeInput({ ...none, keyLeft: true, keyRight: true, pad }, 50).steer).toBe(0.8);
  });

  it("a mouse drag steers towards the pointer, gently near the boat", () => {
    expect(mergeInput({ ...none, pointerX: 90 }, 50).steer).toBe(1);
    expect(mergeInput({ ...none, pointerX: 45 }, 50).steer).toBeCloseTo(-0.5);
  });

  it("the phone steers only while connected", () => {
    expect(mergeInput({ ...none, pad: { connected: true, steer: -0.3, boost: true } }, 50)).toEqual({ steer: -0.3, boost: true });
    expect(mergeInput({ ...none, pad: { connected: false, steer: -0.3, boost: true } }, 50)).toEqual({ steer: 0, boost: false });
    expect(mergeInput({ ...none, pad: { connected: true, steer: 9, boost: false } }, 50).steer).toBe(1);
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
});
