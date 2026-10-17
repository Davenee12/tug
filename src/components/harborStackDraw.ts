// Draws one frame of Harbor Stack onto a canvas: the harbor seen from the side, the boat rocking on
// the water with its stack of containers, the crane overhead with the next container on its hook,
// sliced ends tumbling into the water, and a celebration at milestones (or two scenes side by side
// in a stack-off). tug's own palette, read from theme classes by the component, so no colour is
// written here. The daily's weather shows too: rain, snow and wind streaks (Tugboat Run's), and at
// night a darker harbor with a lighthouse sweeping its beam across the sky. Pure drawing: no state,
// no timers.
//
// Reduced motion: no shake, the boat doesn't rock or bob (the stack still leans exactly as the game
// says, since that's what you aim at), the water and the lighthouse hold still, and a celebration is
// bunting without falling confetti.

import {
  BOAT_W,
  BOX_H,
  CRANE_GAP,
  DECK_Y,
  FIELD_W,
  HOLD_S,
  WATER_Y,
  craneX,
  driftAt,
  gustWarning,
  swayAt,
  topHeight,
  type Kind,
  type Stack,
} from "../lib/harborStack";
import type { Layout } from "../lib/tugboatRun";
import { drawWeather, type Sky } from "./tugboatRunDraw";

export interface StackPalette {
  sky: string;
  skyline: string;
  waterBase: string;
  water: string;
  waterDeep: string;
  foam: string;
  deck: string;
  cabin: string;
  ink: string;
  rope: string;
  steel: string;
  steelDark: string;
  coral: string;
  teal: string;
  amber: string;
  slate: string;
  green: string;
  gold: string;
  spray: string;
  flake: string;
  tower: string;
  lamp: string;
  beam: string;
  star: string;
}

export interface StackLane {
  stack: Stack;
  layout: Layout;
  palette: StackPalette;
  /** The boat's hull: coral, or teal for Player 2 in a stack-off. */
  hull: string;
  /** Seconds of shake left (0 with reduced motion). */
  shake: number;
  /** Where the crane's trolley sits when it isn't holding anything (it stays where it let go). */
  trolleyX: number;
}

export interface StackView {
  width: number;
  height: number;
  dpr: number;
  reducedMotion: boolean;
  sky: Sky | null;
  lanes: StackLane[];
}

/** Where the crane's rail runs, on screen (it rises with the stack). */
const RAIL_Y = 12;
/** The gap between two scenes in a stack-off, in field units (as Tugboat Run's lanes). */
const LANE_GAP = 14;

function hash(i: number): number {
  let h = Math.imul(i + 1, 0x9e3779b1);
  h = Math.imul(h ^ (h >>> 15), 0x85ebca6b);
  return ((h ^ (h >>> 13)) >>> 0) / 4294967296;
}

function colour(p: StackPalette, kind: Kind): string {
  return p[kind];
}

export function drawStack(ctx: CanvasRenderingContext2D, view: StackView) {
  const { dpr } = view;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = view.lanes[0]?.palette.sky ?? "";
  ctx.fillRect(0, 0, view.width, view.height);
  const two = view.lanes.length > 1;
  for (const lane of view.lanes) {
    ctx.save();
    if (two) {
      const l = lane.layout;
      ctx.beginPath();
      ctx.rect(l.left - (LANE_GAP / 2) * l.scale, 0, (FIELD_W + LANE_GAP) * l.scale, view.height);
      ctx.clip();
    }
    drawScene(ctx, lane, view, two);
    ctx.restore();
  }
  ctx.setTransform(1, 0, 0, 1, 0, 0);
}

