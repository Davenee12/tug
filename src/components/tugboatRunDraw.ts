// Draws one frame of Tugboat Run onto a canvas: the harbor, what's floating in it and the
// tugboat (or two lanes side by side in a race), in simple shapes and tug's own palette (read from
// theme classes by the component, so no colour is written here). The daily challenge's weather
// shows too: rain and choppy water, snow, wind streaks, and at night a darker harbor with a
// lighthouse sweeping its beam. Pure drawing: no state, no timers.

import { BOAT_H, BOAT_W, BOAT_Y, FIELD_H, FIELD_W, GRACE_S, HOP_S, LANE_GAP, waveAt, type Layout, type Run, type Thing } from "../lib/tugboatRun";

export interface Palette {
  /** The water is a translucent tint over this. */
  waterBase: string;
  water: string;
  waterDeep: string;
  quay: string;
  quayEdge: string;
  hull: string;
  /** Player 2's hull in a race. */
  hull2: string;
  deck: string;
  cabin: string;
  ink: string;
  rope: string;
  coin: string;
  log: string;
  logEnd: string;
  boat: string;
  boatCabin: string;
  foam: string;
  /** Rain streaks and spray. */
  spray: string;
  flake: string;
  /** The lighthouse: its tower, lamp and beam, and the glow of the tug's own lamp at night. */
  tower: string;
  lamp: string;
  beam: string;
  glow: string;
}

/** What the weather looks like (the daily challenge; nothing in calm water). */
export interface Sky {
  rain: 0 | 1 | 2 | 3;
  snow: boolean;
  night: boolean;
  /** The wind's push, field units a second (its streaks lean and drift this way). */
  drift: number;
}

export interface Lane {
  run: Run;
  layout: Layout;
  palette: Palette;
  /** The tug's hull: coral, or teal for Player 2 in a race (buoys stay the palette's colour). */
  hull: string;
  /** Seconds of screen shake left (0 with reduced motion). */
  shake: number;
  /** Which quay the lighthouse stands on at night (the outer one in a race). */
  lighthouse: "left" | "right";
}

export interface View {
  /** Canvas size in CSS pixels, and device pixels per CSS pixel. */
  width: number;
  height: number;
  dpr: number;
  reducedMotion: boolean;
  sky: Sky | null;
  lanes: Lane[];
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, r);
}

/** A repeatable number in [0, 1) for decoration number `i` (rain drops, flakes), no state. */
function hash(i: number): number {
  let h = Math.imul(i + 1, 0x9e3779b1);
  h = Math.imul(h ^ (h >>> 15), 0x85ebca6b);
  return ((h ^ (h >>> 13)) >>> 0) / 4294967296;
}

export function draw(ctx: CanvasRenderingContext2D, view: View) {
  const { dpr } = view;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  // The quays either side of the channel(s).
  ctx.fillStyle = view.lanes[0]?.palette.quay ?? "";
  ctx.fillRect(0, 0, view.width, view.height);
  const two = view.lanes.length > 1;
  for (const lane of view.lanes) {
    ctx.save();
    if (two) {
      // Each lane keeps to its own half (a shake never spills into the other player's).
      const l = lane.layout;
      ctx.beginPath();
      ctx.rect(l.left - (LANE_GAP / 2) * l.scale, 0, (FIELD_W + LANE_GAP) * l.scale, view.height);
      ctx.clip();
    }
    drawLane(ctx, lane, view);
    ctx.restore();
  }
  ctx.setTransform(1, 0, 0, 1, 0, 0);
}

