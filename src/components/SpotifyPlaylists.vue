<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { ListMusic, Music2, Search, X } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { useFocusTrap } from "../lib/focusTrap";
import { matchPlaylists, playlistDetail } from "../lib/spotify";
import { api } from "../lib/ipc";

const tug = useTugStore();
const root = ref<HTMLElement | null>(null);
const input = ref<HTMLInputElement | null>(null);
const query = ref("");
const loading = ref(false);
const startingUri = ref<string | null>(null);

useFocusTrap(root, () => close());

onMounted(async () => {
  input.value?.focus();
  if (tug.playlists.length === 0) {
    loading.value = true;
    await tug.loadPlaylists(true);
    loading.value = false;
  }
});

// Reuse the same fuzzy match the Ctrl+K "play …" action uses, so filtering feels consistent.
const shown = computed(() => {
  const q = query.value.trim();
  return q ? matchPlaylists(q, tug.playlists, 100) : tug.playlists;
});

// Covers come through tug (the webview never loads remote images), fetched once per URL for the
// rows being shown and cached on disk by the backend.
const covers = ref<Record<string, string | null>>({});
watch(
  shown,
  (list) => {
    for (const p of list.slice(0, 60)) {
      const url = p.imageUrl;
      if (!url || url in covers.value) continue;
      covers.value = { ...covers.value, [url]: null };
      api
        .spotifyCover(url)
        .then((uri) => (covers.value = { ...covers.value, [url]: uri }))
        .catch(() => undefined);
    }
  },
  { immediate: true },
);

function close() {
  tug.spotifyPanelOpen = false;
}

async function play(uri: string, name: string) {
  startingUri.value = uri;
  const ok = await tug.playPlaylist(uri, name);
  startingUri.value = null;
  if (ok) close();
}
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-start justify-center bg-ink/30 px-6 pt-[10vh] backdrop-blur-[2px]" @click.self="close">
    <div
      ref="root"
      role="dialog"
      aria-modal="true"
      aria-label="Your Spotify playlists"
      class="flex max-h-[76vh] w-full max-w-[560px] flex-col overflow-hidden rounded-xl border border-hairline bg-canvas"
    >
      <div class="flex items-center gap-3 border-b border-hairline-soft px-5 py-3.5">
        <Search :size="18" class="shrink-0 text-muted-soft" />
        <input
          ref="input"
          v-model="query"
          class="h-8 flex-1 bg-transparent text-[17px] text-ink outline-none placeholder:text-muted-soft"
          placeholder="Filter your playlists"
          spellcheck="false"
          autocomplete="off"
          aria-label="Filter playlists"
          @keydown.esc="close"
        />
        <button class="rounded-md p-1.5 text-muted active:bg-surface-card" aria-label="Close" @click="close">
          <X :size="16" />
        </button>
      </div>

      <ul class="min-h-0 flex-1 overflow-y-auto px-2 py-2">
        <li v-if="loading" class="px-4 py-8 text-center text-[13px] text-muted">Loading your playlists…</li>
        <li v-else-if="tug.playlists.length === 0" class="px-4 py-8 text-center text-[13px] text-muted">
          No playlists found on your Spotify account.
        </li>
        <li v-else-if="shown.length === 0" class="px-4 py-8 text-center text-[13px] text-muted">
          Nothing matches “{{ query.trim() }}”.
        </li>
        <li v-for="p in shown" :key="p.uri">
          <button
            class="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left active:bg-surface-card disabled:opacity-60"
            :disabled="startingUri !== null"
            @click="play(p.uri, p.name)"
          >
            <img
              v-if="p.imageUrl && covers[p.imageUrl]"
              :src="covers[p.imageUrl]!"
              alt=""
              class="size-9 shrink-0 rounded-md object-cover"
            />
            <span v-else class="flex size-9 shrink-0 items-center justify-center rounded-md bg-surface-card text-muted">
              <Music2 :size="16" />
            </span>
            <span class="min-w-0 flex-1">
              <span class="block truncate text-[14px] font-medium text-ink">{{ p.name }}</span>
              <span class="block truncate text-[12px] text-muted-soft">
                {{ playlistDetail(p) }}
              </span>
            </span>
            <span v-if="startingUri === p.uri" class="shrink-0 text-[12px] text-muted">Starting…</span>
          </button>
        </li>
      </ul>

      <footer class="flex items-center gap-2 border-t border-hairline-soft px-5 py-2.5 text-[12px] text-muted-soft">
        <ListMusic :size="13" />
        Tap a playlist to play it on your iPhone.
      </footer>
    </div>
  </div>
</template>
