// Draws one frame of the weather card's harbor (lib/weatherHarbor): the sky with the sun or the
// moon (in its real phase) where they really are, stars on a clear night, clouds drifting with
// the wind, the far shore with its lighthouse, the water in waves from the Beaufort force, and
// tug's own tugboat riding them with its pennant and smoke going the way the real wind blows.
// Rain, snow, fog and a storm's lightning on top. tug's palette, read from theme classes by the
// component, so no colour is written here. Pure drawing: no state, no timers.
//
// The harbor faces the equator, as you would to watch the sun cross the sky: in the north east is
// on the left (the sun rises left and sets right), in the south it's the other way round, and the
// moon's lit side turns with it.

import type { HarborScene } from "../lib/weatherHarbor";

export interface HarborPalette {
  base: string;
  skyTop: string;
  skyLow: string;
  glow: string;
  glowFade: string;
  overcast: string;
  sun: string;
  halo: string;
  haloFade: string;
  moon: string;
  moonDark: string;
  star: string;
  shoreFar: string;
  shore: string;
  lit: string;
  cloud: string;
  cloudDark: string;
  fog: string;
  water: string;
  waterDeep: string;
  foam: string;
  hull: string;
  hullDark: string;
  cabin: string;
  window: string;
  funnel: string;
  smoke: string;
  flag: string;
  rain: string;
  flake: string;
  flash: string;
  beam: string;
  lamp: string;
}

export interface HarborView {
  width: number;
  height: number;
  dpr: number;
  /** Seconds, for everything that moves (one fixed moment with reduced motion). */
  t: number;
  scene: HarborScene;
  palette: HarborPalette;
  /** South of the equator: east on the right, the moon lit the other way. */
  south: boolean;
  reducedMotion: boolean;
}

const TAU = Math.PI * 2;

/** The same "random" number for the same i, every frame. */
function rnd(i: number): number {
  const x = Math.sin(i * 127.1 + 311.7) * 43758.5453;
  return x - Math.floor(x);
}

const wrap = (v: number, span: number) => ((v % span) + span) % span;

interface Frame {
  ctx: CanvasRenderingContext2D;
  W: number;
  H: number;
  t: number;
  horizon: number;
  /** Screen x direction of east: −1 left (north of the equator), 1 right. */
  east: number;
  /** The wind's push across the screen, −1 left to 1 right. */
  windX: number;
  force: number;
  p: HarborPalette;
  s: HarborScene;
  still: boolean;
}

export function drawHarbor(ctx: CanvasRenderingContext2D, v: HarborView) {
  const { width: W, height: H, scene: s, palette: p } = v;
  ctx.setTransform(v.dpr, 0, 0, v.dpr, 0, 0);
  ctx.globalAlpha = 1;
  const east = v.south ? 1 : -1;
  const fr: Frame = {
    ctx,
    W,
    H,
    t: v.t,
    horizon: Math.round(H * 0.62),
    east,
    windX: s.wind.push * east,
    force: s.wind.force,
    p,
    s,
    still: v.reducedMotion,
  };
  sky(fr);
  stars(fr);
  sunAndMoon(fr);
  clouds(fr);
  shore(fr);
  water(fr);
  fog(fr, 0);
  tug(fr);
  front(fr);
  fog(fr, 1);
  rain(fr);
  snow(fr);
  lightning(fr);
  ctx.globalAlpha = 1;
}

function sky(f: Frame) {
  const { ctx, W, H, horizon, p, s } = f;
  ctx.fillStyle = p.base;
  ctx.fillRect(0, 0, W, H);
  const g = ctx.createLinearGradient(0, 0, 0, horizon);
  g.addColorStop(0, p.skyTop);
  g.addColorStop(1, p.skyLow);
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, W, horizon);
  // Sunrise and sunset warm the sky on their own side.
  if (s.phase === "dawn" || s.phase === "dusk") {
    const side = s.phase === "dawn" ? f.east : -f.east;
    const x = side < 0 ? W * 0.12 : W * 0.88;
    const glow = ctx.createRadialGradient(x, horizon, 0, x, horizon, W * 0.55);
    glow.addColorStop(0, p.glow);
    glow.addColorStop(1, p.glowFade);
    ctx.fillStyle = glow;
    ctx.fillRect(0, 0, W, horizon);
  }
  // A grey sky as the cloud closes in.
  if (s.clouds > 0.55) {
    ctx.globalAlpha = Math.min(1, (s.clouds - 0.55) * 2.2);
    ctx.fillStyle = s.storm ? p.cloudDark : p.overcast;
    ctx.fillRect(0, 0, W, horizon);
    ctx.globalAlpha = 1;
  }
}

