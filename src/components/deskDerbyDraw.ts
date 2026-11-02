// Draws one frame of Desk Derby onto a canvas: the desk seen from above (wood grain, loose paper,
// the lane of desk mat edged with tape, sticky-note checkpoints, the keyboard, the mug, the mouse,
// pencils, the stapler ramp, a coffee spill and paper clips), skid marks, a ghost of your best run,
// the cars (shadows, a hop in the air off the stapler, a tumble off the edge), drift sparks in the
// boost level's colour, boost flames and dust. tug's palette, read from theme classes by the
// component, so no colour is written here; at night a desk lamp lights the desk. Racing alone, the
// camera follows the car zoomed in and looking ahead; two players, the menus and reduced motion see
// the whole desk. The car is drawn a little larger than it bumps, ringed light-on-dark, so it reads
// at a glance on the mat, the wood and the coffee.
//
// The desk and everything that doesn't move is drawn once into an offscreen layer in desk units (per
// track and palette, sharp at the closest zoom) and copied under each frame. Skid marks, particles
// and the camera are the only state here (`Fx`): how the race looks, never how it plays, so the
// rules stay pure.
//
// Reduced motion: no follow camera, no shake, no particles, no keyboard rattle and no flicker; skid
// marks and the boost flame are still drawn (they say what the car is doing), just steady.

import {
  CAR_L,
  CAR_W,
  DESK_H,
  DESK_W,
  FALL_S,
  KEY,
  RAMP_LEN,
  RAMP_W,
  keyBump,
  wrapAngle,
  type Box,
  type Car,
  type Pose,
  type Race,
  type Track,
} from "../lib/deskDerby";

export interface DerbyPalette {
  floor: string;
  desk: string;
  grain: string;
  deskEdge: string;
  shadow: string;
  lane: string;
  laneLine: string;
  tape: string;
  curb: string;
  paper: string;
  paperLine: string;
  keyboard: string;
  keycap: string;
  keyTop: string;
  mug: string;
  mugRim: string;
  coffee: string;
  coffeeTint: string;
  pencil: string;
  wood: string;
  lead: string;
  eraser: string;
  ferrule: string;
  pen: string;
  penCap: string;
  mouse: string;
  mouseLine: string;
  cable: string;
  stapler: string;
  staplerTop: string;
  noteA: string;
  noteB: string;
  noteC: string;
  noteD: string;
  clip: string;
  clipShine: string;
  car1: string;
  car2: string;
  carDark: string;
  outline: string;
  glass: string;
  stripe: string;
  wheel: string;
  skid: string;
  scuff: string;
  spark1: string;
  spark2: string;
  spark3: string;
  flame: string;
  flameCore: string;
  dust: string;
  lamp: string;
  checkA: string;
  checkB: string;
  headlight: string;
}

export interface DerbyView {
  width: number;
  height: number;
  dpr: number;
  reducedMotion: boolean;
  night: boolean;
  palette: DerbyPalette;
  race: Race;
  fx: Fx;
  /** A ghost of the best run, where it is now (null: none to show). */
  ghost: Pose | null;
  /** Seconds of shake left (0 with reduced motion). */
  shake: number;
  /** The sticky note Player 1 needs next, highlighted (null: none, e.g. two players). */
  showNext: boolean;
}

/** Where the desk sits on screen. */
export interface DeskLayout {
  scale: number;
  left: number;
  top: number;
}

/** Fit the desk in the panel, centred, with a little floor showing round it. */
export function deskLayout(width: number, height: number): DeskLayout {
  const m = Math.min(24, Math.min(width, height) * 0.04);
  const scale = Math.max(0.01, Math.min((width - 2 * m) / DESK_W, (height - 2 * m) / DESK_H));
  return { scale, left: (width - DESK_W * scale) / 2, top: (height - DESK_H * scale) / 2 };
}

function hash(i: number): number {
  let h = Math.imul(i + 1, 0x9e3779b1);
  h = Math.imul(h ^ (h >>> 15), 0x85ebca6b);
  return ((h ^ (h >>> 13)) >>> 0) / 4294967296;
}

// --- Effects: skid marks and particles (how it looks, not how it plays) ---

interface Skid {
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  born: number;
  heavy: boolean;
  /** On the dark mat the marks are pale scuffs; on the wood, rubber. */
  mat: boolean;
}
interface Particle {
  x: number;
  y: number;
  vx: number;
  vy: number;
  life: number;
  max: number;
  size: number;
  colour: keyof DerbyPalette;
}
export interface Fx {
  t: number;
  skids: Skid[];
  particles: Particle[];
  /** Each car's rear wheels last frame (to join skid marks up). */
  wheels: Array<{ l: [number, number]; r: [number, number] } | null>;
  seed: number;
  /** The camera: what it looks at, and how close (1: the whole desk). */
  cam: { x: number; y: number; zoom: number };
}

export function newFx(): Fx {
  return { t: 0, skids: [], particles: [], wheels: [], seed: 1, cam: { x: DESK_W / 2, y: DESK_H / 2, zoom: 1 } };
}

/** How close the follow camera comes, against the whole desk in view. */
export const FOLLOW_ZOOM = 1.75;

/**
 * Ease the camera for this frame. Following (one car, racing, motion allowed): zoomed in on the
 * car, looking ahead along its way. Otherwise the whole desk (two players, the menus, reduced
 * motion), so nothing slides about.
 */
export function updateCamera(fx: Fx, race: Race, dt: number, follow: boolean) {
  const c = race.cars[0];
  const want = follow && c
    ? { x: c.x + Math.cos(c.dir) * c.speed * 0.5, y: c.y + Math.sin(c.dir) * c.speed * 0.5, zoom: FOLLOW_ZOOM }
    : { x: DESK_W / 2, y: DESK_H / 2, zoom: 1 };
  if (dt <= 0) {
    fx.cam = { ...want };
    return;
  }
  const k = 1 - Math.exp(-(follow ? 4 : 3) * dt);
  fx.cam.x += (want.x - fx.cam.x) * k;
  fx.cam.y += (want.y - fx.cam.y) * k;
  fx.cam.zoom += (want.zoom - fx.cam.zoom) * (1 - Math.exp(-2.5 * dt));
}

