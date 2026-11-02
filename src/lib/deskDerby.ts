// Desk Derby: the game's rules, pure and deterministic, like Tugboat Run's and Harbor Stack's. A
// tiny RC car races three laps of a track laid out across a cluttered office desk, seen from above.
// The car always drives itself forward; you steer, and holding the action button drifts: the rear
// slides out and the car turns tighter. Held through a corner, a drift charges a boost (three
// levels, the sparks change colour), released when you let go. That's the skill.
//
// The desk: the lane is a strip of desk mat edged with tape (off it, the bare wood is slow), the
// keyboard rattles the car as it crosses, a spilled coffee puddle has hardly any grip, the mug, the
// mouse and pencils laid between lanes are solid, sticky notes are the checkpoints, a stapler is a
// ramp (a short jump: no steering in the air), and paper clips are pickups (a little zip of boost).
// Off the edge of the desk is a fall: back to the last sticky note, two seconds added.
//
// Everything runs in fixed steps of 1/120 s (`advance` splits a frame), so the same inputs give the
// same race at any frame rate, and the tests drive whole races. Each track is hand-designed; the
// daily seed picks one (and a mirror image, where the coffee spilled and where the clips lie), so
// the daily race is the same for everyone. A race's time is lower-is-better: the store keeps it in
// whole milliseconds (see `raceMs`). DeskDerby.vue calls `advance` from requestAnimationFrame and
// deskDerbyDraw.ts draws what it returns.
//
// Coordinates are desk units: the desk is `DESK_W` × `DESK_H`, x to the right, y down the screen,
// angles in radians clockwise from +x (the screen's way round).

import { random, rowRandom } from "./tugboatRun";

export const DESK_W = 360;
export const DESK_H = 225;
/** One physics step (seconds). */
export const STEP = 1 / 120;
export const LAPS = 3;
/** 3, 2, 1, GO. */
export const COUNTDOWN_S = 3;

/** The car: length and width (for drawing) and its collision radius. */
export const CAR_L = 10;
export const CAR_W = 5.6;
export const CAR_R = 4;

/** Top speed on the mat (desk units a second), and how quickly the motor gets there. */
export const V_TOP = 46;
const ACCEL = 2;
/** Above the top speed (after a boost, or onto the wood) the car eases back down this quickly. */
const DRAG = 2.2;
/** Turning rate at full lock (radians a second), reached from this speed up. */
export const TURN = 2.7;
const TURN_FULL_V = 16;
/** How quickly the car's travel follows where it points (per second): grip. */
export const GRIP = 18;
export const DRIFT_GRIP = 6.2;
/** Steering eases towards the input this quickly (keys tap to small corrections). */
const STEER_RATE = 13;
/** Speed lost to sliding: at a right angle the car would run at (1 - this) of its top speed. */
const SLIP_DRAG = 0.22;

/** Drifting: needs this much speed to start (and drops below `DRIFT_KEEP_V`). */
export const DRIFT_MIN_V = 22;
const DRIFT_KEEP_V = 14;
/** A drift needs at least this much steering to start. */
const DRIFT_STEER = 0.3;
/** Drifting is a little slower than gripping (so a drift without its boost costs time). */
const DRIFT_SPEED = 0.96;
/** Charge (seconds of good drifting) for boost levels 1, 2 and 3, and how long each boost lasts. */
export const CHARGE_LEVELS = [0.5, 1.1, 1.8] as const;
export const BOOST_S = [0.55, 0.9, 1.3] as const;
/** A boost's top speed, as a share of the usual. */
export const BOOST_V = 1.4;
const BOOST_ACCEL = 6;

/** Surfaces: the bare desk is slow; the keyboard rattles; the coffee is slippery. */
export const WOOD_SPEED = 0.64;
const WOOD_GRIP = 0.75;
export const KEYS_SPEED = 0.9;
export const PUDDLE_GRIP = 0.22;

/** The stapler: a jump needs this much speed; in the air for about half a second. */
const RAMP_MIN_V = 14;
export const RAMP_LEN = 15;
export const RAMP_W = 12;
const AIR_BASE = 0.3;
const AIR_PER_V = 0.32 / V_TOP;
/** A clean landing on the mat gives a little zip. */
const LANDING_BOOST = 0.3;
/** A paper clip gives this much boost (stacking up to `CLIP_BOOST_MAX`). */
export const CLIP_BOOST = 0.35;
const CLIP_BOOST_MAX = 1.6;
const CLIP_R = 4.2;
export const CLIPS_PER_LAP = 6;

/** Falling off the desk: the car tumbles for this long, then it's back at the last sticky note
 * with this much added to its time. */
export const FALL_S = 0.9;
export const PENALTY_S = 2;
/** After coming back the car can't fall again straight away. */
const RESPAWN_GRACE_S = 0.8;

/** Bumping something solid: how much speed is kept, and how bouncy. */
const BUMP_KEEP = 0.72;
const BOUNCE = 0.3;

/** Two players: once one finishes, the other has this long. A race can't run longer than `MAX_S`. */
export const FINISH_GRACE_S = 20;
export const MAX_S = 300;

/** Ghosts: a pose sampled this often. */
export const GHOST_HZ = 10;

const TAU = Math.PI * 2;
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
/** An angle folded into (-π, π]. */
export function wrapAngle(a: number): number {
  let r = a % TAU;
  if (r <= -Math.PI) r += TAU;
  else if (r > Math.PI) r -= TAU;
  return r;
}

// --- Tracks ---

export type TrackId = "inbox" | "coffee" | "esses" | "edge";

export interface Circle {
  x: number;
  y: number;
  r: number;
}
export interface Segment {
  x1: number;
  y1: number;
  x2: number;
  y2: number;
}
/** A box: centre, size along and across its angle. */
export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
  angle: number;
}
export interface Pose {
  x: number;
  y: number;
  angle: number;
}

/** One hand-designed layout. Features on the lane are placed by how far round the lap they are
 * (0 to 1 from the start line); the desk's furniture where it stands. */
export interface TrackDef {
  id: TrackId;
  name: string;
  blurb: string;
  /** The lane's centre line: a closed loop through these points, in driving order. */
  points: ReadonlyArray<readonly [number, number]>;
  /** Half the lane's width. */
  half: number;
  /** The sticky notes, as shares of the lap: the start line first (0). */
  checkpoints: readonly number[];
  keyboard: Box;
  mug: Circle;
  /** The mouse: centre and the way its cable runs. */
  mouse: Pose;
  /** Pencils and pens laid between lanes (solid). */
  pencils: ReadonlyArray<Segment & { kind: "pencil" | "pen" }>;
  /** Where the stapler stands on the lane. */
  stapler: number;
  /** Where coffee might have spilled (the seed picks one). */
  puddles: readonly number[];
  /** Loose paper under everything (decoration), as boxes. */
  papers: readonly Box[];
}

