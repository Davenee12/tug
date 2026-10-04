<script setup lang="ts">
import { computed, onUnmounted, ref } from "vue";
import { Music2, Pause, Play, SkipBack, SkipForward, Volume1, Volume2 } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { duration } from "../lib/format";

const tug = useTugStore();
const np = computed(() => tug.nowPlaying);
const available = computed(() => tug.status.services.media && np.value.title != null);
const playing = computed(() => np.value.state === "playing");

// AMS only reports elapsed time on state changes; advance it locally while playing.
const now = ref(Date.now());
const timer = window.setInterval(() => (now.value = Date.now()), 1000);
onUnmounted(() => window.clearInterval(timer));

const elapsed = computed(() => {
  const base = np.value.elapsed;
  if (base == null) return null;
  const drift = playing.value ? ((now.value - tug.nowPlayingAt) / 1000) * (np.value.rate ?? 1) : 0;
  return np.value.duration != null ? Math.min(base + drift, np.value.duration) : base + drift;
});
const progress = computed(() =>
  elapsed.value != null && np.value.duration ? (elapsed.value / np.value.duration) * 100 : 0,
);
const can = (c: string) => np.value.available.length === 0 || np.value.available.includes(c as never);
</script>

<template>
  <section class="rounded-xl bg-surface-dark-elevated p-5">
    <div class="caption-upper mb-3 flex items-center gap-2 text-on-dark-soft">
      <Music2 :size="13" />
      {{ available ? np.player ?? "Now playing" : "Now playing" }}
    </div>

    <template v-if="available">
      <p class="truncate font-display text-[22px] leading-tight text-on-dark" style="letter-spacing: -0.01em">
        {{ np.title }}
      </p>
      <p class="mt-0.5 truncate text-[13px] text-on-dark-soft">
        {{ [np.artist, np.album].filter(Boolean).join(" — ") || "Unknown artist" }}
      </p>

      <div class="mt-4 h-1 overflow-hidden rounded-full bg-surface-dark-soft">
        <div class="h-full rounded-full bg-on-dark transition-[width] duration-1000 ease-linear" :style="{ width: `${progress}%` }" />
      </div>
      <div class="mt-1.5 flex justify-between font-mono text-[11px] text-on-dark-soft">
        <span>{{ duration(elapsed) }}</span>
        <span>{{ duration(np.duration) }}</span>
      </div>

      <div class="mt-3 flex items-center justify-between">
        <button class="rounded-full p-2 text-on-dark-soft active:text-on-dark" :disabled="!can('volumeDown')" aria-label="Volume down" @click="tug.media('volumeDown')">
          <Volume1 :size="18" />
        </button>
        <div class="flex items-center gap-1">
          <button class="rounded-full p-2 text-on-dark active:bg-surface-dark-soft" :disabled="!can('previousTrack')" aria-label="Previous" @click="tug.media('previousTrack')">
            <SkipBack :size="18" />
          </button>
          <button
            class="flex size-11 items-center justify-center rounded-full bg-on-dark text-surface-dark active:bg-on-dark-soft"
            :aria-label="playing ? 'Pause' : 'Play'"
            @click="tug.media('togglePlayPause')"
          >
            <Pause v-if="playing" :size="18" fill="currentColor" />
            <Play v-else :size="18" fill="currentColor" class="translate-x-px" />
          </button>
          <button class="rounded-full p-2 text-on-dark active:bg-surface-dark-soft" :disabled="!can('nextTrack')" aria-label="Next" @click="tug.media('nextTrack')">
            <SkipForward :size="18" />
          </button>
        </div>
        <button class="rounded-full p-2 text-on-dark-soft active:text-on-dark" :disabled="!can('volumeUp')" aria-label="Volume up" @click="tug.media('volumeUp')">
          <Volume2 :size="18" />
        </button>
      </div>
    </template>

    <p v-else class="text-[13px] text-on-dark-soft">
      {{ tug.connected ? "Nothing playing on your iPhone." : "Media controls appear once your iPhone is connected." }}
    </p>
  </section>
</template>
