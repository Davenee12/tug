<script setup lang="ts">
import { computed } from "vue";
import { Disc3, Music2, User } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";

// Cover art for a track/album/playlist (square) or artist (round). Images come through tug (the
// webview never loads remote URLs), fetched once per URL and cached on disk by the backend.
const props = withDefaults(
  defineProps<{ url: string | null; kind?: "track" | "album" | "playlist" | "artist"; size?: number }>(),
  { kind: "track", size: 40 },
);
const tug = useTugStore();
const src = computed(() => tug.spotifyCoverFor(props.url));
const round = computed(() => props.kind === "artist");
const Placeholder = computed(() => (props.kind === "artist" ? User : props.kind === "album" ? Disc3 : Music2));
</script>

<template>
  <img
    v-if="src"
    :src="src"
    alt=""
    :class="['shrink-0 object-cover', round ? 'rounded-full' : 'rounded-md']"
    :style="{ width: `${size}px`, height: `${size}px` }"
  />
  <span
    v-else
    :class="['flex shrink-0 items-center justify-center bg-surface-card text-muted', round ? 'rounded-full' : 'rounded-md']"
    :style="{ width: `${size}px`, height: `${size}px` }"
  >
    <component :is="Placeholder" :size="Math.round(size * 0.42)" />
  </span>
</template>