export const TRACKS: readonly TrackDef[] = [
  {
    id: "inbox",
    name: "Inbox Loop",
    blurb: "Round the desk and down into the in-tray. A friendly first lap.",
    points: [
      [80, 196],
      [160, 197],
      [240, 196],
      [300, 190],
      [332, 166],
      [324, 132],
      [340, 100],
      [330, 64],
      [300, 38],
      [258, 30],
      [222, 44],
      [206, 80],
      [196, 116],
      [172, 140],
      [146, 128],
      [138, 92],
      [124, 52],
      [94, 30],
      [54, 32],
      [28, 58],
      [24, 104],
      [28, 150],
      [44, 184],
    ],
    half: 11,
    checkpoints: [0, 0.25, 0.47, 0.72],
    keyboard: { x: 196, y: 196, w: 120, h: 36, angle: 0 },
    mug: { x: 297, y: 118, r: 11 },
    mouse: { x: 262, y: 156, angle: -0.3 },
    pencils: [
      { x1: 171, y1: 42, x2: 170, y2: 112, kind: "pencil" },
      { x1: 70, y1: 64, x2: 72, y2: 150, kind: "pen" },
    ],
    stapler: 0.79,
    puddles: [0.86, 0.36, 0.6],
    papers: [
      { x: 92, y: 106, w: 54, h: 72, angle: -0.12 },
      { x: 250, y: 108, w: 48, h: 62, angle: 0.1 },
    ],
  },
  {
    id: "coffee",
    name: "Coffee Run",
    blurb: "Tight round the mug, a dash down the keyboard, and mind the spill.",
    points: [
      [60, 30],
      [150, 26],
      [236, 28],
      [306, 32],
      [338, 60],
      [334, 104],
      [300, 118],
      [258, 104],
      [222, 82],
      [188, 92],
      [178, 130],
      [204, 160],
      [262, 158],
      [312, 162],
      [337, 174],
      [342, 194],
      [322, 210],
      [280, 211],
      [180, 206],
      [90, 204],
      [40, 190],
      [24, 150],
      [26, 100],
      [36, 56],
    ],
    half: 11,
    checkpoints: [0, 0.26, 0.5, 0.74],
    keyboard: { x: 150, y: 202, w: 124, h: 34, angle: 0.02 },
    mug: { x: 262, y: 132, r: 12 },
    mouse: { x: 132, y: 152, angle: 0.4 },
    pencils: [
      { x1: 236, y1: 58, x2: 316, y2: 82, kind: "pencil" },
      { x1: 220, y1: 182, x2: 300, y2: 186, kind: "pen" },
    ],
    stapler: 0.12,
    puddles: [0.2, 0.62, 0.9],
    papers: [
      { x: 150, y: 70, w: 60, h: 46, angle: 0.08 },
      { x: 300, y: 138, w: 40, h: 34, angle: -0.2 },
    ],
  },
  {
    id: "esses",
    name: "Deadline Esses",
    blurb: "Back and forth across the desk. Drift the bends or run out of road.",
    points: [
      [40, 202],
      [130, 203],
      [220, 201],
      [300, 198],
      [336, 178],
      [326, 146],
      [280, 140],
      [236, 148],
      [206, 141],
      [196, 121],
      [212, 102],
      [250, 98],
      [316, 96],
      [338, 66],
      [318, 34],
      [250, 26],
      [180, 30],
      [140, 54],
      [134, 92],
      [118, 130],
      [86, 146],
      [62, 128],
      [58, 90],
      [57, 62],
      [56, 42],
      [50, 27],
      [35, 21],
      [20, 27],
      [14, 44],
      [14, 74],
      [14, 130],
      [18, 176],
    ],
    half: 10.5,
    checkpoints: [0, 0.24, 0.5, 0.76],
    keyboard: { x: 160, y: 200, w: 126, h: 34, angle: 0 },
    mug: { x: 182, y: 96, r: 11 },
    mouse: { x: 96, y: 96, angle: 1.2 },
    pencils: [
      { x1: 236, y1: 122, x2: 322, y2: 120, kind: "pencil" },
      { x1: 230, y1: 170, x2: 310, y2: 174, kind: "pen" },
      { x1: 36, y1: 56, x2: 37, y2: 150, kind: "pencil" },
    ],
    stapler: 0.58,
    puddles: [0.42, 0.66, 0.08],
    papers: [
      { x: 266, y: 62, w: 70, h: 40, angle: -0.06 },
      { x: 80, y: 172, w: 50, h: 30, angle: 0.15 },
    ],
  },
  {
    id: "edge",
    name: "Edge Case",
    blurb: "Fast sweepers right along the desk's edge. Brave lines win.",
    points: [
      [70, 210],
      [180, 211],
      [290, 210],
      [334, 198],
      [346, 162],
      [328, 132],
      [326, 104],
      [346, 76],
      [344, 36],
      [310, 17],
      [250, 17],
      [222, 40],
      [226, 84],
      [204, 112],
      [160, 104],
      [146, 66],
      [120, 32],
      [70, 17],
      [28, 26],
      [14, 80],
      [16, 140],
      [30, 194],
    ],
    half: 11,
    checkpoints: [0, 0.27, 0.5, 0.75],
    keyboard: { x: 120, y: 194, w: 120, h: 36, angle: -0.03 },
    mug: { x: 290, y: 118, r: 13 },
    mouse: { x: 92, y: 120, angle: -0.6 },
    pencils: [{ x1: 186, y1: 22, x2: 186, y2: 80, kind: "pen" }],
    stapler: 0.88,
    puddles: [0.36, 0.14, 0.62],
    papers: [
      { x: 70, y: 76, w: 64, h: 48, angle: 0.1 },
      { x: 284, y: 84, w: 50, h: 70, angle: -0.08 },
    ],
  },
];

export const TRACK_IDS: TrackId[] = TRACKS.map((t) => t.id);

export function trackDef(id: string): TrackDef {
  return TRACKS.find((t) => t.id === id) ?? TRACKS[0];
}

/** Which layout and clutter a race uses: the daily seed picks it; Free play is the layout as drawn. */
export interface Variant {
  track: TrackId;
  /** Mirrored left to right (a new set of corners from the same layout). */
  mirror: boolean;
  /** Which of the layout's spill spots has the coffee. */
  puddle: number;
  /** Where the paper clips lie. */
  clipSeed: number;
}

