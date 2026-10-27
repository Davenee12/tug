<script setup lang="ts">
// The weather card's harbor: a live little scene of the weather where you are (lib/weatherHarbor
// works it out, weatherHarborDraw draws it). It moves only while you can see it: paused while tug
// is hidden or the card is scrolled away, and one still frame with reduced motion. About 30 frames
// a second, which is plenty for waves and keeps it light.
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import type { Forecast } from "../lib/weather";
import { describe as describeSky } from "../lib/weather";
import { harborScene, placeNow, seaLine, skyCaption } from "../lib/weatherHarbor";
import { drawHarbor, type HarborPalette } from "./weatherHarborDraw";
import { readSwatches } from "./gamePalette";

const props = defineProps<{
  /** The forecast with the current hour applied. */
  forecast: Forecast;
  /** The minute clock the card already keeps. */
  now: Date;
  /** The place is south of the equator (the sky turns round). */
  south: boolean;
}>();

const scene = computed(() => harborScene(props.forecast, props.now.getTime()));
const caption = computed(() => skyCaption(scene.value, props.forecast, placeNow(props.now.getTime(), props.forecast.timezone)));
const sea = computed(() => seaLine(scene.value.wind));
const label = computed(() =>
  [describeSky(props.forecast.now.code).label, caption.value, sea.value].filter(Boolean).join(". "),
);
const night = computed(() => !scene.value.sun);

const stage = ref<HTMLElement | null>(null);
const canvas = ref<HTMLCanvasElement | null>(null);
const swatches = ref<HTMLElement | null>(null);
let ctx: CanvasRenderingContext2D | null = null;
let day: HarborPalette | null = null;
let dark: HarborPalette | null = null;
let size = { width: 1, height: 1, dpr: 1 };
const reducedMotion = ref(false);
let onScreen = true;
let raf = 0;
let last = 0;
const t0 = performance.now();

const NAMES = [
  "base",
  "skyTop",
  "skyLow",
  "glow",
  "glowFade",
  "overcast",
  "sun",
  "halo",
  "haloFade",
  "moon",
  "moonDark",
  "star",
  "shoreFar",
  "shore",
  "lit",
  "cloud",
  "cloudDark",
  "fog",
  "water",
  "waterDeep",
  "foam",
  "hull",
  "hullDark",
  "cabin",
  "window",
  "funnel",
  "smoke",
  "flag",
  "rain",
  "flake",
  "flash",
  "beam",
  "lamp",
] as const;

function draw(ms: number) {
  if (!ctx || !day || !dark) return;
  // Moved to a screen with another scale: resize (which draws) so the harbor stays sharp.
  if ((window.devicePixelRatio || 1) !== size.dpr) return resize();
  drawHarbor(ctx, {
    ...size,
    t: reducedMotion.value ? 4.2 : (ms - t0) / 1000,
    scene: scene.value,
    palette: night.value ? dark : day,
    south: props.south,
    reducedMotion: reducedMotion.value,
  });
}

function frame(ms: number) {
  raf = requestAnimationFrame(frame);
  if (ms - last < 33) return;
  last = ms;
  draw(ms);
}

function running() {
  return !reducedMotion.value && onScreen && !document.hidden;
}

function play() {
  if (raf || !running()) return;
  raf = requestAnimationFrame(frame);
}

function pause() {
  cancelAnimationFrame(raf);
  raf = 0;
}

/** Start or stop as things change; a still frame whenever it isn't moving. */
function update() {
  if (running()) play();
  else {
    pause();
    draw(performance.now());
  }
}

function resize() {
  const el = stage.value;
  const c = canvas.value;
  if (!el || !c) return;
  const r = el.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  size = { width: Math.max(1, r.width), height: Math.max(1, r.height), dpr };
  c.width = Math.max(1, Math.round(size.width * dpr));
  c.height = Math.max(1, Math.round(size.height * dpr));
  draw(performance.now());
}

let ro: ResizeObserver | undefined;
let io: IntersectionObserver | undefined;
let motion: MediaQueryList | undefined;
const onMotion = () => {
  reducedMotion.value = !!motion?.matches;
  update();
};

onMounted(() => {
  ctx = canvas.value?.getContext("2d") ?? null;
  day = readSwatches(swatches.value, "day", NAMES);
  dark = readSwatches(swatches.value, "night", NAMES);
  motion = window.matchMedia?.("(prefers-reduced-motion: reduce)");
  reducedMotion.value = !!motion?.matches;
  motion?.addEventListener?.("change", onMotion);
  ro = new ResizeObserver(resize);
  if (stage.value) ro.observe(stage.value);
  if (typeof IntersectionObserver !== "undefined" && stage.value) {
    io = new IntersectionObserver((entries) => {
      // Several can queue up in a quick scroll: the newest says where it is now.
      onScreen = entries[entries.length - 1]?.isIntersecting ?? true;
      update();
    });
    io.observe(stage.value);
  }
  document.addEventListener("visibilitychange", update);
  resize();
  update();
});

