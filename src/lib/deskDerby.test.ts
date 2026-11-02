import { describe, expect, it } from "vitest";
import {
  BOOST_S,
  BOOST_V,
  CAR_R,
  CHARGE_LEVELS,
  CLIPS_PER_LAP,
  COUNTDOWN_S,
  DESK_H,
  DESK_W,
  FALL_S,
  FINISH_GRACE_S,
  GHOST_HZ,
  KEYS_SPEED,
  LAPS,
  MAX_S,
  PENALTY_S,
  STEP,
  TRACKS,
  TRACK_IDS,
  TURN,
  V_TOP,
  WOOD_SPEED,
  advance,
  autopilot,
  betterGhost,
  buildTrack,
  chargeLevel,
  clockOf,
  countdownLabel,
  dailyVariant,
  derbyWinner,
  duoKey,
  formatSplit,
  formatTime,
  freeVariant,
  ghostKey,
  ghostOf,
  ghostPose,
  inPuddle,
  inputFrom,
  nearest,
  newRace,
  onDesk,
  onKeyboard,
  onLane,
  onRamp,
  parseGhost,
  places,
  poseAt,
  raceMs,
  solidDistance,
  step,
  wrapAngle,
  type Car,
  type CarEvent,
  type CarInput,
  type Pose,
  type Race,
  type Track,
  type Variant,
} from "./deskDerby";
import { dailySeed } from "./tugboatDaily";

const straight: CarInput = { steer: 0, drift: false };

/** A race already past its countdown. */
function racing(t: Track, players: 1 | 2 = 1): Race {
  const r = newRace(t, players);
  while (r.t < 0) step(r, [straight, straight]);
  return r;
}

/** Put a car somewhere, pointing (and moving) one way, at a speed. */
function put(c: Car, p: Pose, speed = V_TOP) {
  c.x = p.x;
  c.y = p.y;
  c.angle = c.dir = p.angle;
  c.speed = speed;
}

/** Run for `s` seconds with an input (fixed, or from the race each step); collect the car's events. */
function run(r: Race, s: number, input: CarInput | ((r: Race) => CarInput), car = 0): CarEvent[] {
  const seen: CarEvent[] = [];
  const end = r.t + s;
  while (r.t < end - 1e-9 && !r.over) {
    const i = typeof input === "function" ? input(r) : input;
    step(r, car === 0 ? [i, straight] : [straight, i]);
    seen.push(...r.cars[car].events);
    for (const c of r.cars) c.events = [];
  }
  return seen;
}

/** Drive a whole race with the test driver. */
function drive(v: Variant, drift = true): Race {
  const r = newRace(buildTrack(v));
  while (!r.over) step(r, [autopilot(r, r.cars[0], { drift })]);
  return r;
}

const ALL: Variant[] = TRACK_IDS.flatMap((id) => [freeVariant(id), { ...freeVariant(id), mirror: true }]);

