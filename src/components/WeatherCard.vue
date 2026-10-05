<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch, type Component } from "vue";
import {
  Cloud,
  CloudDrizzle,
  CloudFog,
  CloudLightning,
  CloudMoon,
  CloudRain,
  CloudSnow,
  CloudSun,
  LocateFixed,
  MapPin,
  Moon,
  RotateCcw,
  Sun,
} from "lucide-vue-next";
import { useWeatherStore } from "../stores/weather";
import { dayLabel, describe, hourLabel, outlook, searchPlaces, temp, wind, type Place, type Sky } from "../lib/weather";
import { relativeTime } from "../lib/format";

const w = useWeatherStore();

const ICONS: Record<Sky, [Component, Component]> = {
  clear: [Sun, Moon],
  partly: [CloudSun, CloudMoon],
  cloudy: [Cloud, Cloud],
  fog: [CloudFog, CloudFog],
  drizzle: [CloudDrizzle, CloudDrizzle],
  rain: [CloudRain, CloudRain],
  snow: [CloudSnow, CloudSnow],
  storm: [CloudLightning, CloudLightning],
};
const icon = (code: number, day = true) => ICONS[describe(code).sky][day ? 0 : 1];

const f = computed(() => w.forecast);
const t = (c: number) => `${temp(c, w.unit)}°`;
const today = computed(() => f.value?.days[0] ?? null);
const line = computed(() => (f.value ? outlook(f.value) : null));
const place = computed(() => (w.place && w.place !== "off" ? w.place : null));
const updated = computed(() => (f.value ? relativeTime(new Date(f.value.fetchedAt)) : ""));

// Week temperature range, for the iPhone-style bars.
const range = computed(() => {
  const days = f.value?.days ?? [];
  const lo = Math.min(...days.map((d) => d.low));
  const hi = Math.max(...days.map((d) => d.high));
  return { lo, span: Math.max(hi - lo, 1) };
});
const bar = (low: number, high: number) => ({
  left: `${((low - range.value.lo) / range.value.span) * 100}%`,
  width: `${Math.max(((high - low) / range.value.span) * 100, 4)}%`,
});
const nowDot = computed(() => (f.value ? `${((f.value.now.temp - range.value.lo) / range.value.span) * 100}%` : "0"));

// ---- Open / close: drag the card down for more, up for less (or just click it) ----
const open = ref(false);
const inner = ref<HTMLElement | null>(null);
const full = ref(0);
const dragH = ref<number | null>(null);
let startY = 0;
let startH = 0;
let moved = false;
let ro: ResizeObserver | undefined;

onMounted(() => {
  ro = new ResizeObserver(() => (full.value = inner.value?.scrollHeight ?? 0));
  watch(
    inner,
    (el, old) => {
      if (old) ro?.unobserve(old);
      if (el) ro?.observe(el);
    },
    { immediate: true },
  );
});
onUnmounted(() => ro?.disconnect());

const height = computed(() => (dragH.value !== null ? dragH.value : open.value ? full.value : 0));