onUnmounted(() => {
  pause();
  ro?.disconnect();
  io?.disconnect();
  motion?.removeEventListener?.("change", onMotion);
  document.removeEventListener("visibilitychange", update);
});

// A new forecast or minute: the loop picks it up; a still harbor needs its frame redrawn.
watch([scene, () => props.south], () => {
  if (!raf) draw(performance.now());
});
</script>

<template>
  <div ref="stage" class="relative h-24 w-full overflow-hidden" data-toggle>
    <canvas ref="canvas" class="absolute inset-0 size-full" role="img" :aria-label="label" />
    <div
      class="pointer-events-none absolute inset-x-0 top-0 flex items-start justify-between gap-3 px-4 pt-2 text-[11px] font-medium"
      :class="night ? 'text-on-dark-soft' : 'text-body'"
      aria-hidden="true"
    >
      <span class="truncate">{{ caption }}</span>
      <span class="truncate text-right">{{ sea }}</span>
    </div>

    <!-- tug's colours for the canvas (theme tokens only, no colour in code). Night swatches replace
         the day ones they name; the rest are shared. -->
    <div ref="swatches" class="hidden" aria-hidden="true">
      <span data-swatch="day:base" class="bg-canvas" />
      <span data-swatch="day:skyTop" class="bg-accent-teal/35" />
      <span data-swatch="day:skyLow" class="bg-surface-soft" />
      <span data-swatch="day:glow" class="bg-accent-amber/60" />
      <span data-swatch="day:glowFade" class="bg-accent-amber/0" />
      <span data-swatch="day:overcast" class="bg-surface-cream-strong" />
      <span data-swatch="day:sun" class="bg-accent-amber" />
      <span data-swatch="day:halo" class="bg-accent-amber/35" />
      <span data-swatch="day:haloFade" class="bg-accent-amber/0" />
      <span data-swatch="day:moon" class="bg-on-dark" />
      <span data-swatch="day:moonDark" class="bg-on-dark/10" />
      <span data-swatch="day:star" class="bg-on-dark" />
      <span data-swatch="day:shoreFar" class="bg-surface-card" />
      <span data-swatch="day:shore" class="bg-surface-cream-strong" />
      <span data-swatch="day:lit" class="bg-accent-amber" />
      <span data-swatch="day:cloud" class="bg-canvas" />
      <span data-swatch="day:cloudDark" class="bg-muted-soft" />
      <span data-swatch="day:fog" class="bg-canvas/80" />
      <span data-swatch="day:water" class="bg-accent-teal/45" />
      <span data-swatch="day:waterDeep" class="bg-accent-teal/75" />
      <span data-swatch="day:foam" class="bg-canvas" />
      <span data-swatch="day:hull" class="bg-primary" />
      <span data-swatch="day:hullDark" class="bg-primary-active" />
      <span data-swatch="day:cabin" class="bg-canvas" />
      <span data-swatch="day:window" class="bg-surface-dark" />
      <span data-swatch="day:funnel" class="bg-ink" />
      <span data-swatch="day:smoke" class="bg-muted/50" />
      <span data-swatch="day:flag" class="bg-accent-amber" />
      <span data-swatch="day:rain" class="bg-surface-dark/35" />
      <span data-swatch="day:flake" class="bg-canvas" />
      <span data-swatch="day:flash" class="bg-canvas" />
      <span data-swatch="day:beam" class="bg-accent-amber/20" />
      <span data-swatch="day:lamp" class="bg-accent-amber" />
      <span data-swatch="night:base" class="bg-surface-dark" />
      <span data-swatch="night:skyTop" class="bg-surface-dark" />
      <span data-swatch="night:skyLow" class="bg-surface-dark-elevated" />
      <span data-swatch="night:glow" class="bg-primary/45" />
      <span data-swatch="night:glowFade" class="bg-primary/0" />
      <span data-swatch="night:overcast" class="bg-surface-dark-elevated" />
      <span data-swatch="night:halo" class="bg-on-dark/15" />
      <span data-swatch="night:haloFade" class="bg-on-dark/0" />
      <span data-swatch="night:shoreFar" class="bg-surface-dark-soft" />
      <span data-swatch="night:shore" class="bg-surface-dark" />
      <span data-swatch="night:cloud" class="bg-muted/45" />
      <span data-swatch="night:cloudDark" class="bg-surface-dark-elevated" />
      <span data-swatch="night:fog" class="bg-on-dark-soft/30" />
      <span data-swatch="night:water" class="bg-accent-teal/20" />
      <span data-swatch="night:waterDeep" class="bg-accent-teal/35" />
      <span data-swatch="night:foam" class="bg-on-dark/60" />
      <span data-swatch="night:cabin" class="bg-on-dark-soft" />
      <span data-swatch="night:smoke" class="bg-on-dark-soft/35" />
      <span data-swatch="night:rain" class="bg-on-dark/40" />
      <span data-swatch="night:flake" class="bg-on-dark/85" />
      <span data-swatch="night:flash" class="bg-on-dark/80" />
    </div>
  </div>
</template>