function drawLane(ctx: CanvasRenderingContext2D, lane: Lane, view: View) {
  const { dpr } = view;
  const { run, layout: l, palette: p } = lane;
  let ox = l.left;
  let oy = l.top;
  if (lane.shake > 0 && !view.reducedMotion) {
    const k = Math.min(1, lane.shake / 0.25) * 3;
    ox += Math.sin(run.t * 91) * k;
    oy += Math.cos(run.t * 77) * k;
  }
  // From here on, field units.
  ctx.setTransform(dpr * l.scale, 0, 0, dpr * l.scale, dpr * ox, dpr * oy);
  const top = -oy / l.scale - 2;
  const bottom = (view.height - oy) / l.scale + 2;
  const sky = view.sky;

  // Water, and the stone edge of each quay with its bollards scrolling past.
  ctx.fillStyle = p.waterBase;
  ctx.fillRect(0, top, FIELD_W, bottom - top);
  ctx.fillStyle = p.water;
  ctx.fillRect(0, top, FIELD_W, bottom - top);
  ctx.fillStyle = p.quayEdge;
  ctx.fillRect(-3, top, 3, bottom - top);
  ctx.fillRect(FIELD_W, top, 3, bottom - top);
  const span = 40;
  const shift = run.distance % span;
  ctx.fillStyle = p.ink;
  for (let y = top - span + shift; y < bottom; y += span) {
    ctx.beginPath();
    ctx.arc(-6, y, 1.6, 0, Math.PI * 2);
    ctx.arc(FIELD_W + 6, y + span / 2, 1.6, 0, Math.PI * 2);
    ctx.fill();
  }

  // Ripples drift down with the water (fewer with reduced motion: calmer, still shows speed). In
  // rain there are more, and the crests rock from side to side with the swell.
  ctx.strokeStyle = p.waterDeep;
  ctx.lineWidth = 0.6;
  ctx.lineCap = "round";
  const rain = sky?.rain ?? 0;
  const rows = (view.reducedMotion ? 6 : 12) + rain * (view.reducedMotion ? 2 : 5);
  const rowGap = (FIELD_H + 40) / rows;
  const sway = rain > 0 ? waveAt(run.t) * 2.5 : 0;
  for (let i = 0; i < rows; i++) {
    const y = ((i * rowGap + run.distance * 0.9) % (FIELD_H + 40)) - 20;
    const x = ((i * 37) % (FIELD_W - 16)) + 8 + sway;
    ctx.beginPath();
    ctx.arc(x, y, 3 + rain * 0.6, 0.15 * Math.PI, 0.85 * Math.PI);
    ctx.stroke();
  }

  if (sky?.night) drawLighthouse(ctx, run, view, lane, top);
  for (const th of run.things) drawThing(ctx, th, p);
  if (sky?.night) {
    // The tug's own lamp: a soft pool of light around it, so the water just ahead can be read.
    ctx.save();
    const g = ctx.createRadialGradient(run.x, BOAT_Y - 6, 2, run.x, BOAT_Y - 6, 26);
    g.addColorStop(0, p.glow);
    g.addColorStop(1, "transparent");
    ctx.fillStyle = g;
    ctx.fillRect(run.x - 28, BOAT_Y - 34, 56, 56);
    ctx.restore();
  }
  drawTug(ctx, run, view, p, lane.hull);
  if (sky) drawWeather(ctx, run, view, p, sky, top, bottom);
}

/** Rain streaks, snowflakes and wind streaks over the water (or, in Harbor Stack, over the whole
 * harbor). With reduced motion they ride the water down like the ripples instead of falling across
 * it. `run` needs only its clock and how far the water has gone. */