/** The day's race from the daily seed: the same layout, mirror, spill and clips for everyone. */
export function dailyVariant(seed: number): Variant {
  const rng = rowRandom(seed ^ 0xde5c, 0);
  const track = TRACK_IDS[Math.floor(random(rng) * TRACK_IDS.length)];
  const mirror = random(rng) < 0.5;
  const puddle = Math.floor(random(rng) * 3);
  return { track, mirror, puddle, clipSeed: seed | 0 };
}

/** Free play and two-player races: the layout as designed, its clips always in the same places
 * (so a ghost of your best on it is a fair race). */
export function freeVariant(track: TrackId): Variant {
  return { track, mirror: false, puddle: 0, clipSeed: 0x5eed + TRACK_IDS.indexOf(track) };
}

/** A built track: the lane's centre line sampled every `SPACING` units, and everything on the desk. */
export interface Track {
  id: TrackId;
  name: string;
  variant: Variant;
  half: number;
  /** Samples along the centre line, and their unit tangents. */
  xs: Float64Array;
  ys: Float64Array;
  txs: Float64Array;
  tys: Float64Array;
  /** The lap's length. */
  length: number;
  checkpoints: Pose[];
  /** The stapler: where it stands (pointing the way to drive), its size from `RAMP_LEN`/`RAMP_W`. */
  ramp: Pose;
  puddle: Box;
  keyboard: Box;
  mug: Circle;
  mouse: Pose;
  pencils: Array<Segment & { kind: "pencil" | "pen" }>;
  papers: Box[];
  /** The paper clips (the same each lap). */
  clips: Array<{ x: number; y: number }>;
  /** Where cars start: one just behind the line, or two side by side. */
  grid: Pose[];
  /** Everything a car bumps into. */
  solids: Solid[];
}

export type Solid = ({ type: "circle" } & Circle) | ({ type: "capsule"; r: number } & Segment);

const SPACING = 1.5;
/** The mouse, as the car feels it. */
const MOUSE_LEN = 9;
const MOUSE_R = 6;
const PENCIL_R = 1.6;
const PEN_R = 2;

/** A closed centripetal Catmull-Rom curve through `pts`, as a dense polyline. */
function spline(pts: ReadonlyArray<readonly [number, number]>, per = 24): Array<[number, number]> {
  const n = pts.length;
  const out: Array<[number, number]> = [];
  const knot = (a: readonly [number, number], b: readonly [number, number]) => Math.max(1e-6, Math.sqrt(Math.hypot(b[0] - a[0], b[1] - a[1])));
  for (let i = 0; i < n; i++) {
    const p0 = pts[(i - 1 + n) % n];
    const p1 = pts[i];
    const p2 = pts[(i + 1) % n];
    const p3 = pts[(i + 2) % n];
    const t0 = 0;
    const t1 = t0 + knot(p0, p1);
    const t2 = t1 + knot(p1, p2);
    const t3 = t2 + knot(p2, p3);
    for (let k = 0; k < per; k++) {
      const t = t1 + ((t2 - t1) * k) / per;
      const lerp = (a: readonly [number, number], b: readonly [number, number], ta: number, tb: number): [number, number] => {
        const u = (t - ta) / (tb - ta);
        return [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u];
      };
      const a1 = lerp(p0, p1, t0, t1);
      const a2 = lerp(p1, p2, t1, t2);
      const a3 = lerp(p2, p3, t2, t3);
      const b1 = lerp(a1, a2, t0, t2);
      const b2 = lerp(a2, a3, t1, t3);
      out.push(lerp(b1, b2, t1, t2));
    }
  }
  return out;
}

/** The point and direction `s` along the lap (any s; it wraps). */
export function poseAt(t: Pick<Track, "xs" | "ys" | "txs" | "tys" | "length">, s: number): Pose {
  const n = t.xs.length;
  const u = ((((s % t.length) + t.length) % t.length) / t.length) * n;
  const i = Math.floor(u) % n;
  const j = (i + 1) % n;
  const f = u - Math.floor(u);
  const x = t.xs[i] + (t.xs[j] - t.xs[i]) * f;
  const y = t.ys[i] + (t.ys[j] - t.ys[i]) * f;
  return { x, y, angle: Math.atan2(t.tys[i] + (t.tys[j] - t.tys[i]) * f, t.txs[i] + (t.txs[j] - t.txs[i]) * f) };
}

/** Build a track for a variant: its centre line, and everything on the desk where it lies. */
export function buildTrack(v: Variant): Track {
  const def = trackDef(v.track);
  const mx = (x: number) => (v.mirror ? DESK_W - x : x);
  // A direction mirrors to its reflection; a box (which looks the same turned half round) to -angle.
  const ma = (a: number) => (v.mirror ? Math.PI - a : a);
  const mb = (a: number) => (v.mirror ? -a : a);
  // Mirroring flips which way round the lap goes; the lap still starts at the same point.
  const pts = def.points.map(([x, y]) => [mx(x), y] as const);
  const dense = spline(pts);
  // Resample by distance.
  const cum = [0];
  for (let i = 1; i <= dense.length; i++) {
    const a = dense[i - 1];
    const b = dense[i % dense.length];
    cum.push(cum[i - 1] + Math.hypot(b[0] - a[0], b[1] - a[1]));
  }
  const length = cum[cum.length - 1];
  const n = Math.round(length / SPACING);
  const xs = new Float64Array(n);
  const ys = new Float64Array(n);
  let k = 0;
  for (let i = 0; i < n; i++) {
    const want = (i * length) / n;
    while (cum[k + 1] < want) k++;
    const a = dense[k];
    const b = dense[(k + 1) % dense.length];
    const f = (want - cum[k]) / Math.max(1e-9, cum[k + 1] - cum[k]);
    xs[i] = a[0] + (b[0] - a[0]) * f;
    ys[i] = a[1] + (b[1] - a[1]) * f;
  }
  const txs = new Float64Array(n);
  const tys = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    const dx = xs[(i + 1) % n] - xs[(i - 1 + n) % n];
    const dy = ys[(i + 1) % n] - ys[(i - 1 + n) % n];
    const d = Math.hypot(dx, dy) || 1;
    txs[i] = dx / d;
    tys[i] = dy / d;
  }
  const line = { xs, ys, txs, tys, length };
  const at = (share: number) => poseAt(line, share * length);

  const pencils = def.pencils.map((p) => ({ x1: mx(p.x1), y1: p.y1, x2: mx(p.x2), y2: p.y2, kind: p.kind }));
  const mug = { x: mx(def.mug.x), y: def.mug.y, r: def.mug.r };
  const mouse = { x: mx(def.mouse.x), y: def.mouse.y, angle: ma(def.mouse.angle) };
  const spill = at(def.puddles[clamp(v.puddle, 0, def.puddles.length - 1)]);
  const behind = (d: number, side: number): Pose => {
    const p = poseAt(line, -d);
    return { x: p.x - Math.sin(p.angle) * side, y: p.y + Math.cos(p.angle) * side, angle: p.angle };
  };
  const solids: Solid[] = [
    { type: "circle", ...mug },
    {
      type: "capsule",
      x1: mouse.x - (Math.cos(mouse.angle) * MOUSE_LEN) / 2,
      y1: mouse.y - (Math.sin(mouse.angle) * MOUSE_LEN) / 2,
      x2: mouse.x + (Math.cos(mouse.angle) * MOUSE_LEN) / 2,
      y2: mouse.y + (Math.sin(mouse.angle) * MOUSE_LEN) / 2,
      r: MOUSE_R,
    },
    ...pencils.map((p): Solid => ({ type: "capsule", x1: p.x1, y1: p.y1, x2: p.x2, y2: p.y2, r: p.kind === "pen" ? PEN_R : PENCIL_R })),
  ];
  const track: Track = {
    id: def.id,
    name: def.name,
    variant: v,
    half: def.half,
    ...line,
    checkpoints: def.checkpoints.map(at),
    ramp: at(def.stapler),
    puddle: { x: spill.x, y: spill.y, w: 30, h: def.half * 2 + 2, angle: spill.angle },
    keyboard: { ...def.keyboard, x: mx(def.keyboard.x), angle: mb(def.keyboard.angle) },
    mug,
    mouse,
    pencils,
    papers: def.papers.map((p) => ({ ...p, x: mx(p.x), angle: mb(p.angle) })),
    clips: [],
    grid: [behind(10, 0), behind(10, -def.half * 0.48), behind(10, def.half * 0.48)],
    solids,
  };
  track.clips = placeClips(track, v.clipSeed, def);
  return track;
}