function stars(f: Frame) {
  const { ctx, W, horizon, t, p, s } = f;
  if (s.sun || s.clouds > 0.85 || s.fog) return;
  const n = Math.round((1 - s.clouds) * 40 * (W / 600));
  ctx.fillStyle = p.star;
  for (let i = 0; i < n; i++) {
    const twinkle = f.still ? 0.8 : 0.55 + 0.45 * Math.sin(t * (0.8 + rnd(i + 40)) + i * 1.7);
    ctx.globalAlpha = twinkle * (s.phase === "night" ? 1 : 0.4);
    const r = 0.5 + rnd(i + 20) * 0.8;
    ctx.beginPath();
    ctx.arc(rnd(i) * W, 3 + rnd(i + 10) * horizon * 0.8, r, 0, TAU);
    ctx.fill();
  }
  ctx.globalAlpha = 1;
}

/** Where along the sky an arc position (0 rising, 1 setting) and height land. */
function skyPoint(f: Frame, at: number, height: number): [number, number] {
  const margin = f.W * 0.08;
  const along = f.east < 0 ? at : 1 - at; // rises in the east
  const low = f.horizon - 4;
  const top = f.H * 0.2;
  return [margin + along * (f.W - 2 * margin), low - height * (low - top)];
}

function halo(f: Frame, x: number, y: number, r: number, color: string, fade: string) {
  const g = f.ctx.createRadialGradient(x, y, r * 0.6, x, y, r * 3.2);
  g.addColorStop(0, color);
  g.addColorStop(1, fade);
  f.ctx.fillStyle = g;
  f.ctx.fillRect(x - r * 3.2, y - r * 3.2, r * 6.4, r * 6.4);
}

function sunAndMoon(f: Frame) {
  const { ctx, H, p, s } = f;
  const veil = 1 - Math.min(0.85, s.clouds * 0.75 + (s.fog ? 0.3 : 0));
  if (s.sun) {
    const [x, y] = skyPoint(f, s.sun.at, s.sun.height);
    const r = Math.max(6, H * 0.1);
    ctx.globalAlpha = veil;
    halo(f, x, y, r, p.halo, p.haloFade);
    ctx.fillStyle = p.sun;
    ctx.beginPath();
    ctx.arc(x, y, r, 0, TAU);
    ctx.fill();
    ctx.globalAlpha = 1;
  }
  if (s.moon) {
    const [x, y] = skyPoint(f, s.moon.at, s.moon.height);
    const r = Math.max(5, H * 0.085);
    ctx.globalAlpha = veil * (0.35 + 0.65 * s.moon.lit);
    halo(f, x, y, r, p.halo, p.haloFade);
    ctx.globalAlpha = veil;
    ctx.fillStyle = p.moonDark;
    ctx.beginPath();
    ctx.arc(x, y, r, 0, TAU);
    ctx.fill();
    moonLit(f, x, y, r, s.moon.phase);
    ctx.globalAlpha = 1;
  }
}

/**
 * The lit part of the moon: a half disc on the side toward the sun, and the terminator, an
 * ellipse whose width follows the phase (a crescent bites into the half, a gibbous moon adds to it).
 */
function moonLit(f: Frame, x: number, y: number, r: number, phase: number) {
  const { ctx, p } = f;
  const k = Math.cos(TAU * phase); // 1 new, 0 quarter, −1 full
  if (k > 0.985) return; // new moon: nothing lit
  const waxing = phase < 0.5;
  // Waxing, it's lit on the west side (toward the sun that's set), which is opposite east.
  const litRight = waxing === f.east < 0;
  const Q = Math.PI / 2;
  ctx.fillStyle = p.moon;
  ctx.beginPath();
  if (litRight) {
    ctx.arc(x, y, r, -Q, Q, false);
    ctx.ellipse(x, y, r * Math.abs(k), r, 0, Q, -Q, k > 0);
  } else {
    ctx.arc(x, y, r, Q, 3 * Q, false);
    ctx.ellipse(x, y, r * Math.abs(k), r, 0, -Q, Q, k > 0);
  }
  ctx.closePath();
  ctx.fill();
}