describe("the tracks", () => {
  it("are four hand-made layouts, each with a name and a line about it", () => {
    expect(TRACK_IDS).toEqual(["inbox", "coffee", "esses", "edge"]);
    for (const d of TRACKS) {
      expect(d.name.length).toBeGreaterThan(3);
      expect(d.blurb.length).toBeGreaterThan(10);
      expect(d.checkpoints[0]).toBe(0);
      expect([...d.checkpoints].sort()).toEqual([...d.checkpoints]);
    }
  });

  it.each(ALL)("$track (mirrored: $mirror) is a lap of sensible length, all on the desk, never crossing itself", (v) => {
    const t = buildTrack(v);
    expect(t.length).toBeGreaterThan(950);
    expect(t.length).toBeLessThan(1450);
    const n = t.xs.length;
    for (let i = 0; i < n; i++) {
      // The whole lane is on the desk (leaving it is going over the edge).
      expect(Math.min(t.xs[i], t.ys[i], DESK_W - t.xs[i], DESK_H - t.ys[i])).toBeGreaterThanOrEqual(t.half);
      // Lanes that pass each other have room between them for a pencil.
      for (let j = 0; j < n; j += 3) {
        const along = Math.min(Math.abs(i - j), n - Math.abs(i - j)) * (t.length / n);
        if (along > 60) expect(Math.hypot(t.xs[i] - t.xs[j], t.ys[i] - t.ys[j])).toBeGreaterThan(t.half * 2 + 8);
      }
    }
  });

  it.each(ALL)("$track (mirrored: $mirror) has no bend tighter than the lane is wide", (v) => {
    const t = buildTrack(v);
    for (let s = 0; s < t.length; s += 1.5) {
      const bend = Math.abs(wrapAngle(poseAt(t, s + 3).angle - poseAt(t, s - 3).angle));
      // Radius of the centre line: the inner tape never folds over itself.
      expect(6 / Math.max(bend, 1e-9)).toBeGreaterThan(t.half + 1);
    }
  });

  it.each(ALL)("$track (mirrored: $mirror): the car always fits past the mug, the mouse and the pencils", (v) => {
    const t = buildTrack(v);
    for (let i = 0; i < t.xs.length; i++) {
      for (const s of t.solids) expect(solidDistance(s, t.xs[i], t.ys[i])).toBeGreaterThan(CAR_R + 2);
    }
    // Pencils lie between lanes, never across one.
    for (const s of t.solids.filter((s) => s.type === "capsule")) {
      for (let k = 0; k <= 10; k++) {
        if (s.type !== "capsule") continue;
        const x = s.x1 + ((s.x2 - s.x1) * k) / 10;
        const y = s.y1 + ((s.y2 - s.y1) * k) / 10;
        expect(onLane(t, x, y)).toBe(false);
      }
    }
  });

  it.each(ALL)("$track (mirrored: $mirror): sticky notes, the stapler, the spill and clips are on the lane, apart", (v) => {
    const t = buildTrack(v);
    const def = TRACKS.find((d) => d.id === v.track)!;
    for (const cp of t.checkpoints) expect(onLane(t, cp.x, cp.y)).toBe(true);
    expect(onLane(t, t.ramp.x, t.ramp.y)).toBe(true);
    expect(onLane(t, t.puddle.x, t.puddle.y)).toBe(true);
    // Nothing sits on a sticky note.
    const spots = [def.stapler, ...def.puddles];
    for (const f of spots) for (const cp of def.checkpoints) expect(Math.abs(((f - cp + 1.5) % 1) - 0.5)).toBeGreaterThan(0.04);
    // The start line isn't on the keyboard, so the grid is clear.
    expect(onKeyboard(t, t.grid[0].x, t.grid[0].y)).toBe(false);
    expect(t.clips).toHaveLength(CLIPS_PER_LAP);
    for (const k of t.clips) {
      expect(onLane(t, k.x, k.y)).toBe(true);
      for (const s of t.solids) expect(solidDistance(s, k.x, k.y)).toBeGreaterThan(4);
    }
  });

  it("mirroring flips the desk left to right", () => {
    const a = buildTrack(freeVariant("coffee"));
    const b = buildTrack({ ...freeVariant("coffee"), mirror: true });
    expect(b.length).toBeCloseTo(a.length, 6);
    expect(b.mug.x).toBeCloseTo(DESK_W - a.mug.x, 9);
    expect(b.checkpoints[2].x).toBeCloseTo(DESK_W - a.checkpoints[2].x, 6);
    expect(b.checkpoints[2].y).toBeCloseTo(a.checkpoints[2].y, 6);
    expect(b.keyboard.x).toBeCloseTo(DESK_W - a.keyboard.x, 9);
  });

  it("the nearest point on the lane says how far along and which side", () => {
    const t = buildTrack(freeVariant("inbox"));
    const p = poseAt(t, 300);
    const right = { x: p.x - Math.sin(p.angle) * 5, y: p.y + Math.cos(p.angle) * 5 };
    const n = nearest(t, right.x, right.y);
    expect(n.s).toBeCloseTo(300, -1);
    expect(n.off).toBeCloseTo(5, 0);
    expect(n.dist).toBeCloseTo(5, 0);
    expect(onLane(t, right.x, right.y)).toBe(true);
    expect(onLane(t, p.x - Math.sin(p.angle) * 15, p.y + Math.cos(p.angle) * 15)).toBe(false);
  });
});