function drawScene(ctx: CanvasRenderingContext2D, lane: StackLane, view: StackView, two: boolean) {
  const { dpr } = view;
  const { stack: s, layout: l, palette: p } = lane;
  let ox = l.left;
  let oy = l.top;
  if (lane.shake > 0 && !view.reducedMotion) {
    const k = Math.min(1, lane.shake / 0.3) * 3;
    ox += Math.sin(s.t * 91) * k;
    oy += Math.cos(s.t * 77) * k;
  }
  ctx.setTransform(dpr * l.scale, 0, 0, dpr * l.scale, dpr * ox, dpr * oy);
  // The visible stretch, in field units (a wide window shows more harbor either side).
  const left = two ? -LANE_GAP / 2 : -ox / l.scale - 2;
  const right = two ? FIELD_W + LANE_GAP / 2 : (view.width - ox) / l.scale + 2;
  const top = -oy / l.scale - 2;
  const bottom = (view.height - oy) / l.scale + 2;
  const cam = s.cam;
  const night = view.sky?.night ?? false;
  const level = s.blocks.length;
  const lean = swayAt(s.seed, s.t, level, s.weather);
  const drift = driftAt(s.seed, s.t);
  const water = WATER_Y + cam;

  // Stars at night, then the far shore: a skyline of sheds and cranes on the horizon.
  if (night) {
    ctx.fillStyle = p.star;
    for (let i = 0; i < 26; i++) {
      const x = left + hash(i * 7) * (right - left);
      const y = top + hash(i * 7 + 1) * (water - top - 30);
      ctx.fillRect(x, y, 0.5, 0.5);
    }
  }
  ctx.fillStyle = p.skyline;
  for (let x = Math.floor(left / 9) * 9; x < right; x += 9) {
    const n = Math.floor(x / 9) + 1000;
    const hgt = 4 + hash(n) * 9;
    ctx.fillRect(x, water - hgt, 8.2, hgt);
    if (hash(n * 3) < 0.18) {
      // A dockside crane on the far shore.
      ctx.fillRect(x + 3, water - hgt - 12, 1, 12);
      ctx.fillRect(x - 2, water - hgt - 12, 11, 1);
    }
  }
  if (night) drawLighthouse(ctx, s.t, view.reducedMotion, p, left, water, right);

  // The water: a base, a tint, and ripples drifting along (more of them in rain).
  ctx.fillStyle = p.waterBase;
  ctx.fillRect(left, water, right - left, bottom - water);
  ctx.fillStyle = p.water;
  ctx.fillRect(left, water, right - left, bottom - water);
  const rain = view.sky?.rain ?? 0;
  ctx.strokeStyle = p.waterDeep;
  ctx.lineWidth = 0.6;
  ctx.lineCap = "round";
  const rows = 6 + rain * 3;
  const flow = view.reducedMotion ? 0 : s.t * 4;
  for (let i = 0; i < rows; i++) {
    const y = water + 3 + (i * (bottom - water)) / rows;
    const span = right - left;
    const x = left + ((((i * 37 + flow * (1 + (i % 3))) % span) + span) % span);
    ctx.beginPath();
    ctx.arc(x, y, 2.4 + rain * 0.5, 0.15 * Math.PI, 0.85 * Math.PI);
    ctx.stroke();
  }

  // The boat, rocking with the swell (still, with reduced motion), and the stack on its deck.
  const pivotX = FIELD_W / 2 + drift;
  const deck = DECK_Y + cam;
  const bob = view.reducedMotion ? 0 : Math.sin(s.t * 1.9) * 0.4;
  ctx.save();
  ctx.translate(pivotX, deck + bob);
  if (!view.reducedMotion) ctx.rotate(lean * 0.5);
  drawBoat(ctx, p, lane.hull);
  ctx.restore();

  ctx.save();
  ctx.translate(pivotX, deck + bob);
  // A point h above the deck sits h × lean across: the stack's lean, exactly as the game has it.
  ctx.transform(1, 0, -lean, 1, 0, 0);
  const visibleFrom = Math.max(0, Math.floor((deck - bottom) / BOX_H));
  const visibleTo = Math.min(s.blocks.length, Math.ceil((deck - top) / BOX_H) + 1);
  for (let i = visibleFrom; i < visibleTo; i++) {
    const b = s.blocks[i];
    drawContainer(ctx, p, b.kind, b.x, -(i + 1) * BOX_H, b.w);
  }
  // A perfect landing glints; one that grows the container back glints wider.
  if (s.landed && s.blocks.length > 1) {
    const age = s.t - s.landed.t;
    if (s.landed.perfect && age < 0.45) {
      const b = s.blocks[s.blocks.length - 1];
      const k = 1 - age / 0.45;
      ctx.strokeStyle = p.foam;
      ctx.globalAlpha = k;
      ctx.lineWidth = s.landed.grew ? 1.4 : 0.8;
      const grow = view.reducedMotion ? 0.6 : (1 - k) * (s.landed.grew ? 4 : 2.2);
      ctx.strokeRect(b.x - b.w / 2 - grow, -s.blocks.length * BOX_H - grow, b.w + grow * 2, BOX_H + grow * 2);
      ctx.globalAlpha = 1;
    }
  }
  ctx.restore();

  // The crane overhead: its rail, the trolley, and the container on the hook.
  const stackTop = deck - topHeight(s);
  drawCrane(ctx, s, view, p, lane, left, right, stackTop);

  if (s.falling) {
    const f = s.falling;
    drawContainer(ctx, p, f.kind, f.x, DECK_Y + cam - f.h - BOX_H, f.w);
  }
  for (const piece of s.pieces) {
    if (piece.splashed === null) {
      ctx.save();
      ctx.translate(piece.x, DECK_Y + cam - piece.h - BOX_H / 2);
      ctx.rotate(piece.angle);
      drawContainer(ctx, p, piece.kind, 0, -BOX_H / 2, piece.w);
      ctx.restore();
    } else {
      // A splash: rings spreading on the water.
      const age = s.t - piece.splashed;
      ctx.strokeStyle = p.foam;
      ctx.globalAlpha = Math.max(0, 1 - age / 0.8);
      ctx.lineWidth = 0.7;
      const r = 2 + age * (view.reducedMotion ? 4 : 14);
      ctx.beginPath();
      ctx.ellipse(piece.x, water + 1, r, r * 0.3, 0, 0, Math.PI * 2);
      ctx.stroke();
      ctx.globalAlpha = 1;
    }
  }

  if (s.cheer && s.t - s.cheer.t < 2.6) drawCheer(ctx, s.t - s.cheer.t, view.reducedMotion, p, left, right);
  if (view.sky) drawWeather(ctx, { t: s.t, distance: s.t * 30 + cam }, view, p, view.sky, top, bottom);
}