/** The paper clips: spread round the lap, each somewhere across the lane, clear of the sticky
 * notes, the stapler and the spill. */
function placeClips(t: Track, seed: number, def: TrackDef): Array<{ x: number; y: number }> {
  const rng = rowRandom(seed ^ 0xc11b, 7);
  const avoid = [...def.checkpoints, def.stapler, def.puddles[clamp(t.variant.puddle, 0, def.puddles.length - 1)]];
  const near = (f: number) => avoid.some((a) => Math.abs(wrapShare(f - a)) < 0.035);
  const out: Array<{ x: number; y: number }> = [];
  for (let i = 0; i < CLIPS_PER_LAP; i++) {
    let f = (i + 0.5) / CLIPS_PER_LAP + (random(rng) - 0.5) * 0.08;
    for (let tries = 0; near(f) && tries < 8; tries++) f += 0.03;
    const p = poseAt(t, f * t.length);
    const side = (random(rng) * 2 - 1) * t.half * 0.55;
    const c = { x: p.x - Math.sin(p.angle) * side, y: p.y + Math.cos(p.angle) * side };
    // Never under the mug or a pencil.
    if (t.solids.some((s) => solidDistance(s, c.x, c.y) < CLIP_R + 1)) {
      out.push({ x: p.x, y: p.y });
    } else out.push(c);
  }
  return out;
}

function wrapShare(f: number): number {
  return f - Math.round(f);
}

/** The nearest point on the lane's centre line: its sample, how far along the lap it is, and how
 * far off it (and to which side: positive is to the right of the driving direction). */
export function nearest(t: Track, x: number, y: number): { i: number; s: number; off: number; dist: number } {
  let best = 0;
  let bd = Infinity;
  const n = t.xs.length;
  for (let i = 0; i < n; i += 4) {
    const d = (t.xs[i] - x) ** 2 + (t.ys[i] - y) ** 2;
    if (d < bd) {
      bd = d;
      best = i;
    }
  }
  for (let i = best - 4; i <= best + 4; i++) {
    const j = (i + n) % n;
    const d = (t.xs[j] - x) ** 2 + (t.ys[j] - y) ** 2;
    if (d < bd) {
      bd = d;
      best = j;
    }
  }
  const off = -(x - t.xs[best]) * t.tys[best] + (y - t.ys[best]) * t.txs[best];
  return { i: best, s: (best / n) * t.length, off, dist: Math.sqrt(bd) };
}

/** Whether a point is on the lane (the mat between the tape). */
export function onLane(t: Track, x: number, y: number): boolean {
  return nearest(t, x, y).dist <= t.half;
}

/** Whether a point is inside a box (centre, size and angle). */
export function inBox(b: Box, x: number, y: number, grow = 0): boolean {
  const c = Math.cos(b.angle);
  const s = Math.sin(b.angle);
  const dx = x - b.x;
  const dy = y - b.y;
  const u = dx * c + dy * s;
  const v = -dx * s + dy * c;
  return Math.abs(u) <= b.w / 2 + grow && Math.abs(v) <= b.h / 2 + grow;
}

/** Whether a point is in the coffee (an ellipse filling the box). */
export function inPuddle(t: Track, x: number, y: number): boolean {
  const b = t.puddle;
  const c = Math.cos(b.angle);
  const s = Math.sin(b.angle);
  const dx = x - b.x;
  const dy = y - b.y;
  const u = (dx * c + dy * s) / (b.w / 2);
  const v = (-dx * s + dy * c) / (b.h / 2);
  return u * u + v * v <= 1;
}

export function onDesk(x: number, y: number): boolean {
  return x >= 0 && x <= DESK_W && y >= 0 && y <= DESK_H;
}

/** How far a point is from a solid's surface (negative inside). */
export function solidDistance(s: Solid, x: number, y: number): number {
  if (s.type === "circle") return Math.hypot(x - s.x, y - s.y) - s.r;
  const dx = s.x2 - s.x1;
  const dy = s.y2 - s.y1;
  const len2 = dx * dx + dy * dy || 1;
  const u = clamp(((x - s.x1) * dx + (y - s.y1) * dy) / len2, 0, 1);
  return Math.hypot(x - (s.x1 + dx * u), y - (s.y1 + dy * u)) - s.r;
}

/** The point on a solid nearest to (x, y), for pushing a car out. */
function closestOn(s: Solid, x: number, y: number): { x: number; y: number; r: number } {
  if (s.type === "circle") return { x: s.x, y: s.y, r: s.r };
  const dx = s.x2 - s.x1;
  const dy = s.y2 - s.y1;
  const len2 = dx * dx + dy * dy || 1;
  const u = clamp(((x - s.x1) * dx + (y - s.y1) * dy) / len2, 0, 1);
  return { x: s.x1 + dx * u, y: s.y1 + dy * u, r: s.r };
}