describe("today's race", () => {
  it("is the same layout, mirror, spill and clips for everyone on a date", () => {
    const a = dailyVariant(dailySeed("2026-11-02", "desk-derby"));
    expect(dailyVariant(dailySeed("2026-11-02", "desk-derby"))).toEqual(a);
    expect(buildTrack(a).clips).toEqual(buildTrack(a).clips);
    // Its own daily, not Tugboat Run's course seed.
    expect(dailySeed("2026-11-02", "desk-derby")).not.toBe(dailySeed("2026-11-02", "tugboat-run"));
  });

  it("over a couple of months, every layout comes up, both ways round, with the coffee in different places", () => {
    const seen = new Set<string>();
    const spills = new Set<number>();
    for (let d = 1; d <= 60; d++) {
      const date = `2026-${d <= 30 ? "11" : "12"}-${String(((d - 1) % 30) + 1).padStart(2, "0")}`;
      const v = dailyVariant(dailySeed(date, "desk-derby"));
      seen.add(`${v.track}${v.mirror}`);
      spills.add(v.puddle);
    }
    expect(seen.size).toBe(8);
    expect(spills.size).toBe(3);
  });

  it("different days scatter the clips differently; Free play's are always the same", () => {
    const a = buildTrack({ ...freeVariant("edge"), clipSeed: 1 });
    const b = buildTrack({ ...freeVariant("edge"), clipSeed: 2 });
    expect(a.clips).not.toEqual(b.clips);
    expect(buildTrack(freeVariant("edge")).clips).toEqual(buildTrack(freeVariant("edge")).clips);
  });
});

describe("the countdown", () => {
  it("counts 3, 2, 1, GO before the cars move", () => {
    const t = buildTrack(freeVariant("inbox"));
    const r = newRace(t);
    expect(r.t).toBe(-COUNTDOWN_S);
    const start = { ...r.cars[0] };
    const events: string[] = [];
    while (r.t < 0) {
      step(r, [{ steer: 1, drift: true }]);
      events.push(...r.events);
      r.events = [];
    }
    expect(events).toEqual(["count", "count", "count", "go"]);
    expect([r.cars[0].x, r.cars[0].y, r.cars[0].speed]).toEqual([start.x, start.y, 0]);
    expect(clockOf(r, r.cars[0])).toBe(0);
    expect([-3, -2.5, -1.5, -0.5, 0, 0.5, 1].map(countdownLabel)).toEqual(["3", "3", "2", "1", "GO", "GO", null]);
  });
});