/** Where the desk sits on screen for the camera: centred on it, never showing more floor than it must. */
export function cameraLayout(width: number, height: number, cam: Fx["cam"]): DeskLayout {
  const base = deskLayout(width, height);
  const scale = base.scale * cam.zoom;
  // Along each axis: centred if the desk fits, else on the camera, with at most a margin of floor.
  const place = (size: number, desk: number, centre: number) => {
    const span = desk * scale;
    if (span <= size) return (size - span) / 2;
    const m = Math.min(24, size * 0.04);
    return Math.min(m, Math.max(size - span - m, size / 2 - centre * scale));
  };
  return { scale, left: place(width, DESK_W, cam.x), top: place(height, DESK_H, cam.y) };
}

const MAX_SKIDS = 1400;
const SKID_FADE_S = 14;
const MAX_PARTICLES = 260;

function rnd(fx: Fx): number {
  fx.seed = (Math.imul(fx.seed, 1664525) + 1013904223) | 0;
  return (fx.seed >>> 0) / 4294967296;
}

function rearWheels(c: Car): { l: [number, number]; r: [number, number] } {
  const cos = Math.cos(c.angle);
  const sin = Math.sin(c.angle);
  const bx = c.x - cos * CAR_L * 0.32;
  const by = c.y - sin * CAR_L * 0.32;
  const w = CAR_W * 0.42;
  return { l: [bx + sin * w, by - cos * w], r: [bx - sin * w, by + cos * w] };
}

function emit(fx: Fx, p: Omit<Particle, "life">) {
  if (fx.particles.length >= MAX_PARTICLES) fx.particles.shift();
  fx.particles.push({ ...p, life: 0 });
}

/** Lay skid marks and throw particles for this frame, from what the cars are doing. */
export function updateFx(fx: Fx, race: Race, dt: number, reducedMotion: boolean) {
  fx.t += dt;
  race.cars.forEach((c, i) => {
    const grounded = c.fall === null && c.air <= 0;
    const slip = Math.abs(Math.sin(wrapAngle(c.angle - c.dir)));
    const w = rearWheels(c);
    const prev = fx.wheels[i];
    const marking = grounded && c.speed > 8 && (c.drift !== 0 || slip > 0.22 || c.surface === "coffee");
    if (marking && prev) {
      const heavy = c.drift !== 0;
      const mat = c.surface !== "wood";
      fx.skids.push({ x1: prev.l[0], y1: prev.l[1], x2: w.l[0], y2: w.l[1], born: fx.t, heavy, mat });
      fx.skids.push({ x1: prev.r[0], y1: prev.r[1], x2: w.r[0], y2: w.r[1], born: fx.t, heavy, mat });
    }
    fx.wheels[i] = grounded ? w : null;
    if (reducedMotion || !grounded || dt <= 0) return;
    const back = c.dir + Math.PI;
    // Drift sparks in the charge level's colour.
    if (c.drift !== 0 && c.level > 0 && rnd(fx) < dt * 70) {
      const from = rnd(fx) < 0.5 ? w.l : w.r;
      const a = back + (rnd(fx) - 0.5) * 1.4;
      const v = 18 + rnd(fx) * 26;
      emit(fx, { x: from[0], y: from[1], vx: Math.cos(a) * v, vy: Math.sin(a) * v, max: 0.18 + rnd(fx) * 0.18, size: 0.5 + rnd(fx) * 0.4, colour: c.level >= 3 ? "spark3" : c.level === 2 ? "spark2" : "spark1" });
    }
    // Dust off the bare wood, or when sliding.
    if ((c.surface === "wood" && c.speed > 12) || (c.drift !== 0 && rnd(fx) < 0.5)) {
      if (rnd(fx) < dt * 30) {
        const from = rnd(fx) < 0.5 ? w.l : w.r;
        const a = back + (rnd(fx) - 0.5) * 1.8;
        emit(fx, { x: from[0], y: from[1], vx: Math.cos(a) * 6, vy: Math.sin(a) * 6, max: 0.5 + rnd(fx) * 0.4, size: 1.2 + rnd(fx) * 1.4, colour: "dust" });
      }
    }
    // Coffee splashes.
    if (c.surface === "coffee" && c.speed > 10 && rnd(fx) < dt * 40) {
      const a = back + (rnd(fx) - 0.5) * 2.4;
      emit(fx, { x: c.x, y: c.y, vx: Math.cos(a) * 14, vy: Math.sin(a) * 14, max: 0.35, size: 0.7 + rnd(fx) * 0.5, colour: "coffee" });
    }
  });
  for (const c of race.cars) {
    if (reducedMotion) break;
    for (const e of c.events) {
      if (e === "land" || e === "bump") {
        for (let k = 0; k < 8; k++) {
          const a = rnd(fx) * Math.PI * 2;
          emit(fx, { x: c.x, y: c.y, vx: Math.cos(a) * 10, vy: Math.sin(a) * 10, max: 0.45, size: 1 + rnd(fx), colour: "dust" });
        }
      } else if (e === "clip") {
        for (let k = 0; k < 7; k++) {
          const a = rnd(fx) * Math.PI * 2;
          emit(fx, { x: c.x, y: c.y, vx: Math.cos(a) * 16, vy: Math.sin(a) * 16, max: 0.35, size: 0.6, colour: "clipShine" });
        }
      } else if (e === "boost") {
        for (let k = 0; k < 10; k++) {
          const a = c.dir + Math.PI + (rnd(fx) - 0.5) * 0.9;
          const v = 20 + rnd(fx) * 20;
          emit(fx, { x: c.x, y: c.y, vx: Math.cos(a) * v, vy: Math.sin(a) * v, max: 0.3, size: 0.8, colour: "flame" });
        }
      }
    }
  }
  for (const p of fx.particles) {
    p.life += dt;
    p.x += p.vx * dt;
    p.y += p.vy * dt;
    const k = Math.exp(-3 * dt);
    p.vx *= k;
    p.vy *= k;
  }
  fx.particles = fx.particles.filter((p) => p.life < p.max);
  if (fx.skids.length > MAX_SKIDS) fx.skids.splice(0, fx.skids.length - MAX_SKIDS);
  while (fx.skids.length && fx.t - fx.skids[0].born > SKID_FADE_S) fx.skids.shift();
}