export function drawWeather(
  ctx: CanvasRenderingContext2D,
  run: Pick<Run, "t" | "distance">,
  view: Pick<View, "reducedMotion">,
  p: Pick<Palette, "spray" | "flake" | "waterDeep">,
  sky: Sky,
  top: number,
  bottom: number,
) {
  const height = bottom - top + 20;
  const lean = Math.max(-1, Math.min(1, sky.drift / 9));
  // Over the water only (the quays stay dry, and a race lane's weather stays in its lane).
  ctx.save();
  ctx.beginPath();
  ctx.rect(0, top, FIELD_W, bottom - top);
  ctx.clip();
  const fall = (speed: number, i: number) => {
    const base = hash(i * 3 + 1) * height;
    const moved = view.reducedMotion ? run.distance * 0.9 : run.t * speed + run.distance * 0.3;
    return top - 10 + ((base + moved) % height);
  };
  if (sky.rain > 0) {
    const drops = [0, 14, 26, 40][sky.rain] / (view.reducedMotion ? 2 : 1);
    ctx.strokeStyle = p.spray;
    ctx.lineWidth = 0.45;
    ctx.beginPath();
    for (let i = 0; i < drops; i++) {
      const x = hash(i * 3) * (FIELD_W + 10) - 5;
      const y = fall(150, i);
      ctx.moveTo(x, y);
      ctx.lineTo(x + lean * 2.2 - 0.6, y + 4.5);
    }
    ctx.stroke();
  }
  if (sky.snow) {
    const flakes = view.reducedMotion ? 16 : 32;
    ctx.fillStyle = p.flake;
    ctx.beginPath();
    for (let i = 0; i < flakes; i++) {
      const y = fall(18, i);
      const wobble = view.reducedMotion ? 0 : Math.sin(run.t * 1.4 + i) * 1.5;
      const x = ((hash(i * 3 + 2) * (FIELD_W + 10) + (view.reducedMotion ? 0 : run.t * sky.drift * 0.6) + wobble) % (FIELD_W + 10)) - 5;
      const r = 0.45 + hash(i * 7) * 0.5;
      ctx.moveTo(x + r, y);
      ctx.arc(x, y, r, 0, Math.PI * 2);
    }
    ctx.fill();
    // A fine rim, so white flakes still show on pale water.
    ctx.strokeStyle = p.waterDeep;
    ctx.lineWidth = 0.15;
    ctx.stroke();
  }
  if (Math.abs(sky.drift) >= 2 && !view.reducedMotion) {
    // A few long streaks blowing across with the wind.
    ctx.strokeStyle = p.spray;
    ctx.lineWidth = 0.35;
    ctx.globalAlpha = 0.6;
    ctx.beginPath();
    const n = 5;
    for (let i = 0; i < n; i++) {
      const y = top + ((hash(i * 5) * (bottom - top) + run.distance * 0.5) % (bottom - top));
      const x = ((hash(i * 5 + 1) * (FIELD_W + 40) + run.t * sky.drift * 4) % (FIELD_W + 40) + FIELD_W + 40) % (FIELD_W + 40) - 20;
      ctx.moveTo(x, y);
      ctx.lineTo(x + Math.sign(sky.drift) * 9, y + 0.6);
    }
    ctx.stroke();
    ctx.globalAlpha = 1;
  }
  ctx.restore();
}

/** At night: a lighthouse on the quay near the top, its beam sweeping slowly over the harbor
 * (still, with reduced motion). */