function clouds(f: Frame) {
  const { ctx, W, H, t, p, s } = f;
  const n = Math.round(s.clouds * 6);
  if (!n) return;
  const span = W + 160;
  const speed = 3 + f.force * 2.5;
  const dir = f.windX === 0 ? 1 : Math.sign(f.windX);
  ctx.fillStyle = s.storm || s.rain > 0.6 ? p.cloudDark : p.cloud;
  for (let i = 0; i < n; i++) {
    const sc = (0.7 + rnd(i + 11) * 0.7) * (H / 96);
    const x = wrap(rnd(i + 7) * span + (f.still ? 0 : t * speed * dir * (0.7 + rnd(i) * 0.6)), span) - 80;
    const y = H * (0.1 + rnd(i + 3) * 0.28);
    ctx.globalAlpha = 0.75 + rnd(i + 5) * 0.2;
    ctx.beginPath();
    for (const [dx, dy, r] of [
      [0, 2, 9],
      [12, -4, 12],
      [26, 0, 10],
      [36, 3, 7],
      [14, 5, 9],
    ]) {
      ctx.moveTo(x + dx * sc + r * sc, y + dy * sc);
      ctx.arc(x + dx * sc, y + dy * sc, r * sc, 0, TAU);
    }
    ctx.fill();
  }
  ctx.globalAlpha = 1;
}

function shore(f: Frame) {
  const { ctx, W, horizon, t, p, s } = f;
  const night = !s.sun;
  // Low hills far off.
  ctx.fillStyle = p.shoreFar;
  ctx.beginPath();
  ctx.moveTo(0, horizon);
  for (let x = 0; x <= W; x += 8) ctx.lineTo(x, horizon - 4 - 4 * (Math.sin(x / 47) + 1) - 3 * Math.sin(x / 13 + 1));
  ctx.lineTo(W, horizon);
  ctx.fill();
  // The town on the far quay, with a few windows lit at night.
  const from = W * 0.58;
  const to = W * 0.86;
  let x = from;
  let i = 0;
  while (x < to) {
    const w = 7 + rnd(i + 70) * 9;
    const h = 5 + rnd(i + 80) * 13;
    ctx.fillStyle = p.shore;
    ctx.fillRect(x, horizon - h, w - 1, h);
    if (night) {
      ctx.fillStyle = p.lit;
      for (let k = 0; k < 3; k++) {
        if (rnd(i * 7 + k) > 0.55) ctx.fillRect(x + 1.5 + (k % 2) * 3, horizon - h + 2 + k * 3, 1.5, 1.5);
      }
    }
    x += w;
    i++;
  }
  ctx.fillStyle = p.shore;
  ctx.fillRect(from - 6, horizon - 3, to - from + 30, 3);
  // The lighthouse at the end of the quay; its beam sweeps at night and in fog.
  const lx = Math.min(W - 10, to + 18);
  ctx.beginPath();
  ctx.moveTo(lx - 3.5, horizon);
  ctx.lineTo(lx - 2, horizon - 20);
  ctx.lineTo(lx + 2, horizon - 20);
  ctx.lineTo(lx + 3.5, horizon);
  ctx.fill();
  ctx.fillRect(lx - 3, horizon - 23, 6, 3);
  ctx.fillStyle = p.lamp;
  ctx.fillRect(lx - 1.5, horizon - 22.5, 3, 2);
  if (night || s.fog) {
    const a = f.still ? 2.6 : t * 0.9;
    const reach = Math.cos(a) * W * 0.45;
    ctx.fillStyle = p.beam;
    ctx.beginPath();
    ctx.moveTo(lx, horizon - 21.5);
    ctx.lineTo(lx + reach, horizon - 21.5 - 7 * Math.abs(Math.cos(a)));
    ctx.lineTo(lx + reach, horizon - 21.5 + 7 * Math.abs(Math.cos(a)));
    ctx.closePath();
    ctx.fill();
  }
}