function drawBoat(ctx: CanvasRenderingContext2D, p: StackPalette, hull: string) {
  const w = BOAT_W;
  const depth = WATER_Y - DECK_Y + 5;
  // Hull: a flat deck, a raised bow on the left, the stern on the right.
  ctx.fillStyle = hull;
  ctx.beginPath();
  ctx.moveTo(-w / 2 - 4, -2.5);
  ctx.lineTo(w / 2 + 1, 0);
  ctx.lineTo(w / 2 - 2, depth);
  ctx.lineTo(-w / 2 + 6, depth);
  ctx.closePath();
  ctx.fill();
  // The deck strip, and a rope fender along the side like the knots in tug's logo.
  ctx.fillStyle = p.deck;
  ctx.fillRect(-w / 2, -0.8, w, 1.6);
  ctx.strokeStyle = p.rope;
  ctx.lineWidth = 0.9;
  ctx.setLineDash([0.9, 0.6]);
  ctx.beginPath();
  ctx.moveTo(-w / 2 + 2, 3.2);
  ctx.lineTo(w / 2 - 1, 3.2);
  ctx.stroke();
  ctx.setLineDash([]);
  // The wheelhouse at the stern, clear of the stack.
  ctx.fillStyle = p.cabin;
  ctx.fillRect(w / 2 - 7, -7, 6, 6.2);
  ctx.fillStyle = p.deck;
  ctx.fillRect(w / 2 - 6, -5.8, 4, 1.8);
  ctx.fillStyle = p.ink;
  ctx.fillRect(w / 2 - 4.5, -10, 1.2, 3);
}

