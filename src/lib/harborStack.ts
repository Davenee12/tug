// Harbor Stack: the game's rules, pure and deterministic, like Tugboat Run's. A crane swings a
// cargo container back and forth above a boat; you drop it onto the stack. Whatever hangs over the
// edge is sliced off and falls into the harbor, so the next container is narrower. A drop that
// lands within a whisker of the one below is perfect: no width lost, a combo, and every few in a
// row a little width back. A drop that misses the stack costs one of three lives.
//
// The boat sways on the water, gently at first and more as the stack grows (a taller stack also
// catches more of the wind), the crane speeds up, and later on gusts lean on the stack (with a
// moment's warning). Everything that varies comes from the seed: container colours, which side the
// crane starts from, the swell's rhythm and when the gusts come. So the daily stack is the same for
// everyone, and a stack-off is fair (both stacks share a seed). HarborStack.vue calls `advance`
// from requestAnimationFrame and draws what it returns.
//
// Coordinates are field units on Tugboat Run's 100 × 160 field (so its layouts are reused). Heights
// are measured up from the boat's deck; x is across the screen. The stack leans as a shear: a point
// `h` above the deck sits `h × lean` further across, which for these small angles looks like a tilt
// and keeps the arithmetic exact.

import { FIELD_H, FIELD_W, random, rowRandom, type Handling } from "./tugboatRun";
import type { Conditions } from "./tugboatDaily";

export { FIELD_H, FIELD_W };

/** A container's height. */
export const BOX_H = 7;
/** The first containers' width, and the most a perfect streak can grow one back to. */
export const START_W = 36;
/** An overlap thinner than this doesn't hold: it's a miss. */
export const MIN_W = 1;
/** Where the deck sits on screen with the camera at rest. */
export const DECK_Y = 126;
/** Where the camera keeps the top of the stack once it's grown. */
export const TOP_Y = 76;
/** The water line, a little below the deck. */
export const WATER_Y = DECK_Y + 9;
export const BOAT_W = 66;
export const LIVES = 3;
/** A drop this close to the container below (either way) is perfect. */
export const PERFECT = 1.6;
/** Every this many perfect drops in a row, the container grows back by `RESTORE_W`. */
export const RESTORE_EVERY = 4;
export const RESTORE_W = 4;
/** The hanging container's underside is this far above the top of the stack. */
export const CRANE_GAP = 38;
/** How fast a dropped container falls (field units a second, squared): about 0.4 s to land. */
export const GRAVITY = 520;
/** A breath between one container landing and the next swinging in. */
export const NEXT_S = 0.3;
/** The crane lets go by itself after this long, so nobody can stall (a stack-off waits for no one). */
export const HOLD_S = 12;
/** The crane swings the container's centre this far either side of the middle: far enough that a
 * full-width container can miss the stack entirely. */
export const SWING = 38;
/** Crane speed (field units a second): gentle at first, quicker with every container. */
export const SPEED_START = 30;
export const SPEED_MAX = 92;
export const SPEED_LEVELS = 28;
/** A nudge from the phone's slider (or ← →) speeds the crane up or slows it by up to this share. */
export const NUDGE = 0.35;
/** Containers stacked when a celebration comes. */
export const MILESTONES = [10, 25, 50, 100] as const;
/** Gusts start once the stack is this tall. */
export const GUST_FROM = 12;
/** How long a gust leans on the stack, and the warning before it. */
export const GUST_S = 1.6;
export const GUST_WARN_S = 1;
/** Gusts come about this often (a slot each, at a seeded moment within it). */
const GUST_EVERY = 9;
const GUST_FIRST = 6;

export type Kind = "coral" | "teal" | "amber" | "slate" | "green" | "gold";
const COLOURS: Kind[] = ["coral", "teal", "amber", "slate", "green"];

/** One container in the stack. `x` is its centre, measured from the middle of the deck. */
export interface Block {
  x: number;
  w: number;
  kind: Kind;
}

/** A container (or the sliced-off end of one) falling into the harbor, in screen x and height. */
export interface Piece {
  x: number;
  h: number;
  w: number;
  vx: number;
  vh: number;
  angle: number;
  spin: number;
  kind: Kind;
  /** When it reached the water (seconds into the game), for the splash; null while in the air. */
  splashed: number | null;
}

export type StackEvent = "drop" | "perfect" | "slice" | "miss" | "grow" | "milestone" | "splash" | "over";