/** Whether a point is on the keyboard. */
export function onKeyboard(t: Track, x: number, y: number): boolean {
  return inBox(t.keyboard, x, y);
}

/** The little jolt of crossing the keys (0 to 1), for drawing: rows and columns of keycaps. */
export function keyBump(t: Track, x: number, y: number): number {
  if (!onKeyboard(t, x, y)) return 0;
  const b = t.keyboard;
  const c = Math.cos(b.angle);
  const s = Math.sin(b.angle);
  const u = (x - b.x) * c + (y - b.y) * s;
  const v = -(x - b.x) * s + (y - b.y) * c;
  const k = Math.abs(Math.sin((u / KEY) * Math.PI)) * Math.abs(Math.sin((v / KEY) * Math.PI));
  return k;
}
/** A keycap's pitch. */
export const KEY = 7.4;

/** Whether a point is on the stapler's ramp. */
export function onRamp(t: Track, x: number, y: number): boolean {
  return inBox({ ...t.ramp, w: RAMP_LEN, h: RAMP_W }, x, y);
}

// --- Cars and the race ---

export type CarEvent = "drift" | "charge" | "boost" | "bump" | "jump" | "land" | "fall" | "respawn" | "clip" | "checkpoint" | "lap" | "final-lap" | "finish";

export interface Car {
  x: number;
  y: number;
  /** Where it points. */
  angle: number;
  /** Where it's going: speed and direction (the difference from `angle` is the slide). */
  speed: number;
  dir: number;
  /** The steering as the car has it (eased from the input). */
  steer: number;
  /** Drifting: which way (-1 left, 1 right), or 0. */
  drift: -1 | 0 | 1;
  /** Seconds of drift charge, and the level it has reached (0 to 3). */
  charge: number;
  level: number;
  /** Seconds of boost left. */
  boost: number;
  /** In the air off the stapler: seconds left of how many. */
  air: number;
  airTotal: number;
  /** Tumbling off the desk: seconds so far, or null. */
  fall: number | null;
  /** Seconds before it can fall again (just respawned). */
  grace: number;
  /** Laps done, and the sticky note it needs next (0 is the start line). */
  lap: number;
  next: number;
  /** The last sticky note it touched (where a fall brings it back). */
  lastCp: number;
  /** Seconds added for falls. */
  penalty: number;
  falls: number;
  bumps: number;
  clips: number;
  /** Which clips it has picked up this lap. */
  taken: boolean[];
  /** The clock (race time + penalties) when the current lap started, and each lap's time. */
  lapStart: number;
  laps: number[];
  /** Finished: its race time (seconds, penalties included), or null. Out of time: `dnf`. */
  finished: number | null;
  dnf: boolean;
  /** Where it is on the lane now, for the surface and for who's ahead. */
  surface: "lane" | "wood" | "keys" | "coffee";
  /** Seconds since it last bumped something (for not counting one scrape as many bumps). */
  sinceBump: number;
  events: CarEvent[];
}

export interface Race {
  track: Track;
  /** Seconds since GO (negative during the countdown). */
  t: number;
  cars: Car[];
  /** Once someone finishes in a two-player race, the others have until this. */
  deadline: number | null;
  over: boolean;
  /** Car 1's poses for a ghost (x, y in tenths, angle in hundredths), one every 1/GHOST_HZ s. */
  ghost: number[];
  events: Array<"count" | "go" | "over">;
}

export interface CarInput {
  /** -1 (full left) to 1 (full right). */
  steer: number;
  /** The action button is held: drift. */
  drift: boolean;
}

export const NO_INPUT: CarInput = { steer: 0, drift: false };

function newCar(p: Pose, clips: number): Car {
  return {
    x: p.x,
    y: p.y,
    angle: p.angle,
    speed: 0,
    dir: p.angle,
    steer: 0,
    drift: 0,
    charge: 0,
    level: 0,
    boost: 0,
    air: 0,
    airTotal: 0,
    fall: null,
    grace: 0,
    lap: 0,
    next: 1,
    lastCp: 0,
    penalty: 0,
    falls: 0,
    bumps: 0,
    clips: 0,
    taken: new Array<boolean>(clips).fill(false),
    lapStart: 0,
    laps: [],
    finished: null,
    dnf: false,
    surface: "lane",
    sinceBump: 1,
    events: [],
  };
}

/** A new race on `track`: one car, or two side by side. Starts with the countdown. */
export function newRace(track: Track, players: 1 | 2 = 1): Race {
  const cars = players === 2 ? [newCar(track.grid[1], track.clips.length), newCar(track.grid[2], track.clips.length)] : [newCar(track.grid[0], track.clips.length)];
  return { track, t: -COUNTDOWN_S, cars, deadline: null, over: false, ghost: [], events: [] };
}

/** The car's race clock: time since GO plus penalties (frozen once it finishes). */
export function clockOf(r: Pick<Race, "t">, c: Car): number {
  if (c.finished !== null) return c.finished;
  return Math.max(0, r.t) + c.penalty;
}

/** A finished race time in whole milliseconds (how the store keeps it), or 0 for no time. */
export function raceMs(c: Pick<Car, "finished" | "dnf">): number {
  return c.finished !== null && !c.dnf ? Math.max(1, Math.round(c.finished * 1000)) : 0;
}

/** The boost level a drift's charge has reached (0 to 3). */
export function chargeLevel(charge: number): number {
  let l = 0;
  for (const need of CHARGE_LEVELS) if (charge >= need) l++;
  return l;
}

/** How far round the race a car is (laps and distance), for who's ahead. */
export function progress(r: Race, c: Car): number {
  if (c.finished !== null) return LAPS * r.track.length + 1e6 - c.finished;
  const s = nearest(r.track, c.x, c.y).s;
  // Before the first sticky note it's still near the start: don't count the end of the lap.
  const along = c.next === 1 && s > r.track.length / 2 ? s - r.track.length : s;
  return c.lap * r.track.length + along;
}

/** Back on the lane at the last sticky note, pointing the right way, rolling. */
function respawn(r: Race, c: Car) {
  const p = r.track.checkpoints[c.lastCp];
  c.x = p.x;
  c.y = p.y;
  c.angle = c.dir = p.angle;
  c.speed = V_TOP * 0.4;
  c.steer = 0;
  c.drift = 0;
  c.charge = 0;
  c.level = 0;
  c.boost = 0;
  c.air = 0;
  c.fall = null;
  c.grace = RESPAWN_GRACE_S;
  c.penalty += PENALTY_S;
  c.events.push("respawn");
}