/** Wave height in px at x: grows with the Beaufort force. */
function waveY(f: Frame, x: number, row: number): number {
  const amp = Math.min(5, 0.5 + f.force * 0.38) * (0.6 + row * 0.25);
  const len = Math.max(16, 42 - f.force * 2);
  const speed = f.still ? 0 : 1 + f.force * 0.12;
  const drift = f.windX === 0 ? 1 : Math.sign(f.windX);
  return amp * Math.sin((x - drift * f.t * speed * 14) / len + row * 1.9) + amp * 0.35 * Math.sin(x / (len * 0.43) + f.t * speed * 1.7 + row);
}

function seaFill(f: Frame): CanvasGradient {
  const g = f.ctx.createLinearGradient(0, f.horizon, 0, f.H);
  g.addColorStop(0, f.p.water);
  g.addColorStop(1, f.p.waterDeep);
  return g;
}

/** The sun or moon's path on the water, the rows from `fromY` down. */
function glitter(f: Frame, fromY: number) {
  const { ctx, H, horizon, t, p, s } = f;
  const body = s.sun ?? s.moon;
  if (!body || s.clouds >= 0.9) return;
  const [bx] = skyPoint(f, body.at, body.height);
  ctx.fillStyle = s.sun ? p.sun : p.moon;
  const rows = 7;
  for (let k = 0; k < rows; k++) {
    const y = horizon + 3 + (k * (H - horizon - 4)) / rows;
    if (y < fromY) continue;
    const shimmer = f.still ? 0.8 : 0.55 + 0.45 * Math.sin(t * 2.2 + k * 1.3);
    const w = (5 + k * 3) * shimmer;
    ctx.globalAlpha = (0.55 - k * 0.05) * (1 - s.clouds * 0.7) * (s.moon ? 0.4 + 0.6 * s.moon.lit : 1);
    ctx.fillRect(bx - w / 2 + (f.still ? 0 : Math.sin(t * 1.4 + k) * 2), y, w, 1.2);
  }
  ctx.globalAlpha = 1;
}

/** The front edge of the water, the line the tug sits in. */
function frontLine(f: Frame, x: number): number {
  return f.H * 0.8 + Math.min(f.H * 0.78, 76) * 0.11 + waveY(f, x, 3) * 0.6;
}