/** What today's weather does to the stack, from Tugboat Run's handling (calm everywhere else). */
export interface StackWeather {
  /** A steady lean from the wind (radians, positive leans right), once the stack is tall. */
  lean: number;
  /** How much the wind adds to the sway and the gusts (0 calm, 1 a gale). */
  wind: number;
  /** Rain's chop: a quick extra rock (radians). */
  chop: number;
  /** The crane's speed, as a share of the usual (snow: slower). */
  crane: number;
}

export const CALM_STACK: StackWeather = { lean: 0, wind: 0, chop: 0, crane: 1 };

/** Tugboat Run's daily water (`handlingFor`) as weather for the stack: the same wind, rain and snow. */
export function stackWeather(h: Handling): StackWeather {
  return {
    lean: Math.round((h.drift / 9) * 0.012 * 1e5) / 1e5 || 0,
    wind: Math.min(1, Math.abs(h.drift) / 9),
    chop: Math.round((h.waves / 11) * 0.005 * 1e5) / 1e5,
    crane: h.speedScale,
  };
}

export interface Crane {
  /** How far the crane has travelled (its position comes from this, back and forth). */
  s: number;
  w: number;
  kind: Kind;
  /** Seconds it's been holding this container. */
  held: number;
}

export interface Falling {
  /** Centre across the screen, the underside's height, and its speed (negative is down). */
  x: number;
  h: number;
  vh: number;
  w: number;
  kind: Kind;
}

export interface Stack {
  seed: number;
  weather: StackWeather;
  /** Seconds played. */
  t: number;
  /** The containers, from the deck up. The first is aboard before the game starts. */
  blocks: Block[];
  lives: number;
  /** Perfect drops in a row. */
  combo: number;
  bestCombo: number;
  perfects: number;
  misses: number;
  crane: Crane | null;
  falling: Falling | null;
  /** Seconds until the next container swings in (while none is hanging or falling). */
  next: number;
  pieces: Piece[];
  /** How far the view has risen with the stack (field units). */
  cam: number;
  /** The last milestone reached, and when, for the celebration. */
  cheer: { score: number; t: number } | null;
  /** The last landing, for a moment's highlight: perfect or not, and when. */
  landed: { perfect: boolean; grew: boolean; t: number } | null;
  over: boolean;
  /** What happened since the caller last cleared it (the HUD, a shake, the phone's buzz). */
  events: StackEvent[];
}

