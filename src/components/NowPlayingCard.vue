<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { Heart, ListMusic, Music2, Pause, Play, Repeat, Repeat1, RotateCcw, Shuffle, SkipBack, SkipForward, Volume1, Volume2 } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { duration } from "../lib/format";
import { canRestart, createHoldRepeater, repeatLabel } from "../lib/media";

const tug = useTugStore();
const np = computed(() => tug.nowPlaying);
const available = computed(() => tug.status.services.media && np.value.title != null);
const playing = computed(() => np.value.state === "playing");

// Spotify augmentation: only when connected and Spotify is the AMS player (see the store).
const sp = computed(() => (tug.spotifyActive ? tug.spotifyPlayer : null));
const art = computed(() => sp.value?.albumArt ?? null);

// AMS only reports elapsed time on state changes; advance it locally while playing.
const now = ref(Date.now());
const timer = window.setInterval(() => (now.value = Date.now()), 1000);

// Press-and-hold volume: one AMS step on press, then a steady repeat. The timing lives in
// lib/media.ts (pure, unit-tested); this just wires DOM events to it. Volume is a 0–1 fraction,
// so stop at the end in each direction to avoid spamming the phone at the limit.
const timers = { set: (fn: () => void, ms: number) => window.setTimeout(fn, ms), clear: (h: number) => window.clearTimeout(h) };
const holdUp = createHoldRepeater(timers);
const holdDown = createHoldRepeater(timers);
const atMax = () => (np.value.volume ?? 0) >= 1;
const atMin = () => (np.value.volume ?? 1) <= 0;
const pressUp = () => holdUp.start(() => tug.media("volumeUp"), atMax);
const pressDown = () => holdDown.start(() => tug.media("volumeDown"), atMin);

// Space/Enter held on a focused button: ignore the OS key-repeat and let our own timer set the
// pace; preventDefault stops the synthetic click (and Space scrolling the page).
function holdKey(e: KeyboardEvent, press: () => void) {
  if (e.key !== " " && e.key !== "Enter" || e.repeat) return;
  e.preventDefault();
  press();
}

function stopHolds() {
  holdUp.stop();
  holdDown.stop();
}
// Leaving the tab or window (alt-tab, minimise) should end any hold even without a pointer/key up.
function onVisibility() {
  if (document.visibilityState !== "visible") stopHolds();
}
onMounted(() => {
  window.addEventListener("blur", stopHolds);
  document.addEventListener("visibilitychange", onVisibility);
});
onUnmounted(() => {
  window.clearInterval(timer);
  window.removeEventListener("blur", stopHolds);
  document.removeEventListener("visibilitychange", onVisibility);
  stopHolds();
});

const elapsed = computed(() => {
  const base = np.value.elapsed;
  if (base == null) return null;
  // Advance from when the phone reported the position, not from the last update of
  // any kind (a volume change must not rewind the bar).
  const reportedAt = np.value.elapsedAt ?? now.value;
  const drift = playing.value ? (Math.max(0, now.value - reportedAt) / 1000) * (np.value.rate ?? 1) : 0;
  return np.value.duration != null ? Math.min(base + drift, np.value.duration) : base + drift;
});
const progress = computed(() =>
  elapsed.value != null && np.value.duration ? (elapsed.value / np.value.duration) * 100 : 0,
);
const can = (c: string) => np.value.available.length === 0 || np.value.available.includes(c as never);

// Start the song over with Back, only where Back restarts rather than skips (see canRestart).
function restart() {
  if (canRestart(elapsed.value)) void tug.media("previousTrack");
  else tug.notify("info", "Already at the start of the song");
}

// No loop button: on Jordan's iPhone Spotify offers no remote commands for repeat and Apple Music
// accepts AdvanceRepeatMode but never changes mode (checked in the log, 2026-10-05). tug still
// reads and logs the repeat mode (see lib/media.ts), so a button can return for a player that
// honours it.
</script>