// --- The still desk, drawn once ---

let layer: { canvas: HTMLCanvasElement; key: string; res: number } | null = null;
/** The still desk is drawn in desk units with this much floor round it (for its shadow). */
const PAD = 8;

function staticKey(view: DerbyView, res: number): string {
  const t = view.race.track;
  return [t.id, t.variant.mirror, t.variant.puddle, t.variant.clipSeed, view.night, view.palette.desk, view.palette.lane, res].join("|");
}

/** The desk and everything on it that never moves, at `res` canvas pixels per desk unit (enough
 * for the closest the camera comes), drawn once per track and copied under each frame. */
function deskLayer(view: DerbyView, res: number): { canvas: HTMLCanvasElement; res: number } {
  const key = staticKey(view, res);
  if (layer && layer.key === key) return layer;
  const c = layer?.canvas ?? document.createElement("canvas");
  c.width = Math.max(1, Math.round((DESK_W + 2 * PAD) * res));
  c.height = Math.max(1, Math.round((DESK_H + 2 * PAD) * res));
  const ctx = c.getContext("2d");
  if (ctx) drawDesk(ctx, view, res);
  layer = { canvas: c, key, res };
  return layer;
}

function drawDesk(ctx: CanvasRenderingContext2D, view: DerbyView, res: number) {
  const p = view.palette;
  const t = view.race.track;
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.clearRect(0, 0, ctx.canvas.width, ctx.canvas.height);
  ctx.setTransform(res, 0, 0, res, PAD * res, PAD * res);

  // The desk: a shadow on the floor, the top, its grain, and a rounded edge.
  ctx.fillStyle = p.shadow;
  roundRect(ctx, 2, 4, DESK_W, DESK_H, 6);
  ctx.fill();
  ctx.fillStyle = p.desk;
  roundRect(ctx, 0, 0, DESK_W, DESK_H, 6);
  ctx.fill();
  ctx.save();
  roundRect(ctx, 0, 0, DESK_W, DESK_H, 6);
  ctx.clip();
  ctx.strokeStyle = p.grain;
  ctx.lineCap = "round";
  for (let i = 0; i < 60; i++) {
    const y0 = (i / 60) * DESK_H + hash(i) * 3;
    ctx.lineWidth = 0.35 + hash(i * 5) * 0.9;
    ctx.globalAlpha = 0.35 + hash(i * 3) * 0.65;
    ctx.beginPath();
    for (let x = -4; x <= DESK_W + 4; x += 8) {
      const y = y0 + Math.sin(x / (40 + hash(i * 7) * 50) + i) * (1.2 + hash(i * 11) * 2.4) + Math.sin(x / 13 + i * 2) * 0.3;
      if (x === -4) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
  ctx.restore();
  ctx.strokeStyle = p.deskEdge;
  ctx.lineWidth = 1.6;
  roundRect(ctx, 0.8, 0.8, DESK_W - 1.6, DESK_H - 1.6, 5.5);
  ctx.stroke();

  // Loose paper under everything.
  for (const b of t.papers) drawPaper(ctx, p, b);

  drawLane(ctx, p, t);
  // The keyboard sits on the lane, with the lane's tape run on across its keys.
  drawKeyboard(ctx, p, t.keyboard);
  ctx.save();
  inFrame(ctx, t.keyboard, () => {
    ctx.beginPath();
    ctx.rect(-t.keyboard.w / 2, -t.keyboard.h / 2, t.keyboard.w, t.keyboard.h);
  });
  ctx.clip();
  lanePath(ctx, t, 0);
  ctx.globalAlpha = 0.3;
  ctx.strokeStyle = p.lane;
  ctx.lineWidth = t.half * 2;
  ctx.stroke();
  ctx.globalAlpha = 1;
  drawLaneDashes(ctx, p, t);
  drawTape(ctx, p, t);
  ctx.restore();
  drawNotes(ctx, p, t);
  drawPuddle(ctx, p, t);
  drawStapler(ctx, p, t.ramp);
  for (const pen of t.pencils) drawPencil(ctx, p, pen);
  drawMouse(ctx, p, t.mouse);
  drawMug(ctx, p, t.mug.x, t.mug.y, t.mug.r);

  // At night a desk lamp lights a pool of the desk.
  if (view.night) {
    const g = ctx.createRadialGradient(DESK_W * 0.3, DESK_H * 0.2, 10, DESK_W * 0.3, DESK_H * 0.2, DESK_W * 0.75);
    g.addColorStop(0, p.lamp);
    g.addColorStop(1, "rgba(0,0,0,0)");
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, DESK_W, DESK_H);
  }
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, r);
}

function inFrame(ctx: CanvasRenderingContext2D, b: Pick<Box, "x" | "y" | "angle">, draw: () => void) {
  ctx.save();
  ctx.translate(b.x, b.y);
  ctx.rotate(b.angle);
  draw();
  ctx.restore();
}

function drawPaper(ctx: CanvasRenderingContext2D, p: DerbyPalette, b: Box) {
  inFrame(ctx, b, () => {
    ctx.fillStyle = p.shadow;
    ctx.fillRect(-b.w / 2 + 1, -b.h / 2 + 1.5, b.w, b.h);
    ctx.fillStyle = p.paper;
    ctx.fillRect(-b.w / 2, -b.h / 2, b.w, b.h);
    ctx.strokeStyle = p.paperLine;
    ctx.lineWidth = 0.4;
    for (let y = -b.h / 2 + 8; y < b.h / 2 - 3; y += 4) {
      ctx.beginPath();
      ctx.moveTo(-b.w / 2 + 4, y);
      ctx.lineTo(b.w / 2 - 4, y);
      ctx.stroke();
    }
  });
}