describe("driving", () => {
  const t = buildTrack(freeVariant("inbox"));
  /** The middle of the long straight along the bottom, pointing along it. */
  const bottom = poseAt(t, 60);

  it("always drives itself forward, up to top speed and no faster", () => {
    // The lane alone: no keyboard, stapler or coffee in the way, no paper clips to pick up.
    const bare = { ...t, keyboard: { ...t.keyboard, x: -999 }, ramp: { ...t.ramp, x: -999 }, puddle: { ...t.puddle, x: -999 } };
    const r = racing(bare);
    const c = r.cars[0];
    put(c, poseAt(t, 20), 0);
    c.taken.fill(true);
    let top = 0;
    run(r, 1, (rr) => ((top = Math.max(top, rr.cars[0].speed)), autopilot(rr, rr.cars[0], { drift: false })));
    expect(c.speed).toBeGreaterThan(V_TOP * 0.8);
    run(r, 1.5, (rr) => ((top = Math.max(top, rr.cars[0].speed)), autopilot(rr, rr.cars[0], { drift: false })));
    expect(top).toBeLessThanOrEqual(V_TOP + 1e-9);
    expect(top).toBeGreaterThan(V_TOP * 0.97);
  });

  it("steers: full lock turns the car at about the turning rate", () => {
    const r = racing(t);
    put(r.cars[0], bottom);
    const before = r.cars[0].angle;
    run(r, 0.5, { steer: 1, drift: false });
    const turned = wrapAngle(r.cars[0].angle - before);
    // Clockwise on screen is to the right; the steering eases in, so a little under full rate.
    expect(turned).toBeGreaterThan(TURN * 0.5 * 0.8);
    expect(turned).toBeLessThan(TURN * 0.5);
  });

  it("drifting turns tighter, slides the back out, and charges a boost released on letting go", () => {
    const grip = racing(t);
    put(grip.cars[0], bottom);
    run(grip, 0.8, { steer: 1, drift: false });
    const slide = racing(t);
    put(slide.cars[0], bottom);
    const events = run(slide, 0.8, { steer: 1, drift: true });
    const c = slide.cars[0];
    expect(events).toContain("drift");
    expect(c.drift).toBe(1);
    expect(wrapAngle(c.angle - bottom.angle)).toBeGreaterThan(wrapAngle(grip.cars[0].angle - bottom.angle) + 0.4);
    // The car points further round than it's travelling: the back is out.
    expect(wrapAngle(c.angle - c.dir)).toBeGreaterThan(0.3);
    expect(wrapAngle(grip.cars[0].angle - grip.cars[0].dir)).toBeLessThan(0.25);
    // Held long enough for level 2…
    run(slide, 0.6, { steer: 1, drift: true });
    expect(c.charge).toBeGreaterThan(CHARGE_LEVELS[1]);
    expect(c.level).toBe(2);
    // …and let go: a level 2 boost, faster than the car can go on its own.
    const out = run(slide, STEP, { steer: 0, drift: false });
    expect(out).toContain("boost");
    expect(c.drift).toBe(0);
    expect(c.boost).toBeCloseTo(BOOST_S[1] - STEP, 6);
    run(slide, 0.5, straight);
    expect(c.speed).toBeGreaterThan(V_TOP * 1.15);
    expect(c.speed).toBeLessThanOrEqual(V_TOP * BOOST_V);
  });

  it("steering into a drift tightens it, out of it widens it, and a short drift gives no boost", () => {
    const turnWith = (steer: number) => {
      const r = racing(t);
      put(r.cars[0], bottom);
      run(r, 0.1, { steer: 1, drift: true });
      run(r, 0.5, { steer, drift: true });
      return wrapAngle(r.cars[0].angle - bottom.angle);
    };
    expect(turnWith(1)).toBeGreaterThan(turnWith(0));
    expect(turnWith(0)).toBeGreaterThan(turnWith(-1));
    const r = racing(t);
    put(r.cars[0], bottom);
    run(r, CHARGE_LEVELS[0] * 0.6, { steer: 1, drift: true });
    expect(run(r, 0.1, straight)).not.toContain("boost");
    expect(r.cars[0].boost).toBe(0);
  });

  it("a drift needs steering and speed to start", () => {
    const r = racing(t);
    put(r.cars[0], bottom);
    run(r, 0.3, { steer: 0, drift: true });
    expect(r.cars[0].drift).toBe(0);
    put(r.cars[0], bottom, 5);
    run(r, 0.05, { steer: 1, drift: true });
    expect(r.cars[0].drift).toBe(0);
    expect([0, 0.49, 0.5, 1.1, 1.8, 9].map(chargeLevel)).toEqual([0, 0, 1, 2, 3, 3]);
  });

  it("off the lane, the bare wood is slow; the keyboard is a little slow", () => {
    const r = racing(t);
    const c = r.cars[0];
    // Across the desk's open middle, away from the lane (the in-tray's paper).
    put(c, { x: 92, y: 100, angle: Math.PI / 2 }, V_TOP);
    expect(onLane(t, c.x, c.y)).toBe(false);
    run(r, 1.1, straight);
    expect(onLane(t, c.x, c.y)).toBe(false);
    expect(c.surface).toBe("wood");
    expect(c.speed).toBeLessThan(V_TOP * WOOD_SPEED * 1.12);
    // On the keys: a bit under top speed.
    put(c, { x: t.keyboard.x - 40, y: t.keyboard.y, angle: 0 }, V_TOP);
    run(r, 0.8, straight);
    expect(onKeyboard(t, c.x, c.y)).toBe(true);
    expect(c.surface).toBe("keys");
    expect(c.speed).toBeLessThan(V_TOP * KEYS_SPEED * 1.02);
    expect(c.speed).toBeGreaterThan(V_TOP * KEYS_SPEED * 0.97);
  });

  it("the coffee is slippery: the car slides much further sideways", () => {
    const slip = (inCoffee: boolean) => {
      const r = racing(t);
      const p = inCoffee ? t.puddle : poseAt(t, 60);
      // Coming in from just before the spill, steering hard.
      put(r.cars[0], { x: p.x - Math.cos(p.angle) * 8, y: p.y - Math.sin(p.angle) * 8, angle: p.angle });
      run(r, 0.25, { steer: 1, drift: false });
      const c = r.cars[0];
      if (inCoffee) expect(inPuddle(t, c.x, c.y)).toBe(true);
      return Math.abs(wrapAngle(c.angle - c.dir));
    };
    expect(slip(true)).toBeGreaterThan(slip(false) * 2);
  });

  it("bumps off the mug: pushed out, slowed, and a drift's charge is lost", () => {
    const r = racing(t);
    const c = r.cars[0];
    // Straight at the mug from the left.
    put(c, { x: t.mug.x - t.mug.r - 12, y: t.mug.y, angle: 0 });
    c.drift = 1;
    c.charge = 0.8;
    const events = run(r, 0.4, straight);
    expect(events).toContain("bump");
    expect(Math.hypot(c.x - t.mug.x, c.y - t.mug.y)).toBeGreaterThanOrEqual(t.mug.r + CAR_R - 1e-6);
    expect(c.charge).toBe(0);
    expect(c.bumps).toBe(1);
  });

  it("the stapler is a jump: in the air there's no steering, and it lands", () => {
    const r = racing(t);
    const c = r.cars[0];
    const p = t.ramp;
    put(c, { x: p.x - Math.cos(p.angle) * 12, y: p.y - Math.sin(p.angle) * 12, angle: p.angle });
    const up = run(r, 0.2, straight);
    expect(up).toContain("jump");
    expect(c.air).toBeGreaterThan(0);
    const angle = c.angle;
    run(r, 0.1, { steer: 1, drift: true });
    expect(c.angle).toBe(angle);
    expect(c.drift).toBe(0);
    const down = run(r, 1, straight);
    expect(down).toContain("land");
    expect(c.air).toBe(0);
    // Driving at the stapler slowly doesn't launch.
    const slow = racing(t);
    put(slow.cars[0], { x: p.x - Math.cos(p.angle) * 6, y: p.y - Math.sin(p.angle) * 6, angle: p.angle }, 4);
    run(slow, 0.02, straight);
    expect(onRamp(t, slow.cars[0].x, slow.cars[0].y) || slow.cars[0].air === 0).toBe(true);
    expect(slow.cars[0].air).toBe(0);
  });

  it("off the edge of the desk: a fall, then back at the last sticky note with time added", () => {
    const r = racing(t);
    const c = r.cars[0];
    c.lastCp = 1;
    c.next = 2;
    // Pointing straight off the bottom edge.
    put(c, { x: 100, y: DESK_H - 3, angle: Math.PI / 2 });
    const events = run(r, 0.1, straight);
    expect(events).toContain("fall");
    expect(c.fall).not.toBeNull();
    expect(onDesk(c.x, c.y)).toBe(false);
    const before = clockOf(r, c);
    const back: CarEvent[] = [];
    for (let k = 0; k < FALL_S / STEP + 2 && !back.includes("respawn"); k++) back.push(...run(r, STEP, straight));
    expect(back).toContain("respawn");
    expect(c.fall).toBeNull();
    expect(c.x).toBeCloseTo(t.checkpoints[1].x, 6);
    expect(c.angle).toBeCloseTo(t.checkpoints[1].angle, 6);
    expect(c.penalty).toBe(PENALTY_S);
    expect(c.falls).toBe(1);
    expect(clockOf(r, c)).toBeGreaterThan(before + PENALTY_S);
  });

  it("paper clips: each once a lap, a little boost each", () => {
    const r = racing(t);
    const c = r.cars[0];
    const k = t.clips[2];
    const at = nearest(t, k.x, k.y);
    const p = poseAt(t, at.s);
    put(c, { x: k.x - Math.cos(p.angle) * 4, y: k.y - Math.sin(p.angle) * 4, angle: p.angle });
    const events = run(r, 0.2, straight);
    expect(events.filter((e) => e === "clip")).toHaveLength(1);
    expect(c.clips).toBe(1);
    expect(c.taken[2]).toBe(true);
    expect(c.boost).toBeGreaterThan(0);
    put(c, { x: k.x - Math.cos(p.angle) * 4, y: k.y - Math.sin(p.angle) * 4, angle: p.angle });
    expect(run(r, 0.2, straight)).not.toContain("clip");
  });
});