function down(e: PointerEvent) {
  if (e.button !== 0) return;
  startY = e.clientY;
  startH = open.value ? full.value : 0;
  moved = false;
  dragH.value = startH;
  (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
}
function move(e: PointerEvent) {
  if (dragH.value === null) return;
  const dy = e.clientY - startY;
  if (Math.abs(dy) > 4) moved = true;
  // A little give past the ends so it feels physical.
  const raw = startH + dy;
  dragH.value = raw < 0 ? raw / 4 : raw > full.value ? full.value + (raw - full.value) / 4 : raw;
}
function up() {
  if (dragH.value === null) return;
  const h = dragH.value;
  if (!moved) open.value = !open.value;
  else open.value = open.value ? h > full.value * 0.75 : h > full.value * 0.25;
  dragH.value = null;
}
function onKey(e: KeyboardEvent) {
  if (e.key === "Enter" || e.key === " ") {
    e.preventDefault();
    open.value = !open.value;
  } else if (e.key === "ArrowDown") open.value = true;
  else if (e.key === "ArrowUp") open.value = false;
}

/** The hours row scrolls sideways with a normal mouse wheel. */
function sideways(e: WheelEvent) {
  const el = e.currentTarget as HTMLElement;
  if (Math.abs(e.deltaY) <= Math.abs(e.deltaX) || el.scrollWidth <= el.clientWidth) return;
  const atEnd = e.deltaY > 0 ? el.scrollLeft + el.clientWidth >= el.scrollWidth - 1 : el.scrollLeft <= 0;
  if (atEnd) return; // let the page scroll once the row is at its end
  e.preventDefault();
  el.scrollLeft += e.deltaY;
}

// ---- Set-up: your location or a city ----
const query = ref("");
const results = ref<Place[]>([]);
const setupError = ref<string | null>(null);
const locating = ref(false);
let seq = 0;
let debounce: number | undefined;
watch(query, (q) => {
  window.clearTimeout(debounce);
  if (q.trim().length < 2) {
    results.value = [];
    return;
  }
  debounce = window.setTimeout(async () => {
    const mine = ++seq;
    try {
      const r = await searchPlaces(q);
      if (mine === seq) results.value = r;
    } catch (e) {
      if (mine === seq) setupError.value = String(e instanceof Error ? e.message : e);
    }
  }, 250);
});
async function pick(p: Place) {
  query.value = "";
  results.value = [];
  setupError.value = null;
  await w.setPlace(p);
}
async function locate() {
  locating.value = true;
  setupError.value = await w.useMyLocation();
  locating.value = false;
}
async function change() {
  open.value = false;
  w.reset();
  await nextTick();
}
</script>

<template>
  <!-- Set up -->
  <section v-if="w.ready && w.place === null" class="rounded-xl bg-surface-card px-6 py-5">
    <p class="caption-upper text-muted">Weather</p>
    <p class="headline mt-1 text-[24px] leading-tight">Your forecast, right here</p>
    <p class="mt-1 max-w-md text-[13px] text-muted">
      Today at a glance; drag it open for the week. Only the place you choose is sent to the forecast service.
    </p>
    <div class="relative mt-4 flex flex-wrap items-center gap-2">
      <button class="btn-primary btn-sm" :disabled="locating" @click="locate">
        <LocateFixed :size="14" /> {{ locating ? "Finding you…" : "Use my location" }}
      </button>
      <div class="relative w-56">
        <input v-model="query" class="input h-9 text-[14px]" placeholder="or type a city" aria-label="City" spellcheck="false" />
        <ul
          v-if="results.length"
          class="absolute top-full right-0 left-0 z-20 mt-1 overflow-hidden rounded-lg border border-hairline bg-canvas py-1"
        >
          <li v-for="r in results" :key="`${r.latitude},${r.longitude}`">
            <button class="flex w-full items-center gap-2 px-3 py-2 text-left text-[13px] text-ink active:bg-surface-card" @click="pick(r)">
              <MapPin :size="13" class="shrink-0 text-muted-soft" /> {{ r.name }}
            </button>
          </li>
        </ul>
      </div>
      <button class="ml-auto text-[13px] text-muted active:text-ink" @click="w.hide()">Not now</button>
    </div>
    <p v-if="setupError" class="mt-2 text-[12px] text-error">{{ setupError }}</p>
  </section>

  <!-- Forecast -->
  <section v-else-if="place" class="overflow-hidden rounded-xl bg-surface-card select-none">
    <div
      role="button"
      tabindex="0"
      :aria-expanded="open"
      :aria-label="open ? 'Weather: show less' : 'Weather: show the week'"
      class="cursor-pointer touch-none px-6 pt-5 pb-2 outline-none focus-visible:ring-2 focus-visible:ring-ink/20"
      @pointerdown="down"
      @pointermove="move"
      @pointerup="up"
      @pointercancel="up"
      @keydown="onKey"
    >
      <!-- First load: quiet placeholder in the card's own shape -->
      <div v-if="!f && w.loading" class="flex animate-pulse items-center gap-4 pb-3">
        <div class="size-10 rounded-full bg-surface-cream-strong" />
        <div class="h-10 w-20 rounded-md bg-surface-cream-strong" />
        <div class="h-4 w-40 rounded bg-surface-cream-strong" />
      </div>

      <div v-else-if="!f" class="flex items-center gap-3 pb-3 text-[13px] text-muted">
        Can't reach the forecast right now.
        <button class="flex items-center gap-1 text-ink" @pointerdown.stop @click.stop="w.refresh(true)">
          <RotateCcw :size="12" /> Retry
        </button>
      </div>

      <div v-else class="flex items-center gap-4">
        <component :is="icon(f.now.code, f.now.day)" :size="38" :stroke-width="1.5" class="shrink-0 text-ink" />
        <div class="flex items-start gap-1.5">
          <p class="headline text-[46px] leading-none" style="letter-spacing: -0.03em">{{ temp(f.now.temp, w.unit) }}</p>
          <!-- Unit switch beside the number: one click, remembered. -->
          <span class="mt-1 flex gap-1 text-[13px]" @pointerdown.stop>
            <button
              v-for="u in ['f', 'c'] as const"
              :key="u"
              :class="w.unit === u ? 'font-semibold text-ink' : 'text-muted-soft active:text-ink'"
              :aria-pressed="w.unit === u"
              @click.stop="w.unit !== u && w.toggleUnit()"
            >
              °{{ u.toUpperCase() }}
            </button>
          </span>
        </div>
        <div class="min-w-0">
          <p class="text-[15px] font-medium text-ink">{{ describe(f.now.code).label }}</p>
          <p class="text-[13px] text-muted">
            <template v-if="today">H {{ t(today.high) }} · L {{ t(today.low) }} · </template>Feels {{ t(f.now.feels) }}
          </p>
        </div>
        <div class="ml-auto min-w-0 text-right">
          <p class="flex items-center justify-end gap-1 truncate text-[13px] text-muted">
            <MapPin :size="12" class="shrink-0" /> {{ place.name }}
          </p>
          <p v-if="line" class="mt-0.5 truncate text-[13px] font-medium text-body-strong">{{ line }}</p>
        </div>
      </div>

      <!-- Grabber: drag down for the week -->
      <div class="flex justify-center pt-3">
        <span :class="['h-1 w-9 rounded-full transition-colors', dragH !== null ? 'bg-muted-soft' : 'bg-hairline']" />
      </div>
    </div>

    <!-- More: next 24 hours and the week -->
    <div
      :style="{ height: `${height}px` }"
      :class="['overflow-hidden', dragH === null ? 'transition-[height] duration-300 ease-[cubic-bezier(.2,.8,.2,1)]' : '']"
      :aria-hidden="!open"
    >
      <div v-if="f" ref="inner" class="px-6 pt-2 pb-5">
        <ul class="-mx-2 flex gap-1 overflow-x-auto pb-2 [scrollbar-width:none]" @wheel="sideways">
          <li v-for="(h, i) in f.hours" :key="h.time" class="flex w-14 shrink-0 flex-col items-center gap-1.5 rounded-lg py-2">
            <span class="text-[11px] text-muted">{{ i === 0 ? "Now" : hourLabel(h.time) }}</span>
            <component :is="icon(h.code, h.day)" :size="18" :stroke-width="1.6" class="text-ink" />
            <span class="text-[13px] font-medium text-ink">{{ t(h.temp) }}</span>
            <span :class="['text-[10px] font-medium', (h.rain ?? 0) >= 30 ? 'text-accent-teal' : 'invisible']">{{ h.rain }}%</span>
          </li>
        </ul>

        <ul class="mt-2 border-t border-hairline pt-2">
          <li
            v-for="d in f.days"
            :key="d.date"
            class="grid grid-cols-[4.5rem_1.5rem_2.5rem_2.5rem_1fr_2.5rem] items-center gap-2 py-1.5 text-[13px]"
          >
            <span class="font-medium text-ink">{{ dayLabel(d.date, f.days[0].date) }}</span>
            <component :is="icon(d.code)" :size="17" :stroke-width="1.6" class="text-ink" />
            <span class="text-[11px] font-medium text-accent-teal">{{ (d.rain ?? 0) >= 30 ? `${d.rain}%` : "" }}</span>
            <span class="text-right text-muted">{{ t(d.low) }}</span>
            <span class="relative h-1 rounded-full bg-hairline">
              <span class="absolute inset-y-0 rounded-full bg-accent-amber" :style="bar(d.low, d.high)" />
              <span
                v-if="d.date === f.days[0].date"
                class="absolute top-1/2 size-2 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-surface-card bg-ink"
                :style="{ left: nowDot }"
              />
            </span>
            <span class="text-ink">{{ t(d.high) }}</span>
          </li>
        </ul>

        <div class="mt-3 flex flex-wrap items-center gap-x-3 gap-y-1 text-[12px] text-muted-soft">
          <span>Wind {{ wind(f.now.wind, w.unit) }}</span>
          <span>Humidity {{ f.now.humidity }}%</span>
          <span>{{ w.error ? `Offline · from ${updated}` : `Updated ${updated}` }}</span>
          <span class="ml-auto flex items-center gap-3">
            <button class="text-muted active:text-ink" @click="change">Change place</button>
            <button class="text-muted active:text-ink" @click="w.hide()">Hide</button>
          </span>
        </div>
        <p class="mt-1 text-[11px] text-muted-soft">Forecast by Open-Meteo</p>
      </div>
    </div>
  </section>
</template>