/** The lane: desk mat between two strips of tape, red and white curbs on the tight bends. */
function drawLane(ctx: CanvasRenderingContext2D, p: DerbyPalette, t: Track) {
  const n = t.xs.length;
  const path = (off: number) => {
    ctx.beginPath();
    for (let i = 0; i <= n; i++) {
      const j = i % n;
      const x = t.xs[j] - t.tys[j] * off;
      const y = t.ys[j] + t.txs[j] * off;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
  };
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  // Its shadow, then the mat.
  ctx.save();
  ctx.translate(0.6, 1);
  path(0);
  ctx.strokeStyle = p.shadow;
  ctx.lineWidth = t.half * 2 + 3;
  ctx.stroke();
  ctx.restore();
  path(0);
  ctx.strokeStyle = p.lane;
  ctx.lineWidth = t.half * 2;
  ctx.stroke();
  // A faint dashed centre line.
  ctx.setLineDash([3, 5]);
  ctx.strokeStyle = p.laneLine;
  ctx.lineWidth = 0.5;
  path(0);
  ctx.stroke();
  ctx.setLineDash([]);
  drawTape(ctx, p, t);
  // Curbs on the outside of the tight bends.
  for (let i = 0; i < n; i += 2) {
    const a = Math.atan2(t.tys[(i + 3) % n], t.txs[(i + 3) % n]);
    const b = Math.atan2(t.tys[(i - 3 + n) % n], t.txs[(i - 3 + n) % n]);
    const bend = wrapAngle(a - b) / 9;
    if (Math.abs(bend) < 0.045) continue;
    // Outside of the bend: turning right (positive) has its outside on the left.
    const side = bend > 0 ? -1 : 1;
    const off = side * (t.half - 0.6);
    const j = (i + 2) % n;
    ctx.strokeStyle = Math.floor(i / 4) % 2 === 0 ? p.curb : p.tape;
    ctx.lineWidth = 1.8;
    ctx.beginPath();
    ctx.moveTo(t.xs[i] - t.tys[i] * off, t.ys[i] + t.txs[i] * off);
    ctx.lineTo(t.xs[j] - t.tys[j] * off, t.ys[j] + t.txs[j] * off);
    ctx.stroke();
  }
}

function lanePath(ctx: CanvasRenderingContext2D, t: Track, off: number) {
  const n = t.xs.length;
  ctx.beginPath();
  for (let i = 0; i <= n; i++) {
    const j = i % n;
    const x = t.xs[j] - t.tys[j] * off;
    const y = t.ys[j] + t.txs[j] * off;
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
}

/** The tape along each edge of the lane. */
function drawTape(ctx: CanvasRenderingContext2D, p: DerbyPalette, t: Track) {
  ctx.lineJoin = "round";
  for (const side of [-1, 1]) {
    lanePath(ctx, t, side * (t.half - 0.6));
    ctx.strokeStyle = p.tape;
    ctx.lineWidth = 1.2;
    ctx.stroke();
  }
}

/** The lane's faint dashed centre line. */
function drawLaneDashes(ctx: CanvasRenderingContext2D, p: DerbyPalette, t: Track) {
  ctx.save();
  lanePath(ctx, t, 0);
  ctx.strokeStyle = p.laneLine;
  ctx.lineWidth = 0.5;
  ctx.setLineDash([3, 5]);
  ctx.stroke();
  ctx.restore();
}

/** The sticky notes (checkpoints), and the start line's chequered tape. */
function drawNotes(ctx: CanvasRenderingContext2D, p: DerbyPalette, t: Track) {
  const colours = [p.noteA, p.noteB, p.noteC, p.noteD];
  t.checkpoints.forEach((cp, i) => {
    if (i === 0) {
      inFrame(ctx, cp, () => {
        const h = t.half * 2;
        const cells = 8;
        const s = h / cells;
        for (let r = 0; r < 2; r++) {
          for (let k = 0; k < cells; k++) {
            ctx.fillStyle = (r + k) % 2 === 0 ? p.checkA : p.checkB;
            ctx.fillRect(-s + r * s, -h / 2 + k * s, s, s);
          }
        }
      });
      return;
    }
    const size = t.half * 2 + 3;
    inFrame(ctx, { x: cp.x, y: cp.y, angle: cp.angle + (hash(i * 31) - 0.5) * 0.12 }, () => {
      ctx.fillStyle = p.shadow;
      ctx.fillRect(-size / 2 + 0.8, -size / 2 + 1.2, size, size);
      ctx.fillStyle = colours[(i - 1) % colours.length];
      ctx.beginPath();
      ctx.moveTo(-size / 2, -size / 2);
      ctx.lineTo(size / 2, -size / 2);
      ctx.lineTo(size / 2, size / 2 - 3.5);
      ctx.lineTo(size / 2 - 3.5, size / 2);
      ctx.lineTo(-size / 2, size / 2);
      ctx.closePath();
      ctx.fill();
      // The curled corner.
      ctx.fillStyle = p.shadow;
      ctx.beginPath();
      ctx.moveTo(size / 2, size / 2 - 3.5);
      ctx.lineTo(size / 2 - 3.5, size / 2 - 3.5);
      ctx.lineTo(size / 2 - 3.5, size / 2);
      ctx.closePath();
      ctx.fill();
      // A number, so the order reads at a glance.
      ctx.fillStyle = p.carDark;
      ctx.globalAlpha = 0.55;
      ctx.font = `600 ${size * 0.42}px Inter, system-ui, sans-serif`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.rotate(-cp.angle);
      ctx.fillText(String(i), 0, 0.5);
      ctx.globalAlpha = 1;
    });
  });
}

function drawKeyboard(ctx: CanvasRenderingContext2D, p: DerbyPalette, b: Box) {
  inFrame(ctx, b, () => {
    ctx.fillStyle = p.shadow;
    roundRect(ctx, -b.w / 2 + 1.2, -b.h / 2 + 2, b.w, b.h, 3);
    ctx.fill();
    ctx.fillStyle = p.keyboard;
    roundRect(ctx, -b.w / 2, -b.h / 2, b.w, b.h, 3);
    ctx.fill();
    const cols = Math.floor((b.w - 4) / KEY);
    const rows = Math.floor((b.h - 4) / KEY);
    const x0 = -(cols * KEY) / 2;
    const y0 = -(rows * KEY) / 2;
    for (let r = 0; r < rows; r++) {
      const last = r === rows - 1;
      for (let c = 0; c < cols; c++) {
        // The space bar along the bottom row.
        if (last && c > 3 && c < cols - 4) {
          if (c === 4) drawKey(ctx, p, x0 + c * KEY, y0 + r * KEY, KEY * (cols - 8), KEY);
          continue;
        }
        drawKey(ctx, p, x0 + c * KEY, y0 + r * KEY, KEY, KEY);
      }
    }
  });
}

function drawKey(ctx: CanvasRenderingContext2D, p: DerbyPalette, x: number, y: number, w: number, h: number) {
  ctx.fillStyle = p.keycap;
  roundRect(ctx, x + 0.5, y + 0.5, w - 1, h - 1, 1.2);
  ctx.fill();
  ctx.fillStyle = p.keyTop;
  roundRect(ctx, x + 1.3, y + 1, w - 2.6, h - 2.6, 1);
  ctx.fill();
}

function drawPuddle(ctx: CanvasRenderingContext2D, p: DerbyPalette, t: Track) {
  const b = t.puddle;
  inFrame(ctx, b, () => {
    const blob = (grow: number) => {
      ctx.beginPath();
      for (let k = 0; k <= 40; k++) {
        const a = (k / 40) * Math.PI * 2;
        const wob = 1 + 0.07 * Math.sin(a * 3 + 1) + 0.04 * Math.sin(a * 5 + 2);
        const x = Math.cos(a) * (b.w / 2 + grow) * wob;
        const y = Math.sin(a) * (b.h / 2 + grow) * wob;
        if (k === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      }
      ctx.closePath();
    };
    // The spill: see-through, darker where it's pooling at the rim, and a shine on top.
    blob(0);
    ctx.fillStyle = p.coffee;
    ctx.globalAlpha = 0.5;
    ctx.fill();
    ctx.fillStyle = p.coffeeTint;
    ctx.globalAlpha = 1;
    ctx.fill();
    ctx.strokeStyle = p.coffee;
    ctx.globalAlpha = 0.35;
    ctx.lineWidth = 0.6;
    ctx.stroke();
    ctx.strokeStyle = p.clipShine;
    ctx.lineCap = "round";
    ctx.globalAlpha = 0.55;
    ctx.lineWidth = 0.9;
    ctx.beginPath();
    ctx.ellipse(-b.w * 0.05, -b.h * 0.05, b.w * 0.3, b.h * 0.26, 0, Math.PI * 1.05, Math.PI * 1.45);
    ctx.stroke();
    ctx.lineWidth = 0.6;
    ctx.beginPath();
    ctx.ellipse(b.w * 0.08, b.h * 0.06, b.w * 0.18, b.h * 0.14, 0, Math.PI * 0.1, Math.PI * 0.4);
    ctx.stroke();
    ctx.globalAlpha = 1;
  });
}

/** The stapler: its base, the top plate rising towards the way to drive, and arrows. */
function drawStapler(ctx: CanvasRenderingContext2D, p: DerbyPalette, ramp: Pose) {
  inFrame(ctx, ramp, () => {
    const L = RAMP_LEN;
    const W = RAMP_W;
    ctx.fillStyle = p.shadow;
    roundRect(ctx, -L / 2 + 2.5, -W / 2 + 2, L + 2, W, 2);
    ctx.fill();
    ctx.fillStyle = p.stapler;
    roundRect(ctx, -L / 2, -W / 2, L + 3, W, 2);
    ctx.fill();
    const g = ctx.createLinearGradient(-L / 2, 0, L / 2, 0);
    g.addColorStop(0, p.stapler);
    g.addColorStop(1, p.staplerTop);
    ctx.fillStyle = g;
    roundRect(ctx, -L / 2 + 1, -W / 2 + 1.2, L, W - 2.4, 1.6);
    ctx.fill();
    // The hinge at the low end.
    ctx.fillStyle = p.carDark;
    ctx.fillRect(-L / 2 - 0.4, -W / 2 + 1.5, 1.4, W - 3);
    ctx.strokeStyle = p.tape;
    ctx.globalAlpha = 0.85;
    ctx.lineWidth = 0.9;
    for (const x of [-3.5, 0.5, 4.5]) {
      ctx.beginPath();
      ctx.moveTo(x - 1.8, -2.4);
      ctx.lineTo(x, 0);
      ctx.lineTo(x - 1.8, 2.4);
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
  });
}

function drawPencil(ctx: CanvasRenderingContext2D, p: DerbyPalette, s: { x1: number; y1: number; x2: number; y2: number; kind: "pencil" | "pen" }) {
  const len = Math.hypot(s.x2 - s.x1, s.y2 - s.y1);
  const angle = Math.atan2(s.y2 - s.y1, s.x2 - s.x1);
  inFrame(ctx, { x: s.x1, y: s.y1, angle }, () => {
    if (s.kind === "pencil") {
      const r = 1.6;
      ctx.fillStyle = p.shadow;
      ctx.fillRect(0.6, -r + 1, len, r * 2);
      ctx.fillStyle = p.eraser;
      roundRect(ctx, -1, -r, 4, r * 2, 1);
      ctx.fill();
      ctx.fillStyle = p.ferrule;
      ctx.fillRect(2.5, -r, 2.4, r * 2);
      ctx.fillStyle = p.pencil;
      ctx.fillRect(4.9, -r, len - 10, r * 2);
      ctx.strokeStyle = p.shadow;
      ctx.lineWidth = 0.3;
      ctx.beginPath();
      ctx.moveTo(4.9, -r / 3);
      ctx.lineTo(len - 5.1, -r / 3);
      ctx.moveTo(4.9, r / 3);
      ctx.lineTo(len - 5.1, r / 3);
      ctx.stroke();
      ctx.fillStyle = p.wood;
      ctx.beginPath();
      ctx.moveTo(len - 5.1, -r);
      ctx.lineTo(len, 0);
      ctx.lineTo(len - 5.1, r);
      ctx.closePath();
      ctx.fill();
      ctx.fillStyle = p.lead;
      ctx.beginPath();
      ctx.moveTo(len - 1.8, -0.6);
      ctx.lineTo(len, 0);
      ctx.lineTo(len - 1.8, 0.6);
      ctx.closePath();
      ctx.fill();
    } else {
      const r = 2;
      ctx.fillStyle = p.shadow;
      roundRect(ctx, 0.6, -r + 1.2, len, r * 2, r);
      ctx.fill();
      ctx.fillStyle = p.pen;
      roundRect(ctx, 0, -r, len, r * 2, r);
      ctx.fill();
      ctx.fillStyle = p.penCap;
      roundRect(ctx, 0, -r, len * 0.3, r * 2, r);
      ctx.fill();
      ctx.fillRect(len * 0.05, -0.5, len * 0.22, 1);
      ctx.fillStyle = p.lead;
      ctx.beginPath();
      ctx.arc(len, 0, 0.7, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = p.clipShine;
      ctx.globalAlpha = 0.35;
      ctx.fillRect(len * 0.32, -r + 0.5, len * 0.62, 0.6);
      ctx.globalAlpha = 1;
    }
  });
}

function drawMouse(ctx: CanvasRenderingContext2D, p: DerbyPalette, m: Pose) {
  // The cable wanders off towards the back of the desk.
  ctx.strokeStyle = p.cable;
  ctx.lineWidth = 0.9;
  ctx.lineCap = "round";
  const fx = m.x + Math.cos(m.angle) * 10;
  const fy = m.y + Math.sin(m.angle) * 10;
  ctx.beginPath();
  ctx.moveTo(fx, fy);
  ctx.bezierCurveTo(fx + Math.cos(m.angle) * 18, fy + Math.sin(m.angle) * 18, fx + 20, fy - 30, fx + 6, Math.max(2, fy - 60));
  ctx.stroke();
  inFrame(ctx, m, () => {
    ctx.fillStyle = p.shadow;
    ctx.beginPath();
    ctx.ellipse(1, 1.5, 10.5, 6.5, 0, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = p.mouse;
    ctx.beginPath();
    ctx.ellipse(0, 0, 10.5, 6.5, 0, 0, Math.PI * 2);
    ctx.fill();
    ctx.strokeStyle = p.mouseLine;
    ctx.lineWidth = 0.5;
    ctx.beginPath();
    ctx.moveTo(10, 0);
    ctx.lineTo(3, 0);
    ctx.moveTo(3, -5.6);
    ctx.quadraticCurveTo(1.5, 0, 3, 5.6);
    ctx.stroke();
    ctx.fillStyle = p.mouseLine;
    roundRect(ctx, 5, -0.9, 3, 1.8, 0.9);
    ctx.fill();
  });
}

function drawMug(ctx: CanvasRenderingContext2D, p: DerbyPalette, x: number, y: number, r: number) {
  ctx.fillStyle = p.shadow;
  ctx.beginPath();
  ctx.arc(x + 1.8, y + 2.6, r + 0.5, 0, Math.PI * 2);
  ctx.fill();
  // The handle.
  ctx.strokeStyle = p.mug;
  ctx.lineWidth = 2.6;
  ctx.beginPath();
  ctx.arc(x + r + 1.5, y + 1, 3.4, -Math.PI / 2, Math.PI / 2);
  ctx.stroke();
  ctx.fillStyle = p.mug;
  ctx.beginPath();
  ctx.arc(x, y, r, 0, Math.PI * 2);
  ctx.fill();
  ctx.strokeStyle = p.mugRim;
  ctx.lineWidth = 0.8;
  ctx.stroke();
  ctx.fillStyle = p.coffee;
  ctx.beginPath();
  ctx.arc(x, y, r - 1.8, 0, Math.PI * 2);
  ctx.fill();
  ctx.fillStyle = p.coffeeTint;
  ctx.fill();
  ctx.strokeStyle = p.clipShine;
  ctx.globalAlpha = 0.45;
  ctx.lineWidth = 0.8;
  ctx.beginPath();
  ctx.arc(x - 1, y - 1, r - 4, Math.PI * 1.05, Math.PI * 1.45);
  ctx.stroke();
  ctx.globalAlpha = 1;
}

function drawClip(ctx: CanvasRenderingContext2D, p: DerbyPalette, x: number, y: number, angle: number, glint: number) {
  inFrame(ctx, { x, y, angle }, () => {
    ctx.scale(1.35, 1.35);
    ctx.strokeStyle = p.shadow;
    ctx.lineWidth = 0.8;
    ctx.beginPath();
    ctx.roundRect(-3.4, -1.0, 7, 2.6, 1.3);
    ctx.stroke();
    ctx.strokeStyle = p.clip;
    ctx.lineWidth = 0.7;
    ctx.beginPath();
    ctx.roundRect(-3.6, -1.4, 7, 2.8, 1.4);
    ctx.stroke();
    ctx.beginPath();
    ctx.moveTo(2.2, -0.6);
    ctx.lineTo(-2.2, -0.6);
    ctx.arc(-2.2, 0.1, 0.7, -Math.PI / 2, Math.PI / 2, true);
    ctx.lineTo(1.5, 0.8);
    ctx.stroke();
    if (glint > 0) {
      ctx.fillStyle = p.clipShine;
      ctx.globalAlpha = glint;
      ctx.beginPath();
      ctx.arc(1.5, -1.2, 0.9, 0, Math.PI * 2);
      ctx.fill();
      ctx.globalAlpha = 1;
    }
  });
}

// --- Each frame ---

export function drawDerby(ctx: CanvasRenderingContext2D, view: DerbyView) {
  const l = cameraLayout(view.width, view.height, view.fx.cam);
  const p = view.palette;
  const r = view.race;
  const t = r.track;
  const rm = view.reducedMotion;
  const time = view.fx.t;
  let ox = l.left;
  let oy = l.top;
  if (view.shake > 0 && !rm) {
    const k = Math.min(1, view.shake / 0.3) * 2.5;
    ox += Math.sin(time * 91) * k;
    oy += Math.cos(time * 77) * k;
  }
  // The floor, the still desk (sharp at the closest zoom), then everything that moves on it.
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.fillStyle = p.floor;
  ctx.fillRect(0, 0, ctx.canvas.width, ctx.canvas.height);
  const res = Math.min(12, Math.ceil(deskLayout(view.width, view.height).scale * FOLLOW_ZOOM * view.dpr));
  const still = deskLayer(view, res);
  ctx.setTransform(view.dpr * l.scale, 0, 0, view.dpr * l.scale, view.dpr * ox, view.dpr * oy);
  ctx.imageSmoothingEnabled = true;
  ctx.drawImage(still.canvas, -PAD, -PAD, DESK_W + 2 * PAD, DESK_H + 2 * PAD);

  // The sticky note Player 1 needs next: a soft pulse round it.
  if (view.showNext && r.t >= 0) {
    const c = r.cars[0];
    if (c.finished === null && !c.dnf) {
      const cp = t.checkpoints[c.next];
      const pulse = rm ? 0.5 : 0.5 + 0.5 * Math.sin(time * 5);
      inFrame(ctx, cp, () => {
        ctx.strokeStyle = p.tape;
        ctx.globalAlpha = 0.35 + 0.4 * pulse;
        ctx.lineWidth = 1;
        const size = t.half * 2 + 6 + pulse * 2;
        ctx.strokeRect(-size / 2, -size / 2, size, size);
        ctx.globalAlpha = 1;
      });
    }
  }

  // Paper clips not yet picked up (by anyone, this lap).
  t.clips.forEach((k, i) => {
    if (r.cars.every((c) => c.taken[i])) return;
    const glint = rm ? 0 : Math.max(0, Math.sin(time * 3 + i * 1.7)) ** 8;
    drawClip(ctx, p, k.x, k.y, hash(i * 9) * Math.PI, glint);
  });

  // Skid marks, fading over time.
  ctx.lineCap = "round";
  for (const s of view.fx.skids) {
    const age = view.fx.t - s.born;
    ctx.strokeStyle = s.mat ? p.scuff : p.skid;
    ctx.globalAlpha = Math.max(0, 1 - age / SKID_FADE_S) * (s.heavy ? 1 : 0.55);
    ctx.lineWidth = s.heavy ? 1.1 : 0.8;
    ctx.beginPath();
    ctx.moveTo(s.x1, s.y1);
    ctx.lineTo(s.x2, s.y2);
    ctx.stroke();
  }
  ctx.globalAlpha = 1;

  // The ghost of the best run.
  if (view.ghost) {
    ctx.globalAlpha = 0.38;
    drawCar(ctx, p, { pose: view.ghost, colour: p.tape, steer: 0, z: 0, ghost: true, night: false });
    ctx.globalAlpha = 1;
  }

  // Particles under the cars: dust and splashes; sparks and flames over them.
  drawParticles(ctx, view, (c) => c === "dust" || c === "coffee");

  r.cars.forEach((c, i) => drawRacer(ctx, view, c, i));

  drawParticles(ctx, view, (c) => c !== "dust" && c !== "coffee");
  ctx.setTransform(1, 0, 0, 1, 0, 0);
}

function drawParticles(ctx: CanvasRenderingContext2D, view: DerbyView, which: (c: keyof DerbyPalette) => boolean) {
  for (const q of view.fx.particles) {
    if (!which(q.colour)) continue;
    const k = 1 - q.life / q.max;
    ctx.globalAlpha = Math.max(0, k) * (q.colour === "dust" ? 0.6 : 1);
    ctx.fillStyle = view.palette[q.colour];
    ctx.beginPath();
    ctx.arc(q.x, q.y, q.size * (q.colour === "dust" ? 1 + (1 - k) : k + 0.3), 0, Math.PI * 2);
    ctx.fill();
  }
  ctx.globalAlpha = 1;
}

function drawRacer(ctx: CanvasRenderingContext2D, view: DerbyView, c: Car, i: number) {
  const p = view.palette;
  const rm = view.reducedMotion;
  const colour = i === 0 ? p.car1 : p.car2;
  let z = 0;
  if (c.air > 0 && c.airTotal > 0) {
    const u = 1 - c.air / c.airTotal;
    z = Math.sin(Math.PI * u);
  }
  // The keys rattle the car a little.
  let jx = 0;
  let jy = 0;
  if (!rm && c.surface === "keys" && c.air <= 0 && c.fall === null) {
    const b = keyBump(view.race.track, c.x, c.y);
    jx = (b - 0.5) * 0.5 * Math.sin(view.fx.t * 60);
    jy = (b - 0.5) * 0.5 * Math.cos(view.fx.t * 53);
  }
  const pose = { x: c.x + jx, y: c.y + jy, angle: c.angle };
  if (c.fall !== null) {
    // Over the edge: tumbling, shrinking, fading.
    const u = Math.min(1, c.fall / FALL_S);
    ctx.save();
    ctx.translate(c.x, c.y);
    ctx.rotate(rm ? 0 : u * 5);
    ctx.scale(1 - 0.7 * u, 1 - 0.7 * u);
    ctx.translate(-c.x, -c.y);
    ctx.globalAlpha = 1 - u;
    drawCar(ctx, p, { pose, colour, steer: c.steer, z: 0, ghost: false, night: view.night });
    ctx.restore();
    ctx.globalAlpha = 1;
    return;
  }
  // Boost: a flame out of the back.
  if (c.boost > 0) {
    const flick = rm ? 1 : 0.8 + 0.4 * Math.abs(Math.sin(view.fx.t * 40 + i));
    inFrame(ctx, pose, () => {
      ctx.scale(LOOK, LOOK);
      const len = (4 + Math.min(1, c.boost) * 5) * flick;
      ctx.fillStyle = p.flame;
      ctx.beginPath();
      ctx.moveTo(-CAR_L / 2 + 0.5, -1.6);
      ctx.quadraticCurveTo(-CAR_L / 2 - len * 0.6, -1.4, -CAR_L / 2 - len, 0);
      ctx.quadraticCurveTo(-CAR_L / 2 - len * 0.6, 1.4, -CAR_L / 2 + 0.5, 1.6);
      ctx.fill();
      ctx.fillStyle = p.flameCore;
      ctx.beginPath();
      ctx.moveTo(-CAR_L / 2 + 0.5, -0.8);
      ctx.quadraticCurveTo(-CAR_L / 2 - len * 0.35, -0.6, -CAR_L / 2 - len * 0.55, 0);
      ctx.quadraticCurveTo(-CAR_L / 2 - len * 0.35, 0.6, -CAR_L / 2 + 0.5, 0.8);
      ctx.fill();
    });
  }
  // Just back after a fall: blinking.
  if (c.grace > 0 && !rm) ctx.globalAlpha = Math.sin(view.fx.t * 30) > 0 ? 0.45 : 0.9;
  drawCar(ctx, p, { pose, colour, steer: c.steer, z, ghost: false, night: view.night });
  ctx.globalAlpha = 1;
  // A drift's charge as a ring of the level's colour under the rear.
  if (c.drift !== 0 && c.level > 0 && rm) {
    ctx.fillStyle = c.level >= 3 ? p.spark3 : c.level === 2 ? p.spark2 : p.spark1;
    const w = rearWheels(c);
    for (const q of [w.l, w.r]) {
      ctx.beginPath();
      ctx.arc(q[0], q[1], 1, 0, Math.PI * 2);
      ctx.fill();
    }
  }
}

/** The car is drawn a little larger than it bumps, so it reads at a glance. */
const LOOK = 1.3;

function drawCar(ctx: CanvasRenderingContext2D, p: DerbyPalette, o: { pose: Pose; colour: string; steer: number; z: number; ghost: boolean; night: boolean }) {
  const { pose, z } = o;
  const L = CAR_L * LOOK;
  const W = CAR_W * LOOK;
  // Headlights at night.
  if (o.night && !o.ghost) {
    inFrame(ctx, pose, () => {
      const g = ctx.createRadialGradient(L / 2, 0, 1, L / 2 + 14, 0, 16);
      g.addColorStop(0, p.headlight);
      g.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.moveTo(L / 2, -1.5);
      ctx.lineTo(L / 2 + 20, -8);
      ctx.lineTo(L / 2 + 20, 8);
      ctx.lineTo(L / 2, 1.5);
      ctx.fill();
    });
  }
  // The shadow: further away and softer in the air.
  if (!o.ghost) {
    inFrame(ctx, { x: pose.x + 0.8 + z * 5, y: pose.y + 1.2 + z * 7, angle: pose.angle }, () => {
      ctx.fillStyle = p.shadow;
      ctx.globalAlpha *= 1 - z * 0.45;
      roundRect(ctx, -L / 2, -W / 2, L, W, 1.6);
      ctx.fill();
    });
  }
  const s = 1 + z * 0.38;
  ctx.save();
  ctx.translate(pose.x, pose.y);
  ctx.rotate(pose.angle);
  ctx.scale(s, s);
  // Wheels (the front ones turn with the steering).
  ctx.fillStyle = p.wheel;
  for (const [wx, wy, front] of [
    [L * 0.3, -W / 2, true],
    [L * 0.3, W / 2, true],
    [-L * 0.3, -W / 2, false],
    [-L * 0.3, W / 2, false],
  ] as const) {
    ctx.save();
    ctx.translate(wx, wy);
    if (front) ctx.rotate(o.steer * 0.45);
    ctx.fillRect(-1.6, -1, 3.2, 2);
    ctx.restore();
  }
  // Body, ringed light-on-dark so it stands out on the mat, the wood and the coffee alike.
  ctx.beginPath();
  ctx.roundRect(-L / 2, -W / 2 + 0.5, L, W - 1, [1.4, 2.8, 2.8, 1.4]);
  if (!o.ghost) {
    ctx.strokeStyle = p.carDark;
    ctx.lineWidth = 2.2;
    ctx.stroke();
    ctx.strokeStyle = p.outline;
    ctx.lineWidth = 1.1;
    ctx.stroke();
  }
  ctx.fillStyle = o.colour;
  ctx.fill();
  if (o.ghost) {
    ctx.strokeStyle = p.carDark;
    ctx.lineWidth = 0.4;
    ctx.stroke();
    ctx.restore();
    return;
  }
  // A stripe, the cockpit glass and the spoiler.
  ctx.fillStyle = p.stripe;
  ctx.fillRect(-L / 2 + 0.8, -0.45, L - 1.6, 0.9);
  ctx.fillStyle = p.glass;
  ctx.beginPath();
  ctx.roundRect(-0.8, -W / 2 + 1.3, 3.4, W - 2.6, 1);
  ctx.fill();
  ctx.fillStyle = p.carDark;
  ctx.fillRect(-L / 2 - 0.5, -W / 2 + 0.2, 1.5, W - 0.4);
  ctx.restore();
}