function stepCar(r: Race, c: Car, input: CarInput, dt: number) {
  const t = r.track;
  c.sinceBump += dt;
  c.grace = Math.max(0, c.grace - dt);
  const done = c.finished !== null || c.dnf;
  if (done) input = NO_INPUT;

  // Tumbling off the desk: keep going (falling), then back at the last sticky note.
  if (c.fall !== null) {
    c.fall += dt;
    c.x += Math.cos(c.dir) * c.speed * dt;
    c.y += Math.sin(c.dir) * c.speed * dt;
    c.speed *= Math.exp(-2 * dt);
    if (c.fall >= FALL_S) respawn(r, c);
    return;
  }

  // In the air off the stapler: no steering, no grip, no motor; it lands where it lands.
  if (c.air > 0) {
    c.air -= dt;
    c.x += Math.cos(c.dir) * c.speed * dt;
    c.y += Math.sin(c.dir) * c.speed * dt;
    if (c.air <= 0) {
      c.air = 0;
      c.events.push("land");
      if (!onDesk(c.x, c.y)) {
        fallOff(c);
        return;
      }
      if (!done && onLane(t, c.x, c.y)) c.boost = Math.max(c.boost, LANDING_BOOST);
    }
    touch(r, c);
    return;
  }

  // Steering eases towards the input.
  const want = done ? 0 : clamp(Number.isFinite(input.steer) ? input.steer : 0, -1, 1);
  c.steer += (want - c.steer) * (1 - Math.exp(-STEER_RATE * dt));

  // The surface underneath.
  const lane = onLane(t, c.x, c.y);
  const coffee = inPuddle(t, c.x, c.y);
  const keys = onKeyboard(t, c.x, c.y);
  c.surface = coffee ? "coffee" : keys ? "keys" : lane ? "lane" : "wood";
  let top = V_TOP * (lane ? 1 : WOOD_SPEED) * (keys ? KEYS_SPEED : 1);
  let grip = (c.drift ? DRIFT_GRIP : GRIP) * (lane ? 1 : WOOD_GRIP) * (coffee ? PUDDLE_GRIP : 1);
  if (done) top = 0;

  // Drifting: starts with the button held and some steering at speed; ends on letting go (with the
  // charged boost) or when the car slows too much (without).
  if (c.drift === 0 && input.drift && Math.abs(c.steer) >= DRIFT_STEER && c.speed >= DRIFT_MIN_V) {
    c.drift = c.steer > 0 ? 1 : -1;
    c.charge = 0;
    c.level = 0;
    c.events.push("drift");
  } else if (c.drift !== 0 && (!input.drift || c.speed < DRIFT_KEEP_V)) {
    const level = input.drift ? 0 : chargeLevel(c.charge);
    if (level > 0) {
      c.boost = Math.max(c.boost, BOOST_S[level - 1]);
      c.events.push("boost");
    }
    c.drift = 0;
    c.charge = 0;
    c.level = 0;
    grip = GRIP * (lane ? 1 : WOOD_GRIP) * (coffee ? PUDDLE_GRIP : 1);
  }

  // Turning: steering turns the car; drifting turns it harder (steer into the drift to tighten it,
  // out of it to widen). Little turning at a crawl.
  let yaw: number;
  if (c.drift !== 0) {
    const k = clamp(c.steer * c.drift, -1, 1);
    yaw = c.drift * TURN * (1.05 + 0.42 * k);
    if (c.speed >= DRIFT_KEEP_V) {
      c.charge += dt * (0.7 + 0.5 * Math.max(0, k)) * (coffee ? 0.5 : 1);
      const l = chargeLevel(c.charge);
      if (l > c.level) {
        c.level = l;
        c.events.push("charge");
      }
    }
    top *= DRIFT_SPEED;
  } else {
    yaw = c.steer * TURN;
  }
  yaw *= Math.min(1, c.speed / TURN_FULL_V);
  c.angle = wrapAngle(c.angle + yaw * dt);

  // Grip: the direction of travel follows where the car points.
  const slip = wrapAngle(c.angle - c.dir);
  c.dir = wrapAngle(c.dir + slip * (1 - Math.exp(-grip * dt)));

  // The motor: always on, towards the top speed (less while sliding, more while boosting).
  if (c.boost > 0) top *= BOOST_V;
  top *= 1 - SLIP_DRAG * Math.abs(Math.sin(wrapAngle(c.angle - c.dir)));
  const rate = c.boost > 0 && c.speed < top ? BOOST_ACCEL : c.speed < top ? ACCEL : DRAG;
  c.speed += (top - c.speed) * (1 - Math.exp(-rate * dt));
  c.boost = Math.max(0, c.boost - dt);

  c.x += Math.cos(c.dir) * c.speed * dt;
  c.y += Math.sin(c.dir) * c.speed * dt;

  collide(r, c);

  // The stapler: up and over, if going fast enough the right way.
  if (onRamp(t, c.x, c.y) && c.speed >= RAMP_MIN_V && Math.cos(c.dir - t.ramp.angle) > 0.5) {
    c.airTotal = c.air = AIR_BASE + c.speed * AIR_PER_V;
    if (c.drift !== 0) {
      c.drift = 0;
      c.charge = 0;
      c.level = 0;
    }
    c.events.push("jump");
  }

  if (!onDesk(c.x, c.y) && c.grace <= 0) {
    fallOff(c);
    return;
  }
  // Just respawned near an edge: nudge it back onto the desk instead.
  c.x = clamp(c.x, 0, DESK_W);
  c.y = clamp(c.y, 0, DESK_H);
  touch(r, c);
}

function fallOff(c: Car) {
  c.fall = 0;
  c.falls++;
  c.drift = 0;
  c.charge = 0;
  c.level = 0;
  c.boost = 0;
  c.events.push("fall");
}