describe("laps and the race", () => {
  it.each(ALL)("the test driver gets round $track (mirrored: $mirror): three laps of 18 to 40 s each", (v) => {
    const r = drive(v);
    const c = r.cars[0];
    expect(c.finished).not.toBeNull();
    expect(c.laps).toHaveLength(LAPS);
    for (const l of c.laps) {
      expect(l).toBeGreaterThan(18);
      expect(l).toBeLessThan(40);
    }
    expect(c.laps.reduce((a, b) => a + b, 0)).toBeCloseTo(c.finished!, 6);
    expect(raceMs(c)).toBe(Math.round(c.finished! * 1000));
    expect(r.over).toBe(true);
  });

  it("drifts round the bends on the way, and boosts out of them", () => {
    const r = newRace(buildTrack(freeVariant("esses")));
    const seen: CarEvent[] = [];
    while (!r.over) {
      step(r, [autopilot(r, r.cars[0])]);
      seen.push(...r.cars[0].events);
      r.cars[0].events = [];
    }
    expect(seen.filter((e) => e === "drift").length).toBeGreaterThan(2);
    expect(seen).toContain("boost");
    expect(seen.filter((e) => e === "lap").length).toBe(1);
    expect(seen.filter((e) => e === "final-lap").length).toBe(1);
    expect(seen.filter((e) => e === "finish").length).toBe(1);
  });

  it("the sticky notes count only in order: no lap for cutting across to the line", () => {
    const t = buildTrack(freeVariant("inbox"));
    const r = racing(t);
    const c = r.cars[0];
    // Straight to the line from just before it, without the notes in between.
    const before = poseAt(t, -6);
    put(c, before);
    c.lastCp = 0;
    c.next = 1;
    run(r, 0.5, straight);
    expect(c.lap).toBe(0);
    // Having touched every note, the line counts.
    c.next = 0;
    put(c, poseAt(t, -6));
    expect(run(r, 0.5, straight)).toContain("lap");
    expect(c.lap).toBe(1);
  });

  it("is the same race for the same inputs, at any frame rate", () => {
    const t = buildTrack(dailyVariant(dailySeed("2026-11-02", "desk-derby")));
    // The input changes 30 times a second, so every frame rate here sees the same input each step.
    const script = (k: number): CarInput => ({ steer: Math.sin(k / 10), drift: k % 75 > 50 });
    const play = (fps: number) => {
      const r = newRace(t);
      const carry = { left: 0 };
      for (let k = 0; k < fps * 12; k++) advance(r, [script(Math.floor((k * 30) / fps))], 1 / fps, carry);
      return r;
    };
    const a = play(60);
    const b = play(120);
    const c = play(30);
    for (const o of [b, c]) {
      expect(o.t).toBeCloseTo(a.t, 6);
      expect(o.cars[0].x).toBeCloseTo(a.cars[0].x, 6);
      expect(o.cars[0].y).toBeCloseTo(a.cars[0].y, 6);
    }
    expect(JSON.stringify(play(60).cars)).toBe(JSON.stringify(a.cars));
  });

  it("caps a slow frame instead of jumping the car through things", () => {
    const r = racing(buildTrack(freeVariant("inbox")));
    const t0 = r.t;
    advance(r, [straight], 5);
    expect(r.t - t0).toBeLessThanOrEqual(0.1 + 1e-9);
  });

  it("a race nobody finishes ends at the time limit", () => {
    const r = racing(buildTrack(freeVariant("inbox")));
    r.t = MAX_S - STEP / 2;
    step(r, [straight]);
    expect(r.cars[0].dnf).toBe(true);
    expect(r.over).toBe(true);
    expect(raceMs(r.cars[0])).toBe(0);
  });
});