function drawLighthouse(ctx: CanvasRenderingContext2D, run: Run, view: View, lane: Lane, top: number) {
  const p = lane.palette;
  const left = lane.lighthouse === "left";
  const x = left ? -9 : FIELD_W + 9;
  const y = Math.max(top + 14, 18);
  const sweep = view.reducedMotion ? 0.35 : 0.35 + Math.sin(run.t * 0.55) * 0.55;
  // Pointing into the channel and down it, sweeping across.
  const angle = left ? sweep : Math.PI - sweep;
  const reach = 150;
  const spread = 0.16;
  ctx.save();
  // The beam lights the water, not the far quay.
  ctx.beginPath();
  if (left) ctx.rect(x - 3, top, FIELD_W - x + 3, 400);
  else ctx.rect(0, top, x + 3, 400);
  ctx.clip();
  ctx.fillStyle = p.beam;
  ctx.beginPath();
  ctx.moveTo(x, y - 6);
  ctx.lineTo(x + Math.cos(angle - spread) * reach, y - 6 + Math.sin(angle - spread) * reach);
  ctx.lineTo(x + Math.cos(angle + spread) * reach, y - 6 + Math.sin(angle + spread) * reach);
  ctx.closePath();
  ctx.fill();
  // The tower: striped, with its lamp.
  ctx.fillStyle = p.tower;
  ctx.beginPath();
  ctx.moveTo(x - 2.6, y + 4);
  ctx.lineTo(x - 1.6, y - 5);
  ctx.lineTo(x + 1.6, y - 5);
  ctx.lineTo(x + 2.6, y + 4);
  ctx.closePath();
  ctx.fill();
  ctx.fillStyle = p.hull;
  ctx.fillRect(x - 2.2, y - 1.5, 4.4, 1.6);
  ctx.fillStyle = p.lamp;
  ctx.beginPath();
  ctx.arc(x, y - 6, 1.5, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
}

function drawThing(ctx: CanvasRenderingContext2D, th: Thing, p: Palette) {
  ctx.save();
  ctx.globalAlpha = th.hit ? 0.3 : 1;
  ctx.translate(th.x, th.y);
  const r = th.w / 2;
  switch (th.kind) {
    case "buoy":
      ctx.fillStyle = p.hull;
      ctx.beginPath();
      ctx.arc(0, 0, r, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = p.deck;
      ctx.fillRect(-r, -0.7, r * 2, 1.4);
      ctx.fillStyle = p.ink;
      ctx.beginPath();
      ctx.arc(0, 0, 0.8, 0, Math.PI * 2);
      ctx.fill();
      break;
    case "log":
      ctx.fillStyle = p.log;
      roundRect(ctx, -th.w / 2, -th.h / 2, th.w, th.h, th.h / 2);
      ctx.fill();
      ctx.fillStyle = p.logEnd;
      ctx.beginPath();
      ctx.arc(-th.w / 2 + th.h / 2, 0, th.h * 0.32, 0, Math.PI * 2);
      ctx.arc(th.w / 2 - th.h / 2, 0, th.h * 0.32, 0, Math.PI * 2);
      ctx.fill();
      break;
    case "boat":
      // Coming towards you: the bow points down the screen.
      ctx.fillStyle = p.boat;
      ctx.beginPath();
      ctx.moveTo(-th.w / 2, -th.h / 2 + 1);
      ctx.quadraticCurveTo(-th.w / 2, -th.h / 2, -th.w / 2 + 1, -th.h / 2);
      ctx.lineTo(th.w / 2 - 1, -th.h / 2);
      ctx.quadraticCurveTo(th.w / 2, -th.h / 2, th.w / 2, -th.h / 2 + 1);
      ctx.lineTo(th.w / 2, th.h / 6);
      ctx.quadraticCurveTo(th.w / 2, th.h / 2 - 1, 0, th.h / 2);
      ctx.quadraticCurveTo(-th.w / 2, th.h / 2 - 1, -th.w / 2, th.h / 6);
      ctx.closePath();
      ctx.fill();
      ctx.fillStyle = p.boatCabin;
      roundRect(ctx, -th.w / 2 + 2, -th.h / 2 + 3, th.w - 4, th.h / 2 - 1, 0.8);
      ctx.fill();
      break;
    case "coin":
      ctx.fillStyle = p.coin;
      ctx.beginPath();
      ctx.arc(0, 0, r, 0, Math.PI * 2);
      ctx.fill();
      ctx.strokeStyle = p.deck;
      ctx.lineWidth = 0.5;
      ctx.beginPath();
      ctx.arc(0, 0, r * 0.6, 0, Math.PI * 2);
      ctx.stroke();
      break;
    case "ring":
      ctx.lineWidth = 1.6;
      for (let i = 0; i < 8; i++) {
        ctx.strokeStyle = i % 2 ? p.deck : p.hull;
        ctx.beginPath();
        ctx.arc(0, 0, r - 0.8, (i * Math.PI) / 4, ((i + 1) * Math.PI) / 4);
        ctx.stroke();
      }
      break;
  }
  ctx.restore();
}

/** The tugboat from above: coral hull, cream deck, dark wheelhouse, a funnel, and a rope fender
 * round the bow like the knots in tug's logo. */
function drawTug(ctx: CanvasRenderingContext2D, run: Run, view: View, p: Palette, hull: string) {
  const w = BOAT_W;
  const h = BOAT_H;
  // In the air: a touch bigger, with its shadow left on the water.
  const air = run.hop > 0 ? Math.sin((1 - run.hop / HOP_S) * Math.PI) : 0;
  ctx.save();
  ctx.translate(run.x, BOAT_Y);
  // A sunk boat (a race lane that's out) shows faded where it went down.
  if (run.over) ctx.globalAlpha = 0.35;

  // Wake.
  ctx.strokeStyle = p.foam;
  ctx.lineWidth = 0.8;
  ctx.globalAlpha *= 0.8;
  ctx.beginPath();
  ctx.moveTo(-w / 2 + 1, h / 2 - 1);
  ctx.lineTo(-w / 2 - 3 - run.vx * 0.04, h / 2 + 9);
  ctx.moveTo(w / 2 - 1, h / 2 - 1);
  ctx.lineTo(w / 2 + 3 - run.vx * 0.04, h / 2 + 9);
  ctx.stroke();

  const base = run.over ? 0.35 : 1;
  if (air > 0) {
    ctx.globalAlpha = 0.18 * base;
    ctx.fillStyle = p.ink;
    ctx.beginPath();
    ctx.ellipse(1.5 * air, 2 * air, w / 2, h / 2, 0, 0, Math.PI * 2);
    ctx.fill();
  }

  // Blink while in grace (a steady half-fade with reduced motion).
  if (run.grace > 0 && !run.over) {
    const blinkOn = Math.floor((GRACE_S - run.grace) * 10) % 2 === 0;
    ctx.globalAlpha = view.reducedMotion ? 0.5 : blinkOn ? 0.35 : 1;
  } else {
    ctx.globalAlpha = base;
  }
  const s = 1 + 0.18 * air;
  ctx.scale(s, s);
  // Tilt a little into the turn.
  ctx.rotate(Math.max(-0.25, Math.min(0.25, run.vx / 300)));

  // Hull: round stern, pointed bow.
  ctx.fillStyle = hull;
  ctx.beginPath();
  ctx.moveTo(0, -h / 2);
  ctx.quadraticCurveTo(w / 2, -h / 2 + 3, w / 2, -h / 6);
  ctx.lineTo(w / 2, h / 2 - 2);
  ctx.quadraticCurveTo(w / 2, h / 2, w / 2 - 2, h / 2);
  ctx.lineTo(-w / 2 + 2, h / 2);
  ctx.quadraticCurveTo(-w / 2, h / 2, -w / 2, h / 2 - 2);
  ctx.lineTo(-w / 2, -h / 6);
  ctx.quadraticCurveTo(-w / 2, -h / 2 + 3, 0, -h / 2);
  ctx.closePath();
  ctx.fill();

  // Rope fender round the bow.
  ctx.strokeStyle = p.rope;
  ctx.lineWidth = 1.1;
  ctx.setLineDash([0.9, 0.6]);
  ctx.beginPath();
  ctx.moveTo(-w / 2 + 0.6, -h / 6);
  ctx.quadraticCurveTo(-w / 2 + 0.6, -h / 2 + 3.4, 0, -h / 2 + 0.6);
  ctx.quadraticCurveTo(w / 2 - 0.6, -h / 2 + 3.4, w / 2 - 0.6, -h / 6);
  ctx.stroke();
  ctx.setLineDash([]);

  // Deck, wheelhouse and funnel.
  ctx.fillStyle = p.deck;
  roundRect(ctx, -w / 2 + 1.3, -h / 6, w - 2.6, h / 2 + h / 6 - 1.3, 1.2);
  ctx.fill();
  ctx.fillStyle = p.cabin;
  roundRect(ctx, -w / 2 + 1.9, -h / 6 + 0.6, w - 3.8, h / 3, 0.8);
  ctx.fill();
  ctx.fillStyle = p.ink;
  ctx.beginPath();
  ctx.arc(0, h / 4, 1.4, 0, Math.PI * 2);
  ctx.fill();
  ctx.fillStyle = hull;
  ctx.beginPath();
  ctx.arc(0, h / 4, 0.7, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
}