function drawContainer(ctx: CanvasRenderingContext2D, p: StackPalette, kind: Kind, cx: number, y: number, w: number) {
  if (w <= 0) return;
  const x = cx - w / 2;
  ctx.fillStyle = colour(p, kind);
  ctx.fillRect(x, y, w, BOX_H);
  // Corrugated sides, a darker sill, and a light top edge.
  ctx.globalAlpha = 0.18;
  ctx.fillStyle = p.ink;
  ctx.fillRect(x, y + BOX_H - 1, w, 1);
  for (let gx = x + 1.6; gx < x + w - 0.6; gx += 1.6) ctx.fillRect(gx, y + 1, 0.35, BOX_H - 2.2);
  ctx.globalAlpha = 0.35;
  ctx.fillStyle = p.foam;
  ctx.fillRect(x, y, w, 0.5);
  ctx.globalAlpha = 1;
  if (kind === "gold" && w > 6) {
    // A milestone container: a star on its side.
    ctx.fillStyle = p.deck;
    star(ctx, cx, y + BOX_H / 2, 2);
  }
}

function star(ctx: CanvasRenderingContext2D, x: number, y: number, r: number) {
  ctx.beginPath();
  for (let i = 0; i < 10; i++) {
    const a = -Math.PI / 2 + (i * Math.PI) / 5;
    const rr = i % 2 ? r * 0.45 : r;
    ctx.lineTo(x + Math.cos(a) * rr, y + Math.sin(a) * rr);
  }
  ctx.closePath();
  ctx.fill();
}

function drawCrane(ctx: CanvasRenderingContext2D, s: Stack, view: StackView, p: StackPalette, lane: StackLane, left: number, right: number, stackTop: number) {
  // The rail: a girder with its lattice.
  ctx.fillStyle = p.steelDark;
  ctx.fillRect(left, RAIL_Y - 2, right - left, 4);
  ctx.strokeStyle = p.steel;
  ctx.lineWidth = 0.4;
  ctx.beginPath();
  for (let x = Math.floor(left / 4) * 4; x < right; x += 4) {
    ctx.moveTo(x, RAIL_Y - 2);
    ctx.lineTo(x + 2, RAIL_Y + 2);
    ctx.lineTo(x + 4, RAIL_Y - 2);
  }
  ctx.stroke();

  // A windsock at the end of the rail: it lifts before a gust and points the way it'll blow.
  const warn = gustWarning(s.seed, s.t, s.blocks.length);
  const lean = s.weather.lean;
  const dir = warn !== 0 ? warn : lean > 0 ? 1 : lean < 0 ? -1 : 1;
  const strength = warn !== 0 ? 1 : Math.min(1, s.weather.wind);
  const sockX = FIELD_W - 6;
  ctx.fillStyle = p.steel;
  ctx.fillRect(sockX - 0.3, RAIL_Y - 8, 0.6, 6);
  ctx.fillStyle = warn !== 0 ? p.coral : p.deck;
  ctx.beginPath();
  const len = 3 + 4 * strength;
  const droop = (1 - strength) * 4;
  const wave = view.reducedMotion ? 0 : Math.sin(s.t * 9) * 0.4 * strength;
  ctx.moveTo(sockX, RAIL_Y - 8);
  ctx.lineTo(sockX + dir * len, RAIL_Y - 7.2 + droop + wave);
  ctx.lineTo(sockX + dir * len, RAIL_Y - 6.4 + droop + wave);
  ctx.lineTo(sockX, RAIL_Y - 5.8);
  ctx.closePath();
  ctx.fill();

  const c = s.crane;
  const tx = c ? craneX(c.s) : (s.falling?.x ?? lane.trolleyX);
  // The trolley.
  ctx.fillStyle = p.steel;
  ctx.fillRect(tx - 3.5, RAIL_Y - 3, 7, 5);
  ctx.fillStyle = p.ink;
  ctx.fillRect(tx - 2.5, RAIL_Y + 2, 1.4, 1);
  ctx.fillRect(tx + 1.1, RAIL_Y + 2, 1.4, 1);
  if (!c) return;
  // The container on its hook, its underside `CRANE_GAP` above the top of the stack.
  const boxTop = stackTop - CRANE_GAP - BOX_H;
  ctx.strokeStyle = p.rope;
  ctx.lineWidth = 0.35;
  ctx.beginPath();
  ctx.moveTo(tx, RAIL_Y + 2);
  ctx.lineTo(tx, boxTop - 3);
  ctx.moveTo(tx, boxTop - 3);
  ctx.lineTo(tx - c.w / 2 + 0.5, boxTop);
  ctx.moveTo(tx, boxTop - 3);
  ctx.lineTo(tx + c.w / 2 - 0.5, boxTop);
  ctx.stroke();
  drawContainer(ctx, p, c.kind, tx, boxTop, c.w);
  // The crane won't wait forever: a ring runs down over the last few seconds.
  const left_ = HOLD_S - c.held;
  if (left_ < 4) {
    ctx.strokeStyle = p.coral;
    ctx.lineWidth = 0.8;
    ctx.beginPath();
    ctx.arc(tx, boxTop - 3, 2.2, -Math.PI / 2, -Math.PI / 2 + (Math.max(0, left_) / 4) * Math.PI * 2);
    ctx.stroke();
  }
}