describe("two players", () => {
  it("start side by side and nudge each other rather than overlapping", () => {
    const t = buildTrack(freeVariant("coffee"));
    const r = racing(t, 2);
    const [a, b] = r.cars;
    expect(Math.hypot(a.x - b.x, a.y - b.y)).toBeGreaterThan(CAR_R * 2);
    expect(onLane(t, a.x, a.y) && onLane(t, b.x, b.y)).toBe(true);
    // Steer them into each other.
    run(r, 0.6, (rr) => (rr.t >= 0 ? { steer: 0.6, drift: false } : straight));
    expect(Math.hypot(a.x - b.x, a.y - b.y)).toBeGreaterThan(CAR_R * 2 - 0.5);
  });

  it("once one finishes the other has a while, then is out; the faster time wins", () => {
    const t = buildTrack(freeVariant("inbox"));
    const r = racing(t, 2);
    const [a, b] = r.cars;
    a.lap = LAPS - 1;
    a.next = 0;
    put(a, poseAt(t, -6));
    run(r, 0.3, straight);
    expect(a.finished).not.toBeNull();
    expect(r.deadline).toBeCloseTo(r.t - (r.t - (r.deadline! - FINISH_GRACE_S)) + FINISH_GRACE_S, 6);
    expect(r.over).toBe(false);
    // Player 2 dawdles past the deadline.
    while (!r.over) step(r, [straight, { steer: 1, drift: false }]);
    expect(b.dnf).toBe(true);
    expect(derbyWinner([raceMs(a), raceMs(b)])).toBe(1);
    expect(places(r)).toEqual([1, 2]);
  });

  it("the winner: the lower time; no time loses; nobody finishing is no winner", () => {
    expect(derbyWinner([90_000, 91_000])).toBe(1);
    expect(derbyWinner([92_000, 91_000])).toBe(2);
    expect(derbyWinner([0, 91_000])).toBe(2);
    expect(derbyWinner([88_000, 0])).toBe(1);
    expect(derbyWinner([0, 0])).toBeNull();
    expect(derbyWinner([90_000, 90_000])).toBeNull();
  });

  it("who's ahead: further round the race is first", () => {
    const t = buildTrack(freeVariant("edge"));
    const r = racing(t, 2);
    put(r.cars[1], poseAt(t, 200));
    put(r.cars[0], poseAt(t, 120));
    expect(places(r)).toEqual([2, 1]);
    r.cars[0].lap = 1;
    expect(places(r)).toEqual([1, 2]);
  });
});

