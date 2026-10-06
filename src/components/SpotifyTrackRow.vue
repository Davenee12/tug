<script setup lang="ts">
import { ref } from "vue";
import { Disc3, Heart, ListPlus, MoreHorizontal, Play, Plus, User } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { trackLength } from "../lib/spotify";
import type { SpotifyPlaylist, SpotifyTrack } from "../types/protocol";
import SpotifyArt from "./SpotifyArt.vue";

// One track, as every tab lists it: art, title, "artists — album", length, and a ⋯ menu for queue,
// like, add-to-playlist, and jumping to the album or artist. Clicking the row plays the song (in its
// album context when known, so the next songs keep going).
const props = defineProps<{ track: SpotifyTrack; context?: string | null; ownedPlaylists: SpotifyPlaylist[] }>();
const emit = defineEmits<{ openAlbum: [uri: string]; openArtist: [uri: string] }>();
const tug = useTugStore();
const menu = ref(false);
const picking = ref(false); // the add-to-playlist sub-list is showing
const ITEM = "flex w-full items-center gap-2.5 px-3 py-2 text-left text-[13px] text-body active:bg-surface-card";

function play() {
  void tug.playTrack(props.track, props.context ?? props.track.albumUri);
}
function close() {
  menu.value = false;
  picking.value = false;
}
function addTo(p: SpotifyPlaylist) {
  void tug.addToPlaylist(p.id, p.name, props.track);
  close();
}
</script>

<template>
  <div class="group relative flex items-center gap-3 rounded-lg px-3 py-1.5 hover:bg-surface-card">
    <button class="flex min-w-0 flex-1 items-center gap-3 text-left" @click="play">
      <SpotifyArt :url="track.imageUrl" kind="track" :size="38" />
      <span class="min-w-0 flex-1">
        <span class="block truncate text-[14px] font-medium text-ink">{{ track.name }}</span>
        <span class="block truncate text-[12px] text-muted-soft">
          {{ [track.artists, track.album].filter(Boolean).join(" — ") }}
        </span>
      </span>
    </button>
    <span class="shrink-0 font-mono text-[11px] text-muted-soft">{{ trackLength(track.durationMs) }}</span>
    <button
      class="shrink-0 rounded-md p-1.5 text-muted active:bg-surface-cream-strong"
      aria-label="More actions"
      @click="menu = !menu"
    >
      <MoreHorizontal :size="16" />
    </button>

    <!-- Actions menu (one song). A full-screen catcher closes it on an outside click. -->
    <template v-if="menu">
      <div class="fixed inset-0 z-10" @click="close" />
      <div class="absolute right-2 top-10 z-20 w-56 overflow-hidden rounded-lg border border-hairline bg-canvas py-1 shadow-lg">
        <template v-if="!picking">
          <button :class="ITEM" @click="tug.addToQueue(track).then(close)"><ListPlus :size="15" /> Add to queue</button>
          <button :class="ITEM" @click="tug.likeTrack(track).then(close)"><Heart :size="15" /> Save to Liked Songs</button>
          <button v-if="ownedPlaylists.length" :class="ITEM" @click="picking = true"><Plus :size="15" /> Add to playlist…</button>
          <button v-if="track.albumUri" :class="ITEM" @click="emit('openAlbum', track.albumUri!); close()">
            <Disc3 :size="15" /> Go to album
          </button>
          <button v-if="track.artistUri" :class="ITEM" @click="emit('openArtist', track.artistUri!); close()">
            <User :size="15" /> Go to artist
          </button>
        </template>
        <template v-else>
          <p class="caption-upper px-3 py-1.5 text-muted-soft">Add to…</p>
          <div class="max-h-60 overflow-y-auto">
            <button v-for="p in ownedPlaylists" :key="p.id" :class="[ITEM, 'truncate']" @click="addTo(p)">
              <Play :size="14" class="opacity-0 shrink-0" /> <span class="truncate">{{ p.name }}</span>
            </button>
          </div>
        </template>
      </div>
    </template>
  </div>
</template>
