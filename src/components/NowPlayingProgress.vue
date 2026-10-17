<script setup lang="ts">
// The Now Playing card's progress bar and times: the only part of the card that changes every
// second, so it ticks on its own (see lib/useNow) and the rest of the card stays still.
import { computed, onUnmounted, ref } from "vue";
import type { NowPlaying } from "../types/protocol";
import { duration } from "../lib/format";
import { elapsedSeconds, positionMoves, seekKeyFraction } from "../lib/media";
import { seekFraction } from "../lib/spotify";
import { useNow } from "../lib/useNow";

const props = defineProps<{ np: NowPlaying; seekable: boolean }>();
const emit = defineEmits<{ seek: [fraction: number] }>();

// Ticks only while the song plays and tug is on screen; paused or hidden, no timer runs.
const now = useNow(() => positionMoves(props.np), 1000);
const elapsed = computed(() => elapsedSeconds(props.np, now.value));

// Click/drag-to-seek: only when Spotify is the player and we know the song length. Otherwise the
// bar is a plain progress indicator.
const bar = ref<HTMLElement | null>(null);
/** The fraction a drag is currently at, so the fill follows the pointer before it commits. */
const dragFraction = ref<number | null>(null);
function fractionAt(e: PointerEvent): number {
  const rect = bar.value?.getBoundingClientRect();
  return rect ? seekFraction(e.clientX, rect) : 0;
}
function seekDown(e: PointerEvent) {
  if (!props.seekable) return;
  (e.target as HTMLElement).setPointerCapture?.(e.pointerId);
  dragFraction.value = fractionAt(e);
}
function seekMove(e: PointerEvent) {
  if (dragFraction.value == null) return;
  dragFraction.value = fractionAt(e);
}
function seekUp() {
  if (dragFraction.value == null) return;
  window.clearTimeout(keyTimer);
  const f = dragFraction.value;
  dragFraction.value = null;
  emit("seek", f);
}

// The keyboard, like the volume bar: arrows step 5 s, Page Up/Down 30 s, Home to the start. The
// fill moves with each press; a burst of presses becomes one seek once they stop, so holding an
// arrow doesn't send Spotify a request per auto-repeat.
const KEY_COMMIT_MS = 400;
let keyTimer: number | undefined;
function seekKey(e: KeyboardEvent) {
  if (!props.seekable) return;
  const at = dragFraction.value ?? progress.value / 100;
  const to = seekKeyFraction(e.key, at, props.np.duration);
  if (to == null) return;
  e.preventDefault();
  dragFraction.value = to;
  window.clearTimeout(keyTimer);
  keyTimer = window.setTimeout(seekUp, KEY_COMMIT_MS);
}
onUnmounted(() => window.clearTimeout(keyTimer));

const progress = computed(() =>
  dragFraction.value != null
    ? dragFraction.value * 100
    : elapsed.value != null && props.np.duration
      ? (elapsed.value / props.np.duration) * 100
      : 0,
);
/** The time the bar stands for: where a drag or key press is heading, else where the song is. */
const shownSeconds = computed(() =>
  dragFraction.value != null && props.np.duration ? dragFraction.value * props.np.duration : elapsed.value,
);
</script>

<template>
  <!-- When Spotify is the player, click or drag the bar to seek; otherwise it just shows
       progress. Its box is unchanged either way so the compact card keeps its height. -->
  <div
    ref="bar"
    :class="[
      'mt-3 h-1 overflow-hidden rounded-full bg-surface-dark-soft outline-none focus-visible:ring-2 focus-visible:ring-on-dark/30',
      seekable ? 'cursor-pointer' : '',
    ]"
    :role="seekable ? 'slider' : undefined"
    :tabindex="seekable ? 0 : undefined"
    :aria-label="seekable ? 'Seek' : undefined"
    :aria-valuenow="seekable ? Math.round(progress) : undefined"
    :aria-valuetext="seekable ? `${duration(shownSeconds)} of ${duration(np.duration)}` : undefined"
    aria-valuemin="0"
    aria-valuemax="100"
    @keydown="seekKey"
    @pointerdown="seekDown"
    @pointermove="seekMove"
    @pointerup="seekUp"
    @pointercancel="seekUp"
  >
    <!-- Keyed by track so a new song starts at its position. A full-width fill slid left (not a
         width), stepped once a second with no transition: a transition here kept the renderer
         drawing every frame for as long as music played (a 1 s step is under a pixel on a song). -->
    <div
      :key="np.title ?? ''"
      class="h-full w-full rounded-full bg-on-dark"
      :style="{ transform: `translateX(${progress - 100}%)` }"
    />
  </div>
  <div class="mt-1.5 flex items-center justify-between font-mono text-[11px] text-on-dark-soft">
    <span>{{ duration(shownSeconds) }}</span>
    <slot />
    <span>{{ duration(np.duration) }}</span>
  </div>
</template>
