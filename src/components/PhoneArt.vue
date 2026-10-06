<script setup lang="ts">
// tug's own drawing of the user's iPhone, front view: a neutral metal frame, black glass and a
// screen with a soft tug-coloured wallpaper. One of three faces (home button, notch, Dynamic
// Island) in three sizes, from lib/phoneModel. Deliberately simple and original — no product
// photos or renders. Colour isn't known over Bluetooth, so the finish is always neutral.
// Every colour is a theme token (src/style.css), so it sits on the dark sidebar and in Settings.
import { computed, useId } from "vue";
import type { PhoneFace, PhoneSize } from "../lib/phoneModel";

const props = withDefaults(defineProps<{ face: PhoneFace; size: PhoneSize; height?: number; lit?: boolean }>(), {
  height: 56,
  lit: true,
});

const uid = useId();
const id = (name: string) => `phone-${name}-${uid}`;

// Real body proportions (mm) and corner radius per size class, drawn to scale against the largest
// so a mini looks smaller than a Max in the same slot.
const BODY: Record<"homeButton" | "fullScreen", Record<PhoneSize, [w: number, h: number, r: number]>> = {
  homeButton: { mini: [67.3, 138.4, 9.5], regular: [67.3, 138.4, 9.5], large: [78.1, 158.4, 10.5] },
  fullScreen: { mini: [64.2, 131.5, 9.5], regular: [71.5, 147.6, 11], large: [77.6, 160.9, 12.5] },
};
const VIEW_W = 64;
const VIEW_H = 128;
const SCALE = 124 / 160.9; // viewBox units per mm: the largest phone fills the height
/** The metal band seen from the front, and the black glass around the screen (viewBox units).
 *  Thicker than life so they still read at sidebar size. */
const BAND = 2.4;
const BEZEL = 2.2;

const g = computed(() => {
  const home = props.face === "homeButton";
  const [mmW, mmH, mmR] = BODY[home ? "homeButton" : "fullScreen"][props.size];
  const w = mmW * SCALE;
  const h = mmH * SCALE;
  const x = (VIEW_W - w) / 2;
  const y = (VIEW_H - h) / 2;
  const r = mmR * SCALE;
  // Screen: edge to edge behind a thin bezel, or between the home-button phone's tall bezels.
  const sideBezel = home ? 3.4 : BEZEL;
  const endBezel = home ? h * 0.12 : BEZEL;
  const sx = x + BAND + sideBezel;
  const sy = y + BAND + endBezel;
  const sw = w - 2 * (BAND + sideBezel);
  const sh = h - 2 * (BAND + endBezel);
  const sr = home ? 0.8 : Math.max(r - BAND - BEZEL, 1);
  return { home, x, y, w, h, r, sx, sy, sw, sh, sr, endBezel };
});

// Side buttons: action/mute and volume on the left, side button on the right.
const buttons = computed(() => {
  const { x, y, w, h } = g.value;
  const left = [
    [0.17, 0.045],
    [0.26, 0.085],
    [0.36, 0.085],
  ].map(([at, len]) => ({ x: x - 0.9, y: y + h * at, h: h * len }));
  const right = [{ x: x + w - 0.1, y: y + h * 0.28, h: h * 0.13 }];
  return [...left, ...right];
});

const island = computed(() => {
  const { sx, sy, sw, sh } = g.value;
  const iw = sw * 0.3;
  const ih = Math.max(sh * 0.038, 2.6);
  return { x: sx + (sw - iw) / 2, y: sy + sh * 0.014, w: iw, h: ih };
});

const notch = computed(() => {
  const { sx, sy, sw, sh } = g.value;
  const nw = sw * 0.48;
  const nh = Math.max(sh * 0.036, 2.6);
  // Starts inside the black bezel above the screen, so only its rounded lower edge shows.
  return { x: sx + (sw - nw) / 2, y: sy - BEZEL, w: nw, h: nh + BEZEL };
});

const home = computed(() => {
  const { x, y, w, h, endBezel } = g.value;
  const cy = y + h - BAND - endBezel / 2;
  return {
    cx: x + w / 2,
    cy,
    r: endBezel * 0.33,
    speaker: { x: x + w / 2 - w * 0.09, y: y + BAND + endBezel / 2 - 0.6, w: w * 0.18 },
    camera: { cx: x + w / 2 - w * 0.17, cy: y + BAND + endBezel / 2 },
  };
});
</script>

