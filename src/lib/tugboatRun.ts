// Tugboat Run: the game's rules, pure and deterministic. A fixed playfield, a seeded random
// number generator kept in the run itself, and plain objects, so collisions, spawn pacing,
// scoring, the speed curve and the boat's inertia are all tested without a canvas.
// TugboatRun.vue calls `step` from requestAnimationFrame and draws what it returns.
//
// The harbor scrolls down the screen; the tugboat sits near the bottom and steers left and right
// with a little inertia. Buoys and logs can be hopped over (Space, or Boost on the phone); other
// boats are too tall to hop. Coins add points; a life ring gives back a life. Three lives, a short
// grace period after each hit, and the water speeds up, so a run lasts a minute or three.

/** The playfield, in field units. The canvas scales it to fit and draws the harbor around it. */
export const FIELD_W = 100;
export const FIELD_H = 160;
/** The boat's centre line and size. */
export const BOAT_Y = 134;
export const BOAT_W = 8;
export const BOAT_H = 14;
/** The boat's centre never goes closer than this to either bank. */
export const MARGIN = 6;
export const LIVES = 3;
/** After a hit the boat can't be hit again for this long. */
export const GRACE_S = 1.6;
/** A hop lasts this long, then can't be used again for the cooldown. */
export const HOP_S = 0.6;
export const HOP_COOLDOWN_S = 1.1;
/** Steering: top sideways speed, and how quickly the boat answers the helm (per second). */
export const MAX_VX = 75;
export const RESPONSE = 7;
/** Water speed (field units a second): starts gentle, approaches the top speed. */
export const SPEED_START = 36;
export const SPEED_MAX = 100;
export const SPEED_TAU = 75;
/** Rows of obstacles start just above the top edge. */
export const SPAWN_Y = -12;
/** Every row leaves at least this much open water somewhere. */
export const MIN_GAP = 22;
/** Points per field unit travelled, and for pickups. */
const DISTANCE_PER_POINT = 6;
const COIN_POINTS = 10;
const RING_POINTS = 50;

export type Kind = "buoy" | "log" | "boat" | "coin" | "ring";

export interface Thing {
  id: number;
  kind: Kind;
  /** Centre. */
  x: number;
  y: number;
  w: number;
  h: number;
  /** Its own drift, on top of the water carrying it down. */
  vx: number;
  vy: number;
  /** An obstacle the boat already struck (drawn sinking, never hits again). */
  hit: boolean;
}

export type GameEvent = "hit" | "coin" | "ring" | "hop" | "over";

export interface Run {
  /** The random generator's state (mulberry32). */
  seed: number;
  /** Seconds played. */
  t: number;
  distance: number;
  speed: number;
  x: number;
  vx: number;
  /** Seconds left in the air. */
  hop: number;
  /** Seconds until the next hop. */
  cooldown: number;
  /** Boost was held at the last step (a hop starts on the press, not while held). */
  boostHeld: boolean;
  lives: number;
  /** Seconds of grace left after a hit. */
  grace: number;
  coins: number;
  /** Points from pickups. */
  bonus: number;
  hits: number;
  things: Thing[];
  /** The distance at which the next row appears. */
  nextSpawn: number;
  nextId: number;
  over: boolean;
  /** What happened since the caller last cleared it (sounds, shake, the phone's buzz). */
  events: GameEvent[];
}