function water(f: Frame) {
  const { ctx, W, H, horizon, p } = f;
  ctx.fillStyle = seaFill(f);
  ctx.fillRect(0, horizon, W, H - horizon);
  glitter(f, 0);
  // The far wave lines (the nearest is the front edge, drawn over the tug).
  ctx.strokeStyle = p.foam;
  ctx.lineWidth = 1;
  for (let row = 0; row < 3; row++) {
    const y0 = horizon + ((row + 1) * (H - horizon)) / 5;
    ctx.globalAlpha = 0.18 + row * 0.07;
    ctx.beginPath();
    for (let x = 0; x <= W; x += 4) {
      const y = y0 + waveY(f, x, row);
      if (x === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
}

/** The nearest water, over the tug's hull so it sits in the sea rather than on it, and whitecaps. */
function front(f: Frame) {
  const { ctx, W, H, horizon, t, p } = f;
  ctx.beginPath();
  ctx.moveTo(0, H);
  for (let x = 0; x <= W; x += 3) ctx.lineTo(x, frontLine(f, x));
  ctx.lineTo(W, H);
  ctx.closePath();
  // The sea's colours are see-through tints: lay the base first, or the hull shows through.
  ctx.fillStyle = p.base;
  ctx.fill();
  ctx.fillStyle = seaFill(f);
  ctx.fill();
  const body = f.s.sun ?? f.s.moon;
  if (body) glitter(f, frontLine(f, skyPoint(f, body.at, body.height)[0]));
  ctx.strokeStyle = p.foam;
  ctx.globalAlpha = 0.45;
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let x = 0; x <= W; x += 3) {
    if (x === 0) ctx.moveTo(x, frontLine(f, x));
    else ctx.lineTo(x, frontLine(f, x));
  }
  ctx.stroke();
  if (f.force >= 4) {
    ctx.fillStyle = p.foam;
    const n = (f.force - 3) * 5 * (W / 600);
    for (let i = 0; i < n; i++) {
      const life = f.still ? 0.5 : wrap(t * 0.5 + rnd(i + 200), 1);
      ctx.globalAlpha = Math.sin(life * Math.PI) * 0.8;
      const x = rnd(i + 210) * W;
      const y = horizon + 4 + rnd(i + 220) * (H - horizon - 6);
      ctx.beginPath();
      ctx.ellipse(x, y, 3 + f.force * 0.3, 1, 0, 0, TAU);
      ctx.fill();
    }
  }
  ctx.globalAlpha = 1;
}

function tug(f: Frame) {
  const { ctx, W, H, t, p, s } = f;
  const L = Math.min(H * 0.78, 76);
  const h = L * 0.2;
  const cx = W * 0.3;
  const waterline = H * 0.8;
  const bob = waveY(f, cx, 2) * 0.8;
  const roll = f.still ? 0 : Math.sin(t * 1.5) * Math.min(0.14, 0.015 + f.force * 0.012);
  const night = !s.sun;

  ctx.save();
  ctx.translate(cx, waterline + bob);
  ctx.rotate(roll);
  // Smoke from the funnel, carried off by the wind (straight up in a calm).
  const fx = -L * 0.27;
  const fy = -h * 2.35;
  const strength = Math.min(1, f.force / 6);
  ctx.fillStyle = p.smoke;
  for (let i = 0; i < 6; i++) {
    const a = f.still ? i / 6 : wrap(t * 0.45 + i / 6, 1);
    ctx.globalAlpha = (1 - a) * 0.55;
    ctx.beginPath();
    ctx.arc(fx + f.windX * a * 46 * strength, fy - a * 26 * (1 - strength * 0.65), 2 + a * 5, 0, TAU);
    ctx.fill();
  }
  ctx.globalAlpha = 1;
  // Funnel, with tug's band.
  ctx.fillStyle = p.funnel;
  ctx.fillRect(fx - L * 0.05, fy, L * 0.1, -fy - h * 0.2);
  ctx.fillStyle = p.hull;
  ctx.fillRect(fx - L * 0.05, fy + 2, L * 0.1, 2.5);
  // Mast and its pennant, which streams the way the wind goes.
  const mx = L * 0.06;
  const mtop = -h * 3.4;
  ctx.strokeStyle = p.funnel;
  ctx.lineWidth = 1.2;
  ctx.beginPath();
  ctx.moveTo(mx, -h * 2);
  ctx.lineTo(mx, mtop);
  ctx.stroke();
  pennant(f, mx, mtop);
  if (night) {
    ctx.fillStyle = p.lamp;
    ctx.beginPath();
    ctx.arc(mx, mtop - 1.5, 1.6, 0, TAU);
    ctx.fill();
  }
  // Cabin and wheelhouse.
  ctx.fillStyle = p.cabin;
  ctx.fillRect(-L * 0.2, -h * 1.25, L * 0.36, h * 1.3);
  ctx.fillRect(-L * 0.08, -h * 2.05, L * 0.22, h * 0.85);
  ctx.fillStyle = night ? p.lit : p.window;
  for (const wx of [-L * 0.15, -L * 0.06, L * 0.03]) ctx.fillRect(wx, -h * 0.95, L * 0.055, h * 0.4);
  ctx.fillRect(-L * 0.04, -h * 1.8, L * 0.15, h * 0.35);
  // The hull: low stern, raised bow (the bow points right).
  ctx.fillStyle = p.hull;
  ctx.beginPath();
  ctx.moveTo(-L / 2, -1);
  ctx.lineTo(L * 0.42, -2);
  ctx.quadraticCurveTo(L * 0.52, -h * 0.6, L * 0.56, -h * 0.75);
  ctx.quadraticCurveTo(L * 0.5, h * 0.9, L * 0.3, h);
  ctx.lineTo(-L * 0.4, h);
  ctx.quadraticCurveTo(-L * 0.52, h * 0.7, -L / 2, -1);
  ctx.fill();
  ctx.strokeStyle = p.hullDark;
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.moveTo(-L / 2 + 1, 1);
  ctx.lineTo(L * 0.44, 0);
  ctx.stroke();
  ctx.restore();
}

/** A pennant at the masthead: hangs in a calm, flies out as the wind rises, and flutters. */
function pennant(f: Frame, x: number, y: number) {
  const { ctx, t, p } = f;
  const lift = Math.min(1, f.force / 4);
  const dir = f.windX === 0 ? 1 : Math.sign(f.windX);
  // From hanging down the mast (π/2) to straight out downwind (0).
  const angle = (1 - lift) * (Math.PI / 2 - 0.25) + 0.05;
  const len = 9 + Math.min(4, f.force * 0.5);
  const flutter = f.still ? 0 : Math.sin(t * (4 + f.force)) * (0.6 + f.force * 0.25);
  const tipX = x + dir * Math.cos(angle) * len;
  const tipY = y + 1.5 + Math.sin(angle) * len + flutter;
  ctx.fillStyle = p.flag;
  ctx.beginPath();
  ctx.moveTo(x, y);
  ctx.lineTo(tipX, tipY);
  ctx.lineTo(x, y + 4.5);
  ctx.closePath();
  ctx.fill();
}

function fog(f: Frame, layer: 0 | 1) {
  const { ctx, W, horizon, t, p, s } = f;
  if (!s.fog) return;
  ctx.fillStyle = p.fog;
  const bands = layer === 0 ? [horizon - 10, horizon - 2, horizon + 8] : [horizon + 20];
  bands.forEach((y, k) => {
    const x = wrap((f.still ? 0 : t * (4 + k * 2)) + k * 130, W);
    ctx.globalAlpha = layer === 0 ? 0.75 : 0.45;
    for (const ox of [x - W, x]) {
      ctx.beginPath();
      ctx.ellipse(ox + W / 2, y, W * 0.62, 6 + k * 2, 0, 0, TAU);
      ctx.fill();
    }
  });
  ctx.globalAlpha = 1;
}

function rain(f: Frame) {
  const { ctx, W, H, t, p, s } = f;
  if (s.rain <= 0) return;
  const n = Math.round(s.rain * 110 * (W / 600));
  const slant = Math.max(-0.9, Math.min(0.9, f.windX * (0.1 + f.force * 0.06)));
  const len = 6 + s.rain * 6;
  ctx.strokeStyle = p.rain;
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let i = 0; i < n; i++) {
    const speed = 240 + rnd(i + 5) * 120;
    const y = wrap(rnd(i + 9) * (H + 20) + (f.still ? 0 : t * speed), H + 20) - 10;
    const x = wrap(rnd(i) * W + y * slant, W);
    ctx.moveTo(x, y);
    ctx.lineTo(x + slant * len, y + len);
  }
  ctx.stroke();
}

function snow(f: Frame) {
  const { ctx, W, H, t, p, s } = f;
  if (s.snow <= 0) return;
  const n = Math.round(s.snow * 80 * (W / 600));
  ctx.fillStyle = p.flake;
  for (let i = 0; i < n; i++) {
    const fall = 14 + rnd(i + 5) * 16;
    const y = wrap(rnd(i + 9) * (H + 6) + (f.still ? 0 : t * fall), H + 6) - 3;
    const sway = f.still ? 0 : Math.sin(t * 0.9 + i) * 5;
    const x = wrap(rnd(i) * W + sway + (f.still ? 0 : t * f.windX * (2 + f.force * 3)), W);
    ctx.beginPath();
    ctx.arc(x, y, 0.8 + rnd(i + 3) * 1.2, 0, TAU);
    ctx.fill();
  }
}

/** A storm flashes now and then, with a bolt to the sea. Never with reduced motion. */
function lightning(f: Frame) {
  const { ctx, W, H, t, horizon, p, s } = f;
  if (!s.storm || f.still) return;
  const period = 6.5;
  const k = Math.floor(t / period);
  const local = t - k * period;
  if (local > 0.35) return;
  ctx.globalAlpha = ((0.35 - local) / 0.35) * 0.5;
  ctx.fillStyle = p.flash;
  ctx.fillRect(0, 0, W, H);
  if (local < 0.16) {
    ctx.globalAlpha = 1;
    ctx.strokeStyle = p.flash;
    ctx.lineWidth = 1.5;
    let x = W * (0.45 + rnd(k) * 0.45);
    let y = H * 0.15;
    ctx.beginPath();
    ctx.moveTo(x, y);
    for (let i = 0; y < horizon; i++) {
      x += (rnd(k * 13 + i) - 0.5) * 12;
      y += 5 + rnd(k * 7 + i) * 6;
      ctx.lineTo(x, Math.min(y, horizon));
    }
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
}