export interface StackInput {
  /** A drop was pressed since the last step (one press, one container). */
  drop: boolean;
  /** -1 to 1: slow the crane down or speed it up (the phone's slider, or ← →). */
  nudge: number;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const TAU = 2 * Math.PI;

/** The colour of the container for `level` (the first stacked is 1): never the same as the one
 * below, gold at a milestone. A pure function of the seed, the level and the colour below. */
export function kindFor(seed: number, level: number, below: Kind): Kind {
  if ((MILESTONES as readonly number[]).includes(level)) return "gold";
  const rng = rowRandom(seed ^ 0x51ac, level);
  const from = Math.max(0, COLOURS.indexOf(below));
  return COLOURS[(from + 1 + Math.floor(random(rng) * (COLOURS.length - 1))) % COLOURS.length];
}

/** Which side the crane starts from for `level`: 0 (far left) or a half swing (far right). */
export function craneStart(seed: number, level: number): number {
  return random(rowRandom(seed ^ 0xc4a7e, level)) < 0.5 ? 0 : 2 * SWING;
}

/** The crane's speed for the container at `level`: rises smoothly towards the top speed. */
export function craneSpeed(level: number): number {
  return SPEED_START + (SPEED_MAX - SPEED_START) * (1 - Math.exp(-Math.max(0, level) / SPEED_LEVELS));
}

/** Where the hanging container's centre is, across the screen, after the crane has travelled `s`. */
export function craneX(s: number): number {
  const period = 4 * SWING;
  const u = ((s % period) + period) % period;
  return FIELD_W / 2 + (u < 2 * SWING ? -SWING + u : 3 * SWING - u);
}

/** The swell's phases for a seed (the same seed rocks the same way). */
function phases(seed: number): [number, number, number, number] {
  const rng = rowRandom(seed ^ 0x5ea, 0);
  return [random(rng) * TAU, random(rng) * TAU, random(rng) * TAU, random(rng) * TAU];
}

/** How far the stack rocks either way (radians) at `level`: barely at first, growing with height,
 * and more in the wind. */
export function swayAmp(level: number, w: StackWeather = CALM_STACK): number {
  const grow = 1 - Math.exp(-Math.max(0, level) / 26);
  return (0.005 + 0.02 * grow) * (1 + 0.8 * w.wind * Math.min(1, level / 30));
}

/** The gust in the slot `k` for a seed: when it starts and which way it blows. */
function gustSlot(seed: number, k: number): { at: number; dir: 1 | -1 } {
  const rng = rowRandom(seed ^ 0x6057, k);
  return { at: GUST_FIRST + k * GUST_EVERY + random(rng) * 3, dir: random(rng) < 0.5 ? -1 : 1 };
}

/** A gust's lean (radians) at `t`: none until the stack is `GUST_FROM` tall, then stronger as it
 * grows and in the wind. A soft push and release over `GUST_S`. */
export function gustAt(seed: number, t: number, level: number, w: StackWeather = CALM_STACK): number {
  if (level < GUST_FROM) return 0;
  const strength = 0.011 * (1 + w.wind) * Math.min(1, (level - GUST_FROM + 6) / 24);
  const k = Math.floor((t - GUST_FIRST) / GUST_EVERY);
  for (const slot of [k - 1, k]) {
    if (slot < 0) continue;
    const g = gustSlot(seed, slot);
    const u = (t - g.at) / GUST_S;
    if (u >= 0 && u <= 1) return g.dir * strength * Math.sin(Math.PI * u) ** 2;
  }
  return 0;
}

/** A gust is about to come (the windsock lifts): which way, or 0. */
export function gustWarning(seed: number, t: number, level: number): -1 | 0 | 1 {
  if (level < GUST_FROM) return 0;
  const k = Math.floor((t - GUST_FIRST) / GUST_EVERY);
  for (const slot of [k, k + 1]) {
    if (slot < 0) continue;
    const g = gustSlot(seed, slot);
    if (t >= g.at - GUST_WARN_S && t < g.at + GUST_S) return g.dir;
  }
  return 0;
}

/**
 * The stack's lean at `t` (radians, positive leans right): the swell (two waves, so it never feels
 * regular), rain's chop, the wind's steady lean (more on a taller stack) and any gust. A pure
 * function of the seed, the clock, the height and the weather, so the same seed sways the same.
 */
export function swayAt(seed: number, t: number, level: number, w: StackWeather = CALM_STACK): number {
  const [a, b, c] = phases(seed);
  const swell = swayAmp(level, w) * (0.72 * Math.sin((TAU * t) / 3.3 + a) + 0.28 * Math.sin((TAU * t) / 1.9 + b));
  const chop = w.chop * Math.sin((TAU * t) / 0.62 + c);
  const steady = w.lean * Math.min(1, level / 20);
  return swell + chop + steady + gustAt(seed, t, level, w);
}

/** The boat drifting a little from side to side on the swell (field units). */
export function driftAt(seed: number, t: number): number {
  return 1.2 * Math.sin((TAU * t) / 4.4 + phases(seed)[3]);
}

/** Where a point `h` above the deck, `x` from its middle, is across the screen. */
export function worldX(x: number, h: number, lean: number, drift: number): number {
  return FIELD_W / 2 + drift + x + h * lean;
}

/** The game is over and the last container has reached the water: time for the results. */
export function settled(s: Pick<Stack, "over" | "pieces">): boolean {
  return s.over && s.pieces.every((p) => p.splashed !== null);
}

/** How many containers are stacked (the first, aboard from the start, doesn't count). */
export function stackScore(s: Pick<Stack, "blocks">): number {
  return Math.max(0, s.blocks.length - 1);
}

/** The height of the top of the stack above the deck. */
export function topHeight(s: Pick<Stack, "blocks">): number {
  return s.blocks.length * BOX_H;
}

/** How far the view should have risen, so the top of the stack stays at `TOP_Y`. */
export function cameraFor(height: number): number {
  return Math.max(0, height - (DECK_Y - TOP_Y));
}

export type Landing =
  | { kind: "perfect"; block: Block }
  | { kind: "slice"; block: Block; cut: { x: number; w: number } }
  | { kind: "miss" };

/**
 * A container of width `w` landing with its centre at `x` (measured like the stack's, from the
 * middle of the deck) on top of `top`. Within `PERFECT` it's perfect and sits square on the one
 * below; otherwise what overhangs is cut off (`cut`, in the same measure); an overlap under `MIN_W`
 * doesn't hold.
 */
export function resolveDrop(top: Block, x: number, w: number): Landing {
  const dx = x - top.x;
  if (Math.abs(dx) <= PERFECT) return { kind: "perfect", block: { x: top.x, w, kind: top.kind } };
  const lo = Math.max(x - w / 2, top.x - top.w / 2);
  const hi = Math.min(x + w / 2, top.x + top.w / 2);
  const overlap = hi - lo;
  if (overlap < MIN_W) return { kind: "miss" };
  const cutW = w - overlap;
  const cutX = dx > 0 ? hi + cutW / 2 : lo - cutW / 2;
  return { kind: "slice", block: { x: (lo + hi) / 2, w: overlap, kind: top.kind }, cut: { x: cutX, w: cutW } };
}

/** The container width after a perfect drop: back up by `RESTORE_W` every `RESTORE_EVERY` in a row. */
export function restoredWidth(w: number, combo: number): number {
  if (combo <= 0 || combo % RESTORE_EVERY !== 0) return w;
  return Math.min(START_W, Math.max(w, w + RESTORE_W));
}

export function newStack(seed: number, weather: StackWeather = CALM_STACK): Stack {
  const s: Stack = {
    seed: seed | 0,
    weather,
    t: 0,
    blocks: [{ x: 0, w: START_W, kind: "slate" }],
    lives: LIVES,
    combo: 0,
    bestCombo: 0,
    perfects: 0,
    misses: 0,
    crane: null,
    falling: null,
    next: 0,
    pieces: [],
    cam: 0,
    cheer: null,
    landed: null,
    over: false,
    events: [],
  };
  swingIn(s);
  return s;
}

/** The next container swings in on the crane: the width of the top of the stack, its own colour. */
function swingIn(s: Stack) {
  const level = s.blocks.length;
  const top = s.blocks[s.blocks.length - 1];
  s.crane = { s: craneStart(s.seed, level), w: top.w, kind: kindFor(s.seed, level, top.kind), held: 0 };
}

function land(s: Stack, f: Falling, lean: number, drift: number) {
  const top = s.blocks[s.blocks.length - 1];
  const h = topHeight(s);
  // Where it landed, measured like the stack: from the middle of the deck, at that height.
  const x = f.x - FIELD_W / 2 - drift - h * lean;
  const r = resolveDrop(top, x, f.w);
  s.falling = null;
  s.next = NEXT_S;
  if (r.kind === "miss") {
    s.lives--;
    s.misses++;
    s.combo = 0;
    const away = x >= top.x ? 1 : -1;
    s.pieces.push({ x: f.x, h: f.h, w: f.w, vx: away * 10, vh: f.vh * 0.25, angle: 0, spin: away * 1.6, kind: f.kind, splashed: null });
    s.events.push("miss");
    s.landed = null;
    if (s.lives <= 0) {
      s.over = true;
      s.crane = null;
      s.events.push("over");
    }
    return;
  }
  let grew = false;
  if (r.kind === "perfect") {
    s.combo++;
    s.perfects++;
    s.bestCombo = Math.max(s.bestCombo, s.combo);
    const w = restoredWidth(r.block.w, s.combo);
    grew = w > r.block.w;
    s.blocks.push({ x: r.block.x, w, kind: f.kind });
    s.events.push("perfect");
    if (grew) s.events.push("grow");
  } else {
    s.combo = 0;
    s.blocks.push({ x: r.block.x, w: r.block.w, kind: f.kind });
    const away = r.cut.x >= r.block.x ? 1 : -1;
    s.pieces.push({
      x: worldX(r.cut.x, h, lean, drift),
      h,
      w: r.cut.w,
      vx: away * 14,
      vh: 6,
      angle: 0,
      spin: away * 2.2,
      kind: f.kind,
      splashed: null,
    });
    s.events.push("slice");
  }
  s.landed = { perfect: r.kind === "perfect", grew, t: s.t };
  const score = stackScore(s);
  if ((MILESTONES as readonly number[]).includes(score)) {
    s.cheer = { score, t: s.t };
    s.events.push("milestone");
  }
}

/** Advance the game by `dt` seconds (keep steps small: `advance` splits a frame into ≤ 1/60 s). */
export function step(s: Stack, input: StackInput, dt: number): Stack {
  if (dt <= 0) return s;
  s.t += dt;
  // Over: only the last container still falls into the water (the stack stands still).
  if (s.over) {
    fall(s, dt);
    return s;
  }
  const level = s.blocks.length;
  const lean = swayAt(s.seed, s.t, level, s.weather);
  const drift = driftAt(s.seed, s.t);
  // The view rises smoothly with the stack.
  s.cam += (cameraFor(topHeight(s)) - s.cam) * (1 - Math.exp(-5 * dt));

  if (s.crane) {
    const c = s.crane;
    c.held += dt;
    c.s += craneSpeed(level) * s.weather.crane * (1 + NUDGE * clamp(Number.isFinite(input.nudge) ? input.nudge : 0, -1, 1)) * dt;
    if (input.drop || c.held >= HOLD_S) {
      s.falling = { x: craneX(c.s), h: topHeight(s) + CRANE_GAP, vh: 0, w: c.w, kind: c.kind };
      s.crane = null;
      s.events.push("drop");
    }
  } else if (s.falling) {
    const f = s.falling;
    f.vh -= GRAVITY * dt;
    f.h += f.vh * dt;
    if (f.h <= topHeight(s)) land(s, f, lean, drift);
  } else if (!s.over) {
    s.next -= dt;
    if (s.next <= 0) swingIn(s);
  }
  fall(s, dt);
  return s;
}

/** Sliced ends and missed containers tumble into the harbor, and are gone a moment after. */
function fall(s: Stack, dt: number) {
  const water = DECK_Y - WATER_Y;
  for (const p of s.pieces) {
    p.vh -= GRAVITY * 0.6 * dt;
    p.h += p.vh * dt;
    p.x += p.vx * dt;
    p.angle += p.spin * dt;
    if (p.splashed === null && p.h < water) {
      p.splashed = s.t;
      s.events.push("splash");
    }
  }
  s.pieces = s.pieces.filter((p) => p.splashed === null || s.t - p.splashed < 0.8);
}

/** Run `dt` seconds as steps of at most 1/60 s (a slow frame is capped). A drop press counts once. */
export function advance(s: Stack, input: StackInput, dt: number): Stack {
  let left = Math.min(Math.max(0, dt), 0.1);
  let drop = input.drop;
  while (left > 1e-9) {
    const h = Math.min(left, 1 / 60);
    step(s, { drop, nudge: input.nudge }, h);
    drop = false;
    left -= h;
  }
  return s;
}

// --- Controls ---

export interface StackControls {
  keyLeft: boolean;
  keyRight: boolean;
  /** The phone's slider, when one is connected (a nudge, back to 0 when let go). */
  pad: { connected: boolean; steer: number } | null;
}

/** The crane's nudge from every control: held keys win, then the phone's slider. */
export function nudgeFrom(c: StackControls): number {
  const keys = (c.keyRight ? 1 : 0) - (c.keyLeft ? 1 : 0);
  if (keys !== 0) return keys;
  return c.pad?.connected ? clamp(Number.isFinite(c.pad.steer) ? c.pad.steer : 0, -1, 1) : 0;
}

/**
 * A phone's Drop as the game reads it from `game-pad` events: `fresh` on the press itself (the
 * rising edge), never while it's held. The caller latches a fresh press until the next frame takes
 * it, so a tap whose press and release both land between two frames still drops exactly once.
 */
export function padDrop(wasHeld: boolean, e: { connected: boolean; drop: boolean }): { fresh: boolean; held: boolean } {
  const held = e.connected && e.drop;
  return { fresh: held && !wasHeld, held };
}

/** A key in a stack-off: whose drop it is (Player 1 ↓ or Enter, Player 2 S), or null. */
export function stackOffKey(key: string): 1 | 2 | null {
  const k = key.length === 1 ? key.toLowerCase() : key;
  if (k === "ArrowDown" || k === "Enter") return 1;
  if (k === "s") return 2;
  return null;
}

/** What today's weather means for a stack (the daily's "expect …", with `conditionsLine`). */
export function stackOutlook(c: Conditions, h: Handling): string[] {
  const expect: string[] = [];
  if (c.snow) expect.push("a slow, icy crane");
  if (h.waves > 0) expect.push("choppy water");
  if (Math.abs(h.drift) >= 2) expect.push("wind leaning on the stack");
  if (c.night) expect.push("a lighthouse to work by");
  return expect;
}

/** The milestone just reached, if `score` is one, for the celebration's words. */
export function milestoneLine(score: number): string | null {
  switch (score) {
    case 10:
      return "10 containers! The harbor master nods.";
    case 25:
      return "25! The gulls are impressed.";
    case 50:
      return "50 containers! A skyline of your own.";
    case 100:
      return "100! They'll name a pier after you.";
    default:
      return null;
  }
}