describe("ghosts", () => {
  const v = freeVariant("edge");
  const r = drive(v, false);

  it("records the first car every tenth of a second, with its time and laps", () => {
    const g = ghostOf(r)!;
    expect(g.key).toBe(ghostKey(v));
    expect(g.ms).toBe(raceMs(r.cars[0]));
    expect(g.laps).toHaveLength(LAPS);
    expect(g.laps.reduce((a, b) => a + b, 0)).toBeGreaterThan(g.ms - 5);
    expect(g.poses.length / 3).toBeCloseTo(r.cars[0].finished! * GHOST_HZ, -1);
  });

  it("replays where the car was", () => {
    const t = buildTrack(v);
    const replay = newRace(t);
    const g = ghostOf(r)!;
    while (replay.t < 20) {
      step(replay, [autopilot(replay, replay.cars[0], { drift: false })]);
      if (replay.t > 0 && Math.round(replay.t * 120) % 60 === 0) {
        const p = ghostPose(g, replay.t)!;
        expect(Math.hypot(p.x - replay.cars[0].x, p.y - replay.cars[0].y)).toBeLessThan(1);
      }
    }
    expect(ghostPose(g, -1)).toBeNull();
    expect(ghostPose(g, 9999)).toBeNull();
  });

  it("survives being saved, and anything damaged reads as no ghost", () => {
    const g = ghostOf(r)!;
    expect(parseGhost(JSON.parse(JSON.stringify(g)))).toEqual(g);
    for (const bad of [null, 3, "x", {}, { ...g, ms: -1 }, { ...g, poses: [1, 2] }, { ...g, poses: ["a", "b", "c"] }, { ...g, key: 7 }]) {
      expect(parseGhost(bad)).toBeNull();
    }
    expect(parseGhost({ ...g, laps: "nope" })?.laps).toEqual([]);
  });

  it("keeps the faster run on the same track; a run with no time never replaces one", () => {
    const g = ghostOf(r)!;
    const faster = { ...g, ms: g.ms - 1000 };
    expect(betterGhost(g, faster)).toBe(faster);
    expect(betterGhost(faster, g)).toBe(faster);
    expect(betterGhost(g, null)).toBe(g);
    expect(betterGhost(null, g)).toBe(g);
    // Another variant of the track (a different day's): the new run.
    const other = { ...g, key: "edge~m/1/5", ms: g.ms + 9000 };
    expect(betterGhost(g, other)).toBe(other);
  });

  it("only finished races leave a ghost", () => {
    const dnf = racing(buildTrack(v));
    dnf.cars[0].dnf = true;
    expect(ghostOf(dnf)).toBeNull();
  });
});