/** Bump off anything solid: pushed out, slowed, a little bounce, and a drift's charge lost. */
function collide(r: Race, c: Car) {
  for (const s of r.track.solids) {
    const p = closestOn(s, c.x, c.y);
    const dx = c.x - p.x;
    const dy = c.y - p.y;
    const d = Math.hypot(dx, dy);
    const min = p.r + CAR_R;
    if (d >= min) continue;
    const nx = d > 1e-6 ? dx / d : -Math.cos(c.dir);
    const ny = d > 1e-6 ? dy / d : -Math.sin(c.dir);
    c.x = p.x + nx * min;
    c.y = p.y + ny * min;
    let vx = Math.cos(c.dir) * c.speed;
    let vy = Math.sin(c.dir) * c.speed;
    const vn = vx * nx + vy * ny;
    if (vn < 0) {
      vx -= (1 + BOUNCE) * vn * nx;
      vy -= (1 + BOUNCE) * vn * ny;
      const hard = -vn / Math.max(1, c.speed);
      const keep = 1 - (1 - BUMP_KEEP) * Math.min(1, hard * 1.5);
      c.speed = Math.hypot(vx, vy) * keep;
      c.dir = Math.atan2(vy, vx);
      if (hard > 0.25) {
        if (c.sinceBump > 0.4) {
          c.bumps++;
          c.events.push("bump");
        }
        c.sinceBump = 0;
        if (c.drift !== 0) {
          c.charge = 0;
          c.level = 0;
        }
      }
    }
  }
}

/** Sticky notes, the start line and paper clips under the car. */
function touch(r: Race, c: Car) {
  if (c.finished !== null || c.dnf) return;
  const t = r.track;
  const cp = t.checkpoints[c.next];
  if (inBox({ x: cp.x, y: cp.y, w: 8, h: t.half * 2 + 6, angle: cp.angle }, c.x, c.y)) {
    c.lastCp = c.next;
    if (c.next === 0) {
      const clock = clockOf(r, c);
      c.laps.push(clock - c.lapStart);
      c.lapStart = clock;
      c.lap++;
      c.taken.fill(false);
      if (c.lap >= LAPS) {
        c.finished = clock;
        c.events.push("finish");
        if (r.deadline === null && r.cars.length > 1) r.deadline = r.t + FINISH_GRACE_S;
      } else {
        c.events.push(c.lap === LAPS - 1 ? "final-lap" : "lap");
      }
    } else {
      c.events.push("checkpoint");
    }
    c.next = (c.next + 1) % t.checkpoints.length;
  }
  if (c.air > 0) return;
  for (let i = 0; i < t.clips.length; i++) {
    if (c.taken[i]) continue;
    const k = t.clips[i];
    if ((k.x - c.x) ** 2 + (k.y - c.y) ** 2 <= (CLIP_R + CAR_R * 0.5) ** 2) {
      c.taken[i] = true;
      c.clips++;
      c.boost = Math.min(CLIP_BOOST_MAX, c.boost + CLIP_BOOST);
      c.events.push("clip");
    }
  }
}

/** Two cars nudge each other apart (a bump, not a crash). */
function carsTouch(a: Car, b: Car) {
  if (a.fall !== null || b.fall !== null || a.air > 0 || b.air > 0) return;
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const d = Math.hypot(dx, dy);
  const min = CAR_R * 2;
  if (d >= min || d < 1e-6) return;
  const nx = dx / d;
  const ny = dy / d;
  const push = (min - d) / 2;
  a.x -= nx * push;
  a.y -= ny * push;
  b.x += nx * push;
  b.y += ny * push;
  // Trade some of the closing speed.
  const av = { x: Math.cos(a.dir) * a.speed, y: Math.sin(a.dir) * a.speed };
  const bv = { x: Math.cos(b.dir) * b.speed, y: Math.sin(b.dir) * b.speed };
  const closing = (av.x - bv.x) * nx + (av.y - bv.y) * ny;
  if (closing <= 0) return;
  const j = closing * 0.5;
  av.x -= j * nx;
  av.y -= j * ny;
  bv.x += j * nx;
  bv.y += j * ny;
  a.speed = Math.hypot(av.x, av.y);
  a.dir = Math.atan2(av.y, av.x);
  b.speed = Math.hypot(bv.x, bv.y);
  b.dir = Math.atan2(bv.y, bv.x);
}

/** One fixed step of the whole race. */
export function step(r: Race, inputs: readonly CarInput[], dt = STEP): Race {
  if (r.over) return r;
  const before = r.t;
  r.t += dt;
  if (r.t < 0) {
    if (Math.ceil(r.t) !== Math.ceil(before)) r.events.push("count");
    return r;
  }
  if (before < 0) {
    r.events.push("go");
    r.t = 0;
    return r;
  }
  r.cars.forEach((c, i) => stepCar(r, c, inputs[i] ?? NO_INPUT, dt));
  if (r.cars.length === 2) carsTouch(r.cars[0], r.cars[1]);
  // Car 1's ghost: a pose every 1/GHOST_HZ seconds.
  const c0 = r.cars[0];
  if (c0.finished === null && Math.floor(r.t * GHOST_HZ) >= r.ghost.length / 3) {
    // `| 0` so a pose never says -0 (JSON would turn it into 0).
    r.ghost.push(Math.round(c0.x * 10) | 0, Math.round(c0.y * 10) | 0, Math.round(wrapAngle(c0.angle) * 100) | 0);
  }
  // Out of time: anyone not finished by the deadline (or the longest a race may run) is out.
  const limit = Math.min(r.deadline ?? Infinity, MAX_S);
  if (r.t >= limit) for (const c of r.cars) if (c.finished === null) c.dnf = true;
  if (r.cars.every((c) => c.finished !== null || c.dnf)) {
    r.over = true;
    r.events.push("over");
  }
  return r;
}

/** Run `dt` seconds of the race in fixed steps, carrying the remainder (a slow frame is capped). */
export function advance(r: Race, inputs: readonly CarInput[], dt: number, carry = { left: 0 }): Race {
  carry.left = Math.min(carry.left + Math.min(Math.max(0, dt), 0.1), 0.1);
  while (carry.left >= STEP - 1e-9 && !r.over) {
    step(r, inputs, STEP);
    carry.left -= STEP;
  }
  return r;
}

/** Seconds of countdown left as the number to show (3, 2, 1), "GO" for a moment after, or null. */
export function countdownLabel(t: number): string | null {
  if (t < 0) return String(Math.ceil(-t - 1e-9));
  if (t < 0.7) return "GO";
  return null;
}

/** Cars in race order (1-based places), leader first. */
export function places(r: Race): number[] {
  const order = r.cars.map((c, i) => ({ i, p: c.dnf ? -Infinity : progress(r, c) })).sort((a, b) => b.p - a.p);
  const place = new Array<number>(r.cars.length);
  order.forEach((o, k) => (place[o.i] = k + 1));
  return place;
}

/** The two-player result: the lower time wins; a car that didn't finish loses; both out is a tie. */
export function derbyWinner(times: readonly [number, number]): 1 | 2 | null {
  const [a, b] = times;
  if (a <= 0 && b <= 0) return null;
  if (a <= 0) return 2;
  if (b <= 0) return 1;
  if (a === b) return null;
  return a < b ? 1 : 2;
}

