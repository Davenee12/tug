// Draws one frame of Tugboat Run onto a canvas: the harbor, what's floating in it and the
// tugboat, in simple shapes and tug's own palette (read from theme classes by the component, so
// no colour is written here). Pure drawing: no state, no timers.

import { BOAT_H, BOAT_W, BOAT_Y, FIELD_H, FIELD_W, GRACE_S, HOP_S, type Layout, type Run, type Thing } from "../lib/tugboatRun";

export interface Palette {
  /** The water is a translucent tint over this. */
  waterBase: string;
  water: string;
  waterDeep: string;
  quay: string;
  quayEdge: string;
  hull: string;
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
}

export interface View {
  /** Canvas size in CSS pixels, and device pixels per CSS pixel. */
  width: number;
  height: number;
  dpr: number;
  layout: Layout;
  reducedMotion: boolean;
  /** Seconds of screen shake left (0 with reduced motion). */
  shake: number;
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, r);
}

export function draw(ctx: CanvasRenderingContext2D, run: Run, view: View, p: Palette) {
  const { dpr, layout: l } = view;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  // The quays either side of the channel.
  ctx.fillStyle = p.quay;
  ctx.fillRect(0, 0, view.width, view.height);

  let ox = l.left;
  let oy = l.top;
  if (view.shake > 0 && !view.reducedMotion) {
    const k = Math.min(1, view.shake / 0.25) * 3;
    ox += Math.sin(run.t * 91) * k;
    oy += Math.cos(run.t * 77) * k;
  }
  // From here on, field units.
  ctx.setTransform(dpr * l.scale, 0, 0, dpr * l.scale, dpr * ox, dpr * oy);
  const top = -oy / l.scale - 2;
  const bottom = (view.height - oy) / l.scale + 2;

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

  // Ripples drift down with the water (fewer with reduced motion: calmer, still shows speed).
  ctx.strokeStyle = p.waterDeep;
  ctx.lineWidth = 0.6;
  ctx.lineCap = "round";
  const rows = view.reducedMotion ? 6 : 12;
  const rowGap = (FIELD_H + 40) / rows;
  for (let i = 0; i < rows; i++) {
    const y = ((i * rowGap + run.distance * 0.9) % (FIELD_H + 40)) - 20;
    const x = (i * 37) % (FIELD_W - 16) + 8;
    ctx.beginPath();
    ctx.arc(x, y, 3, 0.15 * Math.PI, 0.85 * Math.PI);
    ctx.stroke();
  }

  for (const th of run.things) drawThing(ctx, th, p);
  drawTug(ctx, run, view, p);
  ctx.setTransform(1, 0, 0, 1, 0, 0);
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
function drawTug(ctx: CanvasRenderingContext2D, run: Run, view: View, p: Palette) {
  const w = BOAT_W;
  const h = BOAT_H;
  // In the air: a touch bigger, with its shadow left on the water.
  const air = run.hop > 0 ? Math.sin((1 - run.hop / HOP_S) * Math.PI) : 0;
  ctx.save();
  ctx.translate(run.x, BOAT_Y);

  // Wake.
  ctx.strokeStyle = p.foam;
  ctx.lineWidth = 0.8;
  ctx.globalAlpha = 0.8;
  ctx.beginPath();
  ctx.moveTo(-w / 2 + 1, h / 2 - 1);
  ctx.lineTo(-w / 2 - 3 - run.vx * 0.04, h / 2 + 9);
  ctx.moveTo(w / 2 - 1, h / 2 - 1);
  ctx.lineTo(w / 2 + 3 - run.vx * 0.04, h / 2 + 9);
  ctx.stroke();

  if (air > 0) {
    ctx.globalAlpha = 0.18;
    ctx.fillStyle = p.ink;
    ctx.beginPath();
    ctx.ellipse(1.5 * air, 2 * air, w / 2, h / 2, 0, 0, Math.PI * 2);
    ctx.fill();
  }

  // Blink while in grace (a steady half-fade with reduced motion).
  if (run.grace > 0) {
    const blinkOn = Math.floor((GRACE_S - run.grace) * 10) % 2 === 0;
    ctx.globalAlpha = view.reducedMotion ? 0.5 : blinkOn ? 0.35 : 1;
  } else {
    ctx.globalAlpha = 1;
  }
  const s = 1 + 0.18 * air;
  ctx.scale(s, s);
  // Tilt a little into the turn.
  ctx.rotate(Math.max(-0.25, Math.min(0.25, run.vx / 300)));

  // Hull: round stern, pointed bow.
  ctx.fillStyle = p.hull;
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
  ctx.fillStyle = p.hull;
  ctx.beginPath();
  ctx.arc(0, h / 4, 0.7, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
}