/** At night: a lighthouse on the far breakwater, its beam sweeping slowly across the sky (still,
 * with reduced motion). */
function drawLighthouse(ctx: CanvasRenderingContext2D, t: number, reducedMotion: boolean, p: StackPalette, left: number, water: number, right: number) {
  const x = Math.max(left + 8, -2);
  const base = water - 1;
  const sweep = reducedMotion ? 0.25 : Math.sin(t * 0.5) * 0.9;
  const angle = -Math.PI / 2 + 1.1 + sweep * 0.8;
  ctx.save();
  ctx.fillStyle = p.beam;
  ctx.beginPath();
  ctx.moveTo(x, base - 22);
  const reach = Math.max(120, right - left);
  ctx.lineTo(x + Math.cos(angle - 0.08) * reach, base - 22 + Math.sin(angle - 0.08) * reach);
  ctx.lineTo(x + Math.cos(angle + 0.08) * reach, base - 22 + Math.sin(angle + 0.08) * reach);
  ctx.closePath();
  ctx.fill();
  ctx.fillStyle = p.tower;
  ctx.beginPath();
  ctx.moveTo(x - 3, base);
  ctx.lineTo(x - 1.8, base - 20);
  ctx.lineTo(x + 1.8, base - 20);
  ctx.lineTo(x + 3, base);
  ctx.closePath();
  ctx.fill();
  ctx.fillStyle = p.coral;
  ctx.fillRect(x - 2.5, base - 8, 5, 2);
  ctx.fillRect(x - 2.1, base - 15, 4.2, 2);
  ctx.fillStyle = p.lamp;
  ctx.beginPath();
  ctx.arc(x, base - 22, 1.6, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
}

/** A milestone: bunting along the crane's rail and confetti drifting down (bunting only, with
 * reduced motion). */
function drawCheer(ctx: CanvasRenderingContext2D, age: number, reducedMotion: boolean, p: StackPalette, left: number, right: number) {
  const fade = Math.min(1, (2.6 - age) / 0.5);
  ctx.globalAlpha = Math.max(0, fade);
  const flags = [p.coral, p.teal, p.amber, p.green];
  for (let x = Math.floor(left / 5) * 5, i = 0; x < right; x += 5, i++) {
    const sway = reducedMotion ? 0 : Math.sin(age * 6 + i) * 0.5;
    ctx.fillStyle = flags[((i % 4) + 4) % 4];
    ctx.beginPath();
    ctx.moveTo(x, RAIL_Y + 2);
    ctx.lineTo(x + 4, RAIL_Y + 2);
    ctx.lineTo(x + 2 + sway, RAIL_Y + 6);
    ctx.closePath();
    ctx.fill();
  }
  if (!reducedMotion) {
    for (let i = 0; i < 40; i++) {
      const x = left + hash(i * 5) * (right - left) + Math.sin(age * 3 + i) * 2;
      const y = RAIL_Y + 4 + age * (24 + hash(i * 5 + 1) * 30);
      ctx.fillStyle = flags[i % 4];
      ctx.save();
      ctx.translate(x, y);
      ctx.rotate(age * 5 + i);
      ctx.fillRect(-0.6, -0.3, 1.2, 0.6);
      ctx.restore();
    }
  }
  ctx.globalAlpha = 1;
}