export interface Input {
  /** -1 (full left) to 1 (full right). */
  steer: number;
  boost: boolean;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** The next random number in [0, 1), advancing the run's own generator (mulberry32). */
export function random(run: { seed: number }): number {
  run.seed = (run.seed + 0x6d2b79f5) | 0;
  let t = run.seed;
  t = Math.imul(t ^ (t >>> 15), t | 1);
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}

export function newRun(seed: number): Run {
  return {
    seed: seed | 0,
    t: 0,
    distance: 0,
    speed: SPEED_START,
    x: FIELD_W / 2,
    vx: 0,
    hop: 0,
    cooldown: 0,
    boostHeld: false,
    lives: LIVES,
    grace: 0,
    coins: 0,
    bonus: 0,
    hits: 0,
    things: [],
    // A moment of open water before the first row.
    nextSpawn: 90,
    nextId: 1,
    over: false,
    events: [],
  };
}

/** How far into the difficulty curve a run is, 0 at the start towards 1. */
export function progress(t: number): number {
  return 1 - Math.exp(-Math.max(0, t) / 90);
}

/** Water speed after `t` seconds: rises smoothly from the start speed towards the top speed. */
export function speedAt(t: number): number {
  return SPEED_START + (SPEED_MAX - SPEED_START) * (1 - Math.exp(-Math.max(0, t) / SPEED_TAU));
}

/** Distance between rows: generous at first, tighter as the run goes on. */
export function spawnGap(t: number): number {
  return 64 - (64 - 30) * progress(t);
}

/**
 * The helm with inertia: sideways speed eases towards what the steering asks for, never past it
 * (an exact first-order lag, so it behaves the same at any frame rate).
 */
export function steerVelocity(vx: number, steer: number, dt: number): number {
  const target = clamp(Number.isFinite(steer) ? steer : 0, -1, 1) * MAX_VX;
  return vx + (target - vx) * (1 - Math.exp(-RESPONSE * dt));
}

/** Whether something round or square at (thing) touches the boat at `x`. */
export function touches(x: number, thing: Pick<Thing, "kind" | "x" | "y" | "w" | "h">): boolean {
  // The hull's hitbox is a little inside its drawing, so near misses feel fair.
  const hw = BOAT_W * 0.4;
  const hh = BOAT_H * 0.42;
  if (thing.kind === "log" || thing.kind === "boat") {
    return Math.abs(thing.x - x) < hw + thing.w / 2 && Math.abs(thing.y - BOAT_Y) < hh + thing.h / 2;
  }
  const r = thing.w / 2;
  const nx = clamp(thing.x, x - hw, x + hw);
  const ny = clamp(thing.y, BOAT_Y - hh, BOAT_Y + hh);
  return (thing.x - nx) ** 2 + (thing.y - ny) ** 2 < r * r;
}

export function isObstacle(kind: Kind): boolean {
  return kind === "buoy" || kind === "log" || kind === "boat";
}

/** The widest stretch of open water across a row's obstacles (pickups don't block). */
export function widestGap(row: Array<Pick<Thing, "kind" | "x" | "w">>): number {
  const spans = row
    .filter((t) => isObstacle(t.kind))
    .map((t) => [t.x - t.w / 2, t.x + t.w / 2] as const)
    .sort((a, b) => a[0] - b[0]);
  let widest = 0;
  let edge = 0;
  for (const [a, b] of spans) {
    widest = Math.max(widest, a - edge);
    edge = Math.max(edge, b);
  }
  return Math.max(widest, FIELD_W - edge);
}

function thing(run: Run, kind: Kind, x: number, y = SPAWN_Y): Thing {
  const size: Record<Kind, [number, number]> = {
    buoy: [6, 6],
    log: [18 + random(run) * 12, 4.5],
    boat: [8, 15],
    coin: [4.4, 4.4],
    ring: [6.5, 6.5],
  };
  const [w, h] = size[kind];
  const lo = MARGIN + w / 2 - 4;
  const hi = FIELD_W - MARGIN - w / 2 + 4;
  return {
    id: run.nextId++,
    kind,
    x: clamp(x, lo, hi),
    y,
    w,
    h,
    // Other boats wander across the harbor and come at you a little faster than the water.
    vx: kind === "boat" ? (random(run) < 0.5 ? -1 : 1) * (4 + random(run) * 6) : 0,
    vy: kind === "boat" ? 8 : 0,
    hit: false,
  };
}

const anywhere = (run: Run) => MARGIN + random(run) * (FIELD_W - 2 * MARGIN);

/** One row of obstacles and pickups at the top of the field. Every row leaves `MIN_GAP` open. */
export function spawnRow(run: Run): Thing[] {
  const p = progress(run.t);
  const row: Thing[] = [];
  if (run.lives < LIVES && random(run) < 0.05) {
    row.push(thing(run, "ring", anywhere(run)));
  } else {
    const roll = random(run);
    if (roll < 0.2) {
      // A trail of coins.
      const x = anywhere(run);
      for (let i = 0; i < 4; i++) row.push(thing(run, "coin", x, SPAWN_Y - i * 7));
    } else if (roll < 0.45) {
      row.push(thing(run, "buoy", anywhere(run)));
    } else if (roll < 0.62) {
      const a = anywhere(run);
      const b = a < FIELD_W / 2 ? a + 30 + random(run) * 20 : a - 30 - random(run) * 20;
      row.push(thing(run, "buoy", a), thing(run, "buoy", b));
    } else if (roll < 0.82) {
      row.push(thing(run, "log", anywhere(run)));
    } else {
      row.push(thing(run, "boat", anywhere(run)));
    }
    // Later on, a second obstacle in the same row, placed so open water remains.
    if (random(run) < p * 0.6) {
      for (let tries = 0; tries < 5; tries++) {
        const extra = thing(run, random(run) < 0.7 ? "buoy" : "log", anywhere(run), SPAWN_Y - 4);
        if (widestGap([...row, extra]) >= MIN_GAP) {
          row.push(extra);
          break;
        }
        run.nextId--; // not used
      }
    }
    // Now and then a coin as a reward for threading the gap.
    if (random(run) < 0.3) row.push(thing(run, "coin", anywhere(run), SPAWN_Y - 10));
  }
  run.things.push(...row);
  return row;
}

export function score(run: Pick<Run, "distance" | "bonus">): number {
  return Math.floor(run.distance / DISTANCE_PER_POINT) + run.bonus;
}

/** Advance the run by `dt` seconds (keep steps small: the caller splits a frame into ≤ 1/60 s). */
export function step(run: Run, input: Input, dt: number): Run {
  if (run.over || dt <= 0) return run;
  run.t += dt;
  run.speed = speedAt(run.t);
  const dy = run.speed * dt;
  run.distance += dy;

  // The helm, with inertia; the banks stop the boat.
  run.vx = steerVelocity(run.vx, input.steer, dt);
  run.x += run.vx * dt;
  if (run.x < MARGIN) {
    run.x = MARGIN;
    run.vx = Math.max(0, run.vx);
  } else if (run.x > FIELD_W - MARGIN) {
    run.x = FIELD_W - MARGIN;
    run.vx = Math.min(0, run.vx);
  }

  // Hop on the press of Boost, when it's ready.
  const pressed = input.boost && !run.boostHeld;
  run.boostHeld = input.boost;
  run.hop = Math.max(0, run.hop - dt);
  run.cooldown = Math.max(0, run.cooldown - dt);
  run.grace = Math.max(0, run.grace - dt);
  if (pressed && run.hop === 0 && run.cooldown === 0) {
    run.hop = HOP_S;
    run.cooldown = HOP_S + HOP_COOLDOWN_S;
    run.events.push("hop");
  }

  for (const th of run.things) {
    th.y += dy + th.vy * dt;
    th.x += th.vx * dt;
    if (th.kind === "boat" && (th.x < MARGIN || th.x > FIELD_W - MARGIN)) {
      th.vx = -th.vx;
      th.x = clamp(th.x, MARGIN, FIELD_W - MARGIN);
    }
  }

  while (run.distance >= run.nextSpawn) {
    spawnRow(run);
    run.nextSpawn += spawnGap(run.t) * (0.85 + 0.3 * random(run));
  }

  const taken = new Set<number>();
  for (const th of run.things) {
    if (th.hit || !touches(run.x, th)) continue;
    if (th.kind === "coin") {
      taken.add(th.id);
      run.coins++;
      run.bonus += COIN_POINTS;
      run.events.push("coin");
    } else if (th.kind === "ring") {
      taken.add(th.id);
      if (run.lives < LIVES) run.lives++;
      else run.bonus += RING_POINTS;
      run.events.push("ring");
    } else {
      // Hopping clears buoys and logs; boats are too tall. Grace forgives everything.
      if (run.grace > 0 || (run.hop > 0 && th.kind !== "boat")) continue;
      th.hit = true;
      run.lives--;
      run.hits++;
      run.grace = GRACE_S;
      run.vx *= -0.4;
      run.events.push("hit");
      if (run.lives <= 0) {
        run.over = true;
        run.events.push("over");
        break;
      }
    }
  }
  run.things = run.things.filter((th) => !taken.has(th.id) && th.y - th.h < FIELD_H + 10);
  return run;
}

/** Run `dt` seconds as steps of at most 1/60 s (a slow frame is capped so nothing tunnels). */
export function advance(run: Run, input: Input, dt: number): Run {
  let left = Math.min(Math.max(0, dt), 0.1);
  while (left > 1e-9 && !run.over) {
    const h = Math.min(left, 1 / 60);
    step(run, input, h);
    left -= h;
  }
  return run;
}

// --- Controls ---

export interface Controls {
  keyLeft: boolean;
  keyRight: boolean;
  keyBoost: boolean;
  /** Where the mouse is held, in field units; null when not dragging. */
  pointerX: number | null;
  pointerBoost: boolean;
  /** The phone controller, when one is connected. */
  pad: { connected: boolean; steer: number; boost: boolean } | null;
}

/**
 * One input from every control: the keyboard wins whenever a key is held (so it takes over the
 * instant the phone drops or misbehaves), then a mouse drag (steer towards the pointer), then the
 * phone. Boost from any of them hops.
 */
export function mergeInput(c: Controls, boatX: number): Input {
  const keys = (c.keyRight ? 1 : 0) - (c.keyLeft ? 1 : 0);
  const pad = c.pad?.connected ? c.pad : null;
  let steer = 0;
  if (keys !== 0) steer = keys;
  else if (c.pointerX !== null) steer = clamp((c.pointerX - boatX) / 10, -1, 1);
  else if (pad) steer = clamp(Number.isFinite(pad.steer) ? pad.steer : 0, -1, 1);
  return { steer, boost: c.keyBoost || c.pointerBoost || (pad?.boost ?? false) };
}

// --- Layout (the field scaled into the panel) ---

export interface Layout {
  /** CSS pixels per field unit. */
  scale: number;
  /** Where the field's top-left corner sits, in CSS pixels. */
  left: number;
  top: number;
}

/** Fit the whole field in the panel, centred; the harbor is drawn in the space either side. */
export function layout(width: number, height: number): Layout {
  const scale = Math.max(0.01, Math.min(height / FIELD_H, width / FIELD_W));
  return { scale, left: (width - FIELD_W * scale) / 2, top: (height - FIELD_H * scale) / 2 };
}

/** A pointer's x (CSS pixels in the panel) in field units. */
export function toFieldX(px: number, l: Layout): number {
  return (px - l.left) / l.scale;
}