/** "1:32.45" (minutes, seconds, hundredths) from milliseconds; "–:––.––" for no time. */
export function formatTime(ms: number): string {
  if (!(ms > 0) || !Number.isFinite(ms)) return "–:––.––";
  const cs = Math.round(ms / 10);
  const m = Math.floor(cs / 6000);
  const s = Math.floor((cs % 6000) / 100);
  const c = cs % 100;
  return `${m}:${String(s).padStart(2, "0")}.${String(c).padStart(2, "0")}`;
}

/** "+0.42" / "−1.30": a split against a best, in seconds. */
export function formatSplit(ms: number): string {
  const s = Math.abs(ms) / 1000;
  return `${ms < 0 ? "−" : "+"}${s.toFixed(2)}`;
}

// --- Ghosts ---

/** A saved run to race against: where car 1 was every 1/GHOST_HZ s, its time and its laps. */
export interface Ghost {
  /** The track it was driven on (with its variant: mirror and spill). */
  key: string;
  ms: number;
  laps: number[];
  poses: number[];
}

/** The ghost's key for a track variant (a ghost only rides the exact track it was driven on). */
export function ghostKey(v: Variant): string {
  return `${v.track}${v.mirror ? "~m" : ""}/${v.puddle}/${v.clipSeed}`;
}

/** A finished race's ghost (car 1's), or null if it didn't finish. */
export function ghostOf(r: Race): Ghost | null {
  const c = r.cars[0];
  const ms = raceMs(c);
  if (ms === 0) return null;
  return { key: ghostKey(r.track.variant), ms, laps: c.laps.map((l) => Math.round(l * 1000)), poses: r.ghost.slice() };
}

/** A saved ghost, whatever shape it's in: a valid ghost, or null. */
export function parseGhost(raw: unknown): Ghost | null {
  if (!raw || typeof raw !== "object") return null;
  const g = raw as Partial<Ghost>;
  if (typeof g.key !== "string" || !(typeof g.ms === "number" && g.ms > 0 && Number.isFinite(g.ms))) return null;
  if (!Array.isArray(g.poses) || g.poses.length % 3 !== 0 || g.poses.length > 3 * GHOST_HZ * MAX_S + 3) return null;
  if (!g.poses.every((v) => Number.isInteger(v) && Math.abs(v) < 100_000)) return null;
  const laps = Array.isArray(g.laps) && g.laps.length === LAPS && g.laps.every((l) => typeof l === "number" && l > 0) ? g.laps : [];
  return { key: g.key, ms: Math.round(g.ms), laps, poses: g.poses };
}

/** Where the ghost was `t` seconds after GO (between samples it glides), or null before or after. */
export function ghostPose(g: Pick<Ghost, "poses">, t: number): Pose | null {
  const n = g.poses.length / 3;
  if (n === 0 || t < 0) return null;
  const u = t * GHOST_HZ;
  const i = Math.floor(u);
  if (i >= n - 1) return null;
  const f = u - i;
  const a = i * 3;
  const b = a + 3;
  const da = wrapAngle(g.poses[b + 2] / 100 - g.poses[a + 2] / 100);
  return {
    x: (g.poses[a] + (g.poses[b] - g.poses[a]) * f) / 10,
    y: (g.poses[a + 1] + (g.poses[b + 1] - g.poses[a + 1]) * f) / 10,
    angle: g.poses[a + 2] / 100 + da * f,
  };
}

/** The better of two runs for a ghost: the faster one (a run with no time never replaces one). */
export function betterGhost(old: Ghost | null, run: Ghost | null): Ghost | null {
  if (!run) return old;
  if (!old || old.key !== run.key) return run;
  return run.ms < old.ms ? run : old;
}

// --- Controls ---

/** A key in a two-player race: whose it is and what it does (Player 1 ← → and ↑ or right Shift to
 * drift; Player 2 A D and W or left Shift), or null. `code` tells the Shift keys apart. */
export function duoKey(key: string, code = ""): { player: 1 | 2; action: "left" | "right" | "drift" } | null {
  if (key === "Shift") return code === "ShiftLeft" ? { player: 2, action: "drift" } : { player: 1, action: "drift" };
  switch (key.length === 1 ? key.toLowerCase() : key) {
    case "ArrowLeft":
      return { player: 1, action: "left" };
    case "ArrowRight":
      return { player: 1, action: "right" };
    case "ArrowUp":
      return { player: 1, action: "drift" };
    case "a":
      return { player: 2, action: "left" };
    case "d":
      return { player: 2, action: "right" };
    case "w":
      return { player: 2, action: "drift" };
    default:
      return null;
  }
}

/** One player's input from every control: held keys win, then the phone's slider and button. */
export function inputFrom(c: { left: boolean; right: boolean; drift: boolean; pad: { connected: boolean; steer: number; drift: boolean } | null }): CarInput {
  const keys = (c.right ? 1 : 0) - (c.left ? 1 : 0);
  const pad = c.pad?.connected ? c.pad : null;
  const steer = keys !== 0 ? keys : pad ? clamp(Number.isFinite(pad.steer) ? pad.steer : 0, -1, 1) : 0;
  return { steer, drift: c.drift || (pad?.drift ?? false) };
}

// --- A driver for tests and the preview ---

/**
 * A simple driver: aims at a point a little way up the lane (further at speed), and drifts when the
 * lane ahead bends hard enough. Not quick, but it gets round every track, which the tests rely on.
 */
export function autopilot(r: Race, c: Car, opts: { drift?: boolean; line?: number } = {}): CarInput {
  const t = r.track;
  const here = nearest(t, c.x, c.y);
  const look = 12 + c.speed * 0.32;
  const aim = poseAt(t, here.s + look);
  const side = opts.line ?? 0;
  const ax = aim.x - Math.sin(aim.angle) * side;
  const ay = aim.y + Math.cos(aim.angle) * side;
  const want = Math.atan2(ay - c.y, ax - c.x);
  const err = wrapAngle(want - c.angle);
  const steer = clamp(err * 2.6, -1, 1);
  // How hard the lane bends over the next stretch.
  const far = poseAt(t, here.s + 34);
  const bend = Math.abs(wrapAngle(far.angle - poseAt(t, here.s + 6).angle));
  const turn = wrapAngle(far.angle - poseAt(t, here.s + 6).angle);
  // Into a drift on a hard bend; out of it once the bend eases or the line needs the other way.
  const drift =
    (opts.drift ?? true) && (c.drift !== 0 ? bend > 0.3 && err * c.drift > -0.12 : bend > 0.75 && Math.abs(steer) > 0.5 && turn * steer > 0);
  return { steer, drift };
}