describe("controls and words", () => {
  it("two players on one keyboard: arrows and ↑ or right Shift; A D and W or left Shift", () => {
    expect(duoKey("ArrowLeft")).toEqual({ player: 1, action: "left" });
    expect(duoKey("ArrowRight")).toEqual({ player: 1, action: "right" });
    expect(duoKey("ArrowUp")).toEqual({ player: 1, action: "drift" });
    expect(duoKey("Shift", "ShiftRight")).toEqual({ player: 1, action: "drift" });
    expect(duoKey("Shift", "ShiftLeft")).toEqual({ player: 2, action: "drift" });
    expect(duoKey("A")).toEqual({ player: 2, action: "left" });
    expect(duoKey("d")).toEqual({ player: 2, action: "right" });
    expect(duoKey("w")).toEqual({ player: 2, action: "drift" });
    expect(duoKey(" ")).toBeNull();
    expect(duoKey("Enter")).toBeNull();
  });

  it("keys win over the phone's slider; either's Drift drifts", () => {
    const pad = { connected: true, steer: 0.4, drift: false };
    expect(inputFrom({ left: false, right: false, drift: false, pad })).toEqual({ steer: 0.4, drift: false });
    expect(inputFrom({ left: true, right: false, drift: false, pad })).toEqual({ steer: -1, drift: false });
    expect(inputFrom({ left: true, right: true, drift: false, pad })).toEqual({ steer: 0.4, drift: false });
    expect(inputFrom({ left: false, right: false, drift: false, pad: { ...pad, drift: true } })).toEqual({ steer: 0.4, drift: true });
    expect(inputFrom({ left: false, right: false, drift: true, pad: null })).toEqual({ steer: 0, drift: true });
    // A phone that's gone steers nothing; nonsense is straight ahead.
    expect(inputFrom({ left: false, right: false, drift: false, pad: { connected: false, steer: 1, drift: true } })).toEqual({ steer: 0, drift: false });
    expect(inputFrom({ left: false, right: false, drift: false, pad: { connected: true, steer: NaN, drift: false } }).steer).toBe(0);
  });

  it("times read as minutes, seconds and hundredths; splits with a sign", () => {
    expect(formatTime(92_456)).toBe("1:32.46");
    expect(formatTime(5_000)).toBe("0:05.00");
    expect(formatTime(600_000)).toBe("10:00.00");
    expect(formatTime(0)).toBe("–:––.––");
    expect(formatTime(NaN)).toBe("–:––.––");
    expect(formatSplit(-420)).toBe("−0.42");
    expect(formatSplit(1300)).toBe("+1.30");
    expect(formatSplit(0)).toBe("+0.00");
  });

  it("a race time is whole milliseconds, never 0 for a finish", () => {
    expect(raceMs({ finished: 91.2345, dnf: false })).toBe(91235);
    expect(raceMs({ finished: null, dnf: false })).toBe(0);
    expect(raceMs({ finished: 80, dnf: true })).toBe(0);
    expect(raceMs({ finished: 0.0001, dnf: false })).toBe(1);
  });
});