<template>
  <svg
    :width="height / 2"
    :height="height"
    :viewBox="`0 0 ${VIEW_W} ${VIEW_H}`"
    aria-hidden="true"
    class="shrink-0 overflow-visible"
  >
    <defs>
      <!-- Neutral brushed metal, lit from the top left. -->
      <linearGradient :id="id('metal')" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" style="stop-color: var(--color-hairline)" />
        <stop offset="0.4" style="stop-color: var(--color-on-dark-soft)" />
        <stop offset="1" style="stop-color: var(--color-muted)" />
      </linearGradient>
      <linearGradient :id="id('wall')" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" style="stop-color: var(--color-surface-dark-elevated)" />
        <stop offset="1" style="stop-color: var(--color-surface-dark)" />
      </linearGradient>
      <radialGradient :id="id('glow-a')" cx="0.22" cy="0.3" r="0.75">
        <stop offset="0" style="stop-color: var(--color-primary); stop-opacity: 0.6" />
        <stop offset="1" style="stop-color: var(--color-primary); stop-opacity: 0" />
      </radialGradient>
      <radialGradient :id="id('glow-b')" cx="0.85" cy="0.72" r="0.7">
        <stop offset="0" style="stop-color: var(--color-accent-amber); stop-opacity: 0.45" />
        <stop offset="1" style="stop-color: var(--color-accent-amber); stop-opacity: 0" />
      </radialGradient>
      <radialGradient :id="id('glow-c')" cx="0.15" cy="0.98" r="0.5">
        <stop offset="0" style="stop-color: var(--color-accent-teal); stop-opacity: 0.3" />
        <stop offset="1" style="stop-color: var(--color-accent-teal); stop-opacity: 0" />
      </radialGradient>
      <!-- A faint diagonal sheen across the glass. -->
      <linearGradient :id="id('sheen')" x1="0" y1="0" x2="1" y2="0.6">
        <stop offset="0" style="stop-color: var(--color-on-dark); stop-opacity: 0.14" />
        <stop offset="0.45" style="stop-color: var(--color-on-dark); stop-opacity: 0.03" />
        <stop offset="0.46" style="stop-color: var(--color-on-dark); stop-opacity: 0" />
      </linearGradient>
    </defs>

    <!-- Side buttons sit behind the frame. -->
    <rect v-for="(b, i) in buttons" :key="i" :x="b.x" :y="b.y" width="1" :height="b.h" rx="0.5" style="fill: var(--color-muted)" />

    <!-- Frame, then the black glass front. -->
    <rect
      :x="g.x"
      :y="g.y"
      :width="g.w"
      :height="g.h"
      :rx="g.r"
      :fill="`url(#${id('metal')})`"
      style="stroke: var(--color-muted); stroke-opacity: 0.6"
      stroke-width="0.5"
    />
    <rect :x="g.x + BAND" :y="g.y + BAND" :width="g.w - 2 * BAND" :height="g.h - 2 * BAND" :rx="Math.max(g.r - BAND, 1)" style="fill: var(--color-ink)" />

    <!-- Screen: tug's warm wallpaper when connected, dark when the phone is away. -->
    <rect :x="g.sx" :y="g.sy" :width="g.sw" :height="g.sh" :rx="g.sr" :fill="`url(#${id('wall')})`" />
    <template v-if="lit">
      <rect :x="g.sx" :y="g.sy" :width="g.sw" :height="g.sh" :rx="g.sr" :fill="`url(#${id('glow-a')})`" />
      <rect :x="g.sx" :y="g.sy" :width="g.sw" :height="g.sh" :rx="g.sr" :fill="`url(#${id('glow-b')})`" />
      <rect :x="g.sx" :y="g.sy" :width="g.sw" :height="g.sh" :rx="g.sr" :fill="`url(#${id('glow-c')})`" />
    </template>

    <!-- The face: Dynamic Island, notch, or home button with its speaker and camera. -->
    <rect v-if="face === 'island'" :x="island.x" :y="island.y" :width="island.w" :height="island.h" :rx="island.h / 2" style="fill: var(--color-ink)" />
    <rect
      v-else-if="face === 'notch'"
      :x="notch.x"
      :y="notch.y"
      :width="notch.w"
      :height="notch.h"
      :rx="Math.min(notch.h * 0.45, 3)"
      style="fill: var(--color-ink)"
    />
    <template v-else>
      <circle :cx="home.cx" :cy="home.cy" :r="home.r" fill="none" style="stroke: var(--color-muted)" stroke-width="0.8" />
      <rect :x="home.speaker.x" :y="home.speaker.y" :width="home.speaker.w" height="1.2" rx="0.6" style="fill: var(--color-surface-dark-elevated)" />
      <circle :cx="home.camera.cx" :cy="home.camera.cy" r="0.9" style="fill: var(--color-surface-dark-elevated)" />
    </template>

    <!-- Glass sheen over everything on the front. -->
    <rect :x="g.x + BAND" :y="g.y + BAND" :width="g.w - 2 * BAND" :height="g.h - 2 * BAND" :rx="Math.max(g.r - BAND, 1)" :fill="`url(#${id('sheen')})`" />
  </svg>
</template>
