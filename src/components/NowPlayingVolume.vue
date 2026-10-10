<script setup lang="ts">
// The phone's volume as a slim bar on the Now Playing card: shows the level the phone reports, and
// click/drag, the mouse wheel and the arrow keys change it. AMS only has one-step VolumeUp and
// VolumeDown, so a new level is reached step by step; the pacing and planning live in lib/volume
// (pure, unit-tested) and this just wires DOM events to it. Nothing ticks here: the fill moves only
// when the phone reports a new volume, and the seeker's short wait timer runs only while stepping.
import { computed, onUnmounted, ref, watch } from "vue";
import { useTugStore } from "../stores/tug";
import { seekFraction } from "../lib/spotify";
import { createVolumeSeeker, keyAction, volumePercent, wheelDirection } from "../lib/volume";

const props = defineProps<{ volume: number; adjustable: boolean }>();

const tug = useTugStore();

/** The level being stepped toward, marked on the bar until the phone gets there (null when idle). */
const target = ref<number | null>(null);
const seeker = createVolumeSeeker({
  send: (direction) => tug.media(direction > 0 ? "volumeUp" : "volumeDown"),
  timers: { set: (fn, ms) => window.setTimeout(fn, ms), clear: (h) => window.clearTimeout(h) },
  onTarget: (t) => (target.value = t),
});
watch(
  () => props.volume,
  (v) => seeker.report(v),
  { immediate: true },
);
onUnmounted(() => seeker.cancel());
// The card's − and + buttons take over from a seek in progress.
defineExpose({ cancel: () => seeker.cancel() });

const percent = computed(() => volumePercent(props.volume));
// The fill is a full-width bar slid left (a transform, not a width) and eases only when the level
// changes, so nothing is drawn while the volume stays put.
const fill = computed(() => `translateX(${Math.min(1, Math.max(0, props.volume)) * 100 - 100}%)`);

const bar = ref<HTMLElement | null>(null);
let dragging = false;
function fractionAt(e: PointerEvent): number {
  const rect = bar.value?.getBoundingClientRect();
  return rect ? seekFraction(e.clientX, rect) : props.volume;
}
function onDown(e: PointerEvent) {
  if (!props.adjustable || e.button !== 0) return;
  (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
  dragging = true;
  seeker.seek(fractionAt(e));
}
// Dragging only moves the goal; the seeker still sends one step at a time.
function onMove(e: PointerEvent) {
  if (dragging) seeker.seek(fractionAt(e));
}
function onUp() {
  dragging = false;
}
function onWheel(e: WheelEvent) {
  if (!props.adjustable) return;
  const direction = wheelDirection(e.deltaY);
  if (direction === 0) return;
  e.preventDefault(); // the wheel changes the volume here, it doesn't scroll the sidebar
  seeker.nudge(direction);
}
function onKey(e: KeyboardEvent) {
  const action = keyAction(e.key);
  if (!action) return;
  e.preventDefault();
  if (!props.adjustable) return;
  // Separate presses add up (three taps, three steps); a held key's auto-repeat doesn't race ahead.
  if ("nudge" in action) seeker.nudge(action.nudge, !e.repeat);
  else seeker.seek(action.to);
}
</script>

<template>
  <!-- A 4 px bar with a taller hit area; the negative margin keeps the row as slim as the bar. -->
  <div
    ref="bar"
    role="slider"
    aria-label="Volume"
    aria-valuemin="0"
    aria-valuemax="100"
    :aria-valuenow="percent"
    :aria-valuetext="`${percent}%`"
    :aria-disabled="adjustable ? undefined : 'true'"
    :tabindex="adjustable ? 0 : -1"
    :title="`Volume ${percent}%`"
    :class="[
      'relative -my-2 min-w-0 flex-1 touch-none rounded-full py-2 outline-none focus-visible:ring-2 focus-visible:ring-on-dark/30',
      adjustable ? 'cursor-pointer' : '',
    ]"
    @pointerdown="onDown"
    @pointermove="onMove"
    @pointerup="onUp"
    @pointercancel="onUp"
    @lostpointercapture="onUp"
    @wheel="onWheel"
    @keydown="onKey"
  >
    <div class="h-1 overflow-hidden rounded-full bg-surface-dark-soft">
      <div
        class="h-full w-full rounded-full bg-on-dark transition-transform duration-150 ease-out motion-reduce:transition-none"
        :style="{ transform: fill }"
      />
    </div>
    <span
      v-if="target != null"
      class="pointer-events-none absolute top-1/2 h-2.5 w-0.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-on-dark-soft"
      :style="{ left: `${target * 100}%` }"
      aria-hidden="true"
    />
  </div>
</template>