<template>
  <section class="rounded-xl bg-surface-dark-elevated p-4">
    <div class="caption-upper mb-2.5 flex items-center gap-2 text-on-dark-soft">
      <Music2 :size="13" />
      <span class="min-w-0 flex-1 truncate">{{ available ? np.player ?? "Now playing" : "Now playing" }}</span>
      <template v-if="available">
        <button
          class="rounded-full p-1.5 normal-case text-on-dark-soft active:text-on-dark"
          :disabled="!can('previousTrack')"
          aria-label="Restart song"
          title="Restart song"
          @click="restart"
        >
          <RotateCcw :size="15" />
        </button>
      </template>
      <button
        v-if="tug.spotify.connected"
        class="rounded-full p-1.5 normal-case text-on-dark-soft active:text-on-dark"
        aria-label="Your playlists"
        title="Your Spotify playlists"
        @click="tug.spotifyPanelOpen = true"
      >
        <ListMusic :size="15" />
      </button>
    </div>

    <template v-if="available">
      <div class="flex items-center gap-3">
        <img v-if="art" :src="art" alt="" class="size-11 shrink-0 rounded-md object-cover" />
        <div class="min-w-0 flex-1">
          <p class="truncate font-display text-[22px] leading-tight text-on-dark" style="letter-spacing: -0.01em">
            {{ np.title }}
          </p>
          <p class="mt-0.5 truncate text-[13px] text-on-dark-soft">
            {{ [np.artist, np.album].filter(Boolean).join(" — ") || "Unknown artist" }}
          </p>
        </div>
        <!-- Like the current song: Spotify only, sat next to the title. -->
        <button
          v-if="sp"
          class="shrink-0 rounded-full p-1.5 text-on-dark active:bg-surface-dark-soft"
          :aria-pressed="sp.saved === true"
          aria-label="Like song"
          title="Save to your Liked Songs"
          @click="tug.toggleSpotifyLike()"
        >
          <Heart :size="17" :fill="sp.saved ? 'currentColor' : 'none'" />
        </button>
      </div>

      <div class="mt-3 h-1 overflow-hidden rounded-full bg-surface-dark-soft">
        <!-- Keyed by track so a new song starts at its position instead of sliding back. -->
        <div
          :key="np.title ?? ''"
          class="h-full rounded-full bg-on-dark transition-[width] duration-1000 ease-linear"
          :style="{ width: `${progress}%` }"
        />
      </div>
      <div class="mt-1.5 flex justify-between font-mono text-[11px] text-on-dark-soft">
        <span>{{ duration(elapsed) }}</span>
        <span>{{ duration(np.duration) }}</span>
      </div>

      <!-- One row: playback modes bookend it (Spotify only), volume just inside, transport centred. -->
      <div class="mt-3 flex items-center justify-between">
        <div class="flex items-center gap-0.5">
          <button
            v-if="sp"
            class="rounded-full p-1.5 active:bg-surface-dark-soft"
            :class="sp.shuffle ? 'text-on-dark' : 'text-on-dark-soft/50'"
            :aria-pressed="sp.shuffle"
            aria-label="Shuffle"
            :title="sp.shuffle ? 'Shuffle on' : 'Shuffle off'"
            @click="tug.toggleSpotifyShuffle()"
          >
            <Shuffle :size="16" />
          </button>
          <button
            class="rounded-full p-1.5 text-on-dark-soft active:text-on-dark"
            :disabled="!can('volumeDown')"
            aria-label="Volume down"
            title="Hold to keep changing"
            @pointerdown="pressDown"
            @pointerup="holdDown.stop"
            @pointercancel="holdDown.stop"
            @pointerleave="holdDown.stop"
            @blur="holdDown.stop"
            @keydown="holdKey($event, pressDown)"
            @keyup="holdDown.stop"
          >
            <Volume1 :size="17" />
          </button>
        </div>
        <div class="flex items-center gap-0.5">
          <button class="rounded-full p-1.5 text-on-dark active:bg-surface-dark-soft" :disabled="!can('previousTrack')" aria-label="Previous" @click="tug.media('previousTrack')">
            <SkipBack :size="17" />
          </button>
          <button
            class="flex size-10 items-center justify-center rounded-full bg-on-dark text-surface-dark active:bg-on-dark-soft"
            :aria-label="playing ? 'Pause' : 'Play'"
            @click="tug.media('togglePlayPause')"
          >
            <Pause v-if="playing" :size="17" fill="currentColor" />
            <Play v-else :size="17" fill="currentColor" class="translate-x-px" />
          </button>
          <button class="rounded-full p-1.5 text-on-dark active:bg-surface-dark-soft" :disabled="!can('nextTrack')" aria-label="Next" @click="tug.media('nextTrack')">
            <SkipForward :size="17" />
          </button>
        </div>
        <div class="flex items-center gap-0.5">
          <button
            class="rounded-full p-1.5 text-on-dark-soft active:text-on-dark"
            :disabled="!can('volumeUp')"
            aria-label="Volume up"
            title="Hold to keep changing"
            @pointerdown="pressUp"
            @pointerup="holdUp.stop"
            @pointercancel="holdUp.stop"
            @pointerleave="holdUp.stop"
            @blur="holdUp.stop"
            @keydown="holdKey($event, pressUp)"
            @keyup="holdUp.stop"
          >
            <Volume2 :size="17" />
          </button>
          <button
            v-if="sp"
            class="rounded-full p-1.5 active:bg-surface-dark-soft"
            :class="sp.repeat && sp.repeat !== 'off' ? 'text-on-dark' : 'text-on-dark-soft/50'"
            aria-label="Repeat"
            :title="repeatLabel(sp.repeat)"
            @click="tug.cycleSpotifyRepeat()"
          >
            <Repeat1 v-if="sp.repeat === 'one'" :size="16" />
            <Repeat v-else :size="16" />
          </button>
        </div>
      </div>
    </template>

    <p v-else class="text-[13px] text-on-dark-soft">
      {{ tug.connected ? "Nothing playing on your iPhone." : "Media controls appear once your iPhone is connected." }}
    </p>
  </section>
</template>
