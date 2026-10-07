<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";
import {
  ArrowLeft,
  ChevronDown,
  Clock,
  Disc3,
  ListMusic,
  Loader2,
  Play,
  Search,
  Smartphone,
  Monitor,
  Speaker,
  TrendingUp,
  User,
  X,
} from "lucide-vue-next";
import { useTugStore, type SpotifyTab } from "../stores/tug";
import { useFocusTrap } from "../lib/focusTrap";
import { idFromUri, playlistDetail, uniqueAlbums, uniqueSongs } from "../lib/spotify";
import { api, errorMessage } from "../lib/ipc";
import type {
  SpotifyAlbum,
  SpotifyArtist,
  SpotifyPlaylist,
  SpotifySearch,
  SpotifyTrack,
  SpotifyDevice,
} from "../types/protocol";
import SpotifyArt from "./SpotifyArt.vue";
import SpotifyTrackRow from "./SpotifyTrackRow.vue";

const tug = useTugStore();
const root = ref<HTMLElement | null>(null);
const searchInput = ref<HTMLInputElement | null>(null);
useFocusTrap(root, () => close());

const tab = ref<SpotifyTab>(tug.spotifyPanelTab);
const ownedPlaylists = computed(() => tug.playlists.filter((p) => p.owned));

function close() {
  tug.spotifyPanelOpen = false;
}

const TABS: Array<{ id: SpotifyTab; label: string; icon: unknown }> = [
  { id: "search", label: "Search", icon: Search },
  { id: "playlists", label: "Playlists", icon: ListMusic },
  { id: "recent", label: "Recent", icon: Clock },
  { id: "top", label: "Your top", icon: TrendingUp },
  { id: "queue", label: "Up next", icon: Play },
];

// --- A detail view (album / artist / a playlist's songs) stacks over the tabs, with Back. ---
type Detail =
  | { kind: "album"; title: string; subtitle: string; art: string | null; uri: string; tracks: SpotifyTrack[] }
  | { kind: "artist"; title: string; art: string | null; uri: string; albums: SpotifyAlbum[] }
  | { kind: "playlist"; title: string; subtitle: string; art: string | null; uri: string; tracks: SpotifyTrack[] };
const detail = ref<Detail | null>(null);
const detailLoading = ref(false);

async function openAlbum(uri: string) {
  const id = idFromUri(uri);
  if (!id) return;
  detailLoading.value = true;
  detail.value = null;
  try {
    const d = await api.spotifyAlbum(id);
    detail.value = {
      kind: "album",
      title: d.album.name,
      subtitle: [d.album.artists, d.album.year].filter(Boolean).join(" · "),
      art: d.album.imageUrl,
      uri: d.album.uri,
      tracks: d.tracks,
    };
  } catch (e) {
    tug.notify("error", errorMessage(e));
  } finally {
    detailLoading.value = false;
  }
}

async function openArtist(uri: string) {
  const id = idFromUri(uri);
  if (!id) return;
  detailLoading.value = true;
  detail.value = null;
  try {
    const d = await api.spotifyArtist(id);
    detail.value = { kind: "artist", title: d.artist.name, art: d.artist.imageUrl, uri: d.artist.uri, albums: d.albums };
  } catch (e) {
    tug.notify("error", errorMessage(e));
  } finally {
    detailLoading.value = false;
  }
}

async function openPlaylist(p: SpotifyPlaylist) {
  // Followed playlists can't have their songs read (Feb 2026): just play them.
  if (!p.owned) {
    void tug.playContext(p.uri, p.name);
    return;
  }
  detailLoading.value = true;
  detail.value = null;
  try {
    const tracks = await api.spotifyPlaylistItems(p.id);
    detail.value = { kind: "playlist", title: p.name, subtitle: playlistDetail(p), art: p.imageUrl, uri: p.uri, tracks };
  } catch (e) {
    tug.notify("error", errorMessage(e));
  } finally {
    detailLoading.value = false;
  }
}

// --- Search -------------------------------------------------------------------------------
const query = ref("");
const results = ref<SpotifySearch | null>(null);
const searching = ref(false);
const offsets = ref({ tracks: 0, albums: 0, artists: 0, playlists: 0 });
const selected = ref(0); // keyboard selection among track results
let searchTimer: number | undefined;
let searchSeq = 0;

watch(query, (q) => {
  window.clearTimeout(searchTimer);
  selected.value = 0;
  if (!q.trim()) {
    results.value = null;
    searching.value = false;
    return;
  }
  searching.value = true;
  searchTimer = window.setTimeout(() => void runSearch(), 250);
});

async function runSearch() {
  const q = query.value.trim();
  if (!q) return;
  const mine = ++searchSeq;
  searching.value = true;
  try {
    const r = await api.spotifySearch(q, ["track", "album", "artist", "playlist"], 0);
    if (mine !== searchSeq) return;
    // Offsets count what Spotify returned (so "Show more" pages on), not what's shown.
    offsets.value = { tracks: r.tracks.length, albums: r.albums.length, artists: r.artists.length, playlists: r.playlists.length };
    results.value = { ...r, tracks: uniqueSongs(r.tracks), albums: uniqueAlbums(r.albums) };
  } catch (e) {
    if (mine === searchSeq) tug.notify("error", errorMessage(e));
  } finally {
    if (mine === searchSeq) searching.value = false;
  }
}

async function showMore(kind: "track" | "album" | "artist" | "playlist") {
  const q = query.value.trim();
  if (!q || !results.value) return;
  const key = `${kind}s` as "tracks" | "albums" | "artists" | "playlists";
  try {
    const r = await api.spotifySearch(q, [kind], offsets.value[key]);
    const cur = results.value;
    const fresh =
      key === "tracks" ? uniqueSongs(r.tracks, cur.tracks) : key === "albums" ? uniqueAlbums(r.albums, cur.albums) : r[key];
    cur[key] = [...cur[key], ...(fresh as never[])] as never;
    cur.more[key] = r.more[key];
    offsets.value[key] += r[key].length;
  } catch (e) {
    tug.notify("error", errorMessage(e));
  }
}

function onSearchKey(e: KeyboardEvent) {
  const tracks = results.value?.tracks ?? [];
  if (e.key === "ArrowDown") {
    selected.value = Math.min(selected.value + 1, Math.max(0, tracks.length - 1));
    e.preventDefault();
  } else if (e.key === "ArrowUp") {
    selected.value = Math.max(selected.value - 1, 0);
    e.preventDefault();
  } else if (e.key === "Enter") {
    const t = tracks[selected.value] ?? tracks[0];
    if (t) void tug.playTrack(t, t.albumUri);
    e.preventDefault();
  }
}

// --- Per-tab data, loaded lazily ----------------------------------------------------------
const recent = ref<SpotifyTrack[] | null>(null);
const topMode = ref<"tracks" | "artists">("tracks");
const topRange = ref<"short_term" | "medium_term" | "long_term">("medium_term");
const topTracks = ref<SpotifyTrack[] | null>(null);
const topArtists = ref<SpotifyArtist[] | null>(null);
const queue = ref<{ currentlyPlaying: SpotifyTrack | null; queue: SpotifyTrack[] } | null>(null);
const tabLoading = ref(false);

async function load<T>(setter: (v: T) => void, call: () => Promise<T>) {
  tabLoading.value = true;
  try {
    setter(await call());
  } catch (e) {
    tug.notify("error", errorMessage(e));
  } finally {
    tabLoading.value = false;
  }
}

async function loadRecent() {
  if (recent.value) return;
  await load((v: SpotifyTrack[]) => (recent.value = v), () => api.spotifyRecentlyPlayed());
}
async function loadTop() {
  if (topMode.value === "tracks") {
    await load((v: SpotifyTrack[]) => (topTracks.value = v), () => api.spotifyTopTracks(topRange.value));
  } else {
    await load((v: SpotifyArtist[]) => (topArtists.value = v), () => api.spotifyTopArtists(topRange.value));
  }
}
async function loadQueue() {
  await load((v) => (queue.value = v), () => api.spotifyQueue());
}

// Switching tab (or the top filters) loads what that tab shows, once.
watch(
  [tab, topMode, topRange],
  ([t]) => {
    detail.value = null;
    if (t === "recent") void loadRecent();
    else if (t === "top") {
      topTracks.value = null;
      topArtists.value = null;
      void loadTop();
    } else if (t === "queue") void loadQueue();
  },
  { immediate: true },
);

onMounted(async () => {
  if (tug.playlists.length === 0) void tug.loadPlaylists(true);
  await nextTick();
  if (tab.value === "search") searchInput.value?.focus();
});

// --- Device picker ("Play on") ------------------------------------------------------------
const devicesOpen = ref(false);
const devices = ref<SpotifyDevice[]>([]);
async function toggleDevices() {
  devicesOpen.value = !devicesOpen.value;
  if (devicesOpen.value) {
    try {
      devices.value = await api.spotifyDevices();
    } catch (e) {
      tug.notify("error", errorMessage(e));
    }
  }
}
function deviceIcon(kind: string) {
  const k = kind.toLowerCase();
  if (k === "computer") return Monitor;
  if (k === "smartphone") return Smartphone;
  return Speaker;
}
async function pickDevice(d: SpotifyDevice | null) {
  devicesOpen.value = false;
  await tug.chooseDevice(d);
}

const TAB_BTN = "flex items-center gap-1.5 whitespace-nowrap rounded-full px-3 py-1.5 text-[13px] font-medium";
const SHOW_MORE = "mx-3 my-1 rounded-lg px-3 py-2 text-left text-[13px] font-medium text-accent-teal active:bg-surface-card";
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-start justify-center bg-ink/30 px-6 pt-[7vh] backdrop-blur-[2px]" @click.self="close">
    <div
      ref="root"
      role="dialog"
      aria-modal="true"
      aria-label="Spotify"
      class="flex max-h-[84vh] w-full max-w-[640px] flex-col overflow-hidden rounded-xl border border-hairline bg-canvas"
    >
      <!-- Header -->
      <header class="flex items-center gap-3 border-b border-hairline-soft px-5 py-3">
        <ListMusic :size="18" class="shrink-0 text-accent-teal" />
        <span class="headline text-[18px] text-ink">Spotify</span>
        <div class="relative ml-auto">
          <button
            class="flex items-center gap-1.5 rounded-full border border-hairline px-2.5 py-1 text-[12px] text-body active:bg-surface-card"
            aria-label="Choose where to play"
            @click="toggleDevices"
          >
            <Smartphone :size="13" />
            <span class="max-w-[140px] truncate">{{ tug.spotifyTargetName }}</span>
            <ChevronDown :size="13" />
          </button>
          <template v-if="devicesOpen">
            <div class="fixed inset-0 z-10" @click="devicesOpen = false" />
            <div class="absolute right-0 top-9 z-20 w-60 overflow-hidden rounded-lg border border-hairline bg-canvas py-1 shadow-lg">
              <p class="caption-upper px-3 py-1.5 text-muted-soft">Play on</p>
              <button
                class="flex w-full items-center gap-2.5 px-3 py-2 text-left text-[13px] text-body active:bg-surface-card"
                @click="pickDevice(null)"
              >
                <Smartphone :size="15" /> <span class="flex-1 truncate">iPhone</span>
                <span v-if="!tug.spotifyDevice" class="text-[11px] text-accent-teal">Current</span>
              </button>
              <button
                v-for="d in devices.filter((x) => x.kind.toLowerCase() !== 'smartphone')"
                :key="d.id"
                class="flex w-full items-center gap-2.5 px-3 py-2 text-left text-[13px] text-body active:bg-surface-card"
                @click="pickDevice(d)"
              >
                <component :is="deviceIcon(d.kind)" :size="15" />
                <span class="flex-1 truncate">{{ d.name }}</span>
                <span v-if="tug.spotifyDevice?.id === d.id" class="text-[11px] text-accent-teal">Current</span>
              </button>
              <p v-if="devices.filter((x) => x.kind.toLowerCase() !== 'smartphone').length === 0" class="px-3 py-2 text-[12px] text-muted-soft">
                No other Spotify devices are active.
              </p>
            </div>
          </template>
        </div>
        <button class="shrink-0 rounded-md p-1.5 text-muted active:bg-surface-card" aria-label="Close" @click="close">
          <X :size="16" />
        </button>
      </header>

      <!-- Tabs -->
      <nav class="flex shrink-0 gap-1 overflow-x-auto border-b border-hairline-soft px-3 py-2">
        <button
          v-for="t in TABS"
          :key="t.id"
          :class="[TAB_BTN, tab === t.id && !detail ? 'bg-ink text-on-dark' : 'text-muted active:bg-surface-card']"
          @click="detail = null; tab = t.id"
        >
          <component :is="t.icon" :size="14" />
          {{ t.label }}
        </button>
      </nav>

      <div class="min-h-0 flex-1 overflow-y-auto">
        <!-- Detail view (album / artist / playlist songs) -->
        <section v-if="detail || detailLoading" class="p-2">
          <button class="mb-1 flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-[13px] text-muted active:bg-surface-card" @click="detail = null">
            <ArrowLeft :size="15" /> Back
          </button>
          <div v-if="detailLoading" class="flex items-center justify-center gap-2 py-12 text-[13px] text-muted">
            <Loader2 :size="16" class="animate-spin" /> Loading…
          </div>
          <template v-else-if="detail">
            <div class="flex items-center gap-4 px-3 py-3">
              <SpotifyArt :url="detail.art" :kind="detail.kind === 'artist' ? 'artist' : detail.kind" :size="88" />
              <div class="min-w-0 flex-1">
                <p class="headline text-[22px] leading-tight text-ink">{{ detail.title }}</p>
                <p v-if="detail.kind !== 'artist'" class="mt-0.5 truncate text-[13px] text-muted">{{ detail.subtitle }}</p>
                <button
                  class="mt-2.5 inline-flex items-center gap-1.5 rounded-full bg-primary px-4 py-1.5 text-[13px] font-medium text-on-primary active:bg-primary-active"
                  @click="tug.playContext(detail.uri, detail.title)"
                >
                  <Play :size="14" fill="currentColor" /> Play
                </button>
              </div>
            </div>

            <!-- Album / playlist songs -->
            <template v-if="detail.kind === 'album' || detail.kind === 'playlist'">
              <SpotifyTrackRow
                v-for="t in detail.tracks"
                :key="t.uri"
                :track="t"
                :context="detail.uri"
                :owned-playlists="ownedPlaylists"
                @open-album="openAlbum"
                @open-artist="openArtist"
              />
              <p v-if="detail.tracks.length === 0" class="px-4 py-8 text-center text-[13px] text-muted">No songs to show.</p>
            </template>

            <!-- Artist albums -->
            <template v-else>
              <p class="caption-upper px-4 pt-2 pb-1 text-muted-soft">Albums</p>
              <button
                v-for="al in detail.albums"
                :key="al.uri"
                class="flex w-full items-center gap-3 rounded-lg px-3 py-1.5 text-left hover:bg-surface-card"
                @click="openAlbum(al.uri)"
              >
                <SpotifyArt :url="al.imageUrl" kind="album" :size="38" />
                <span class="min-w-0 flex-1">
                  <span class="block truncate text-[14px] font-medium text-ink">{{ al.name }}</span>
                  <span class="block truncate text-[12px] text-muted-soft">{{ [al.year, al.totalTracks ? `${al.totalTracks} songs` : null].filter(Boolean).join(" · ") }}</span>
                </span>
              </button>
              <p v-if="detail.albums.length === 0" class="px-4 py-8 text-center text-[13px] text-muted">No albums to show.</p>
            </template>
          </template>
        </section>

        <!-- Search tab -->
        <section v-else-if="tab === 'search'" class="flex flex-col">
          <div class="flex items-center gap-3 border-b border-hairline-soft px-5 py-2.5">
            <Search :size="17" class="shrink-0 text-muted-soft" />
            <input
              ref="searchInput"
              v-model="query"
              class="h-7 flex-1 bg-transparent text-[15px] text-ink outline-none placeholder:text-muted-soft"
              placeholder="Songs, albums, artists, playlists"
              spellcheck="false"
              autocomplete="off"
              aria-label="Search Spotify"
              @keydown="onSearchKey"
              @keydown.esc="close"
            />
            <Loader2 v-if="searching" :size="15" class="shrink-0 animate-spin text-muted-soft" />
          </div>

          <div class="p-2">
            <p v-if="!query.trim()" class="px-4 py-10 text-center text-[13px] text-muted">
              Search for a song and play it, add it to your queue, or save it.
            </p>
            <p
              v-else-if="!searching && results && results.tracks.length + results.albums.length + results.artists.length + results.playlists.length === 0"
              class="px-4 py-10 text-center text-[13px] text-muted"
            >
              Nothing on Spotify matches “{{ query.trim() }}”.
            </p>
            <template v-else-if="results">
              <!-- Tracks -->
              <template v-if="results.tracks.length">
                <p class="caption-upper px-4 pt-2 pb-1 text-muted-soft">Songs</p>
                <div
                  v-for="(t, i) in results.tracks"
                  :key="t.uri"
                  :class="['rounded-lg', i === selected ? 'ring-1 ring-primary/40' : '']"
                >
                  <SpotifyTrackRow :track="t" :owned-playlists="ownedPlaylists" @open-album="openAlbum" @open-artist="openArtist" />
                </div>
                <button v-if="results.more.tracks" :class="SHOW_MORE" @click="showMore('track')">Show more songs</button>
              </template>

              <!-- Artists -->
              <template v-if="results.artists.length">
                <p class="caption-upper px-4 pt-3 pb-1 text-muted-soft">Artists</p>
                <button
                  v-for="a in results.artists"
                  :key="a.uri"
                  class="flex w-full items-center gap-3 rounded-lg px-3 py-1.5 text-left hover:bg-surface-card"
                  @click="openArtist(a.uri)"
                >
                  <SpotifyArt :url="a.imageUrl" kind="artist" :size="38" />
                  <span class="min-w-0 flex-1 truncate text-[14px] font-medium text-ink">{{ a.name }}</span>
                  <User :size="15" class="shrink-0 text-muted-soft" />
                </button>
                <button v-if="results.more.artists" :class="SHOW_MORE" @click="showMore('artist')">Show more artists</button>
              </template>

              <!-- Albums -->
              <template v-if="results.albums.length">
                <p class="caption-upper px-4 pt-3 pb-1 text-muted-soft">Albums</p>
                <button
                  v-for="al in results.albums"
                  :key="al.uri"
                  class="flex w-full items-center gap-3 rounded-lg px-3 py-1.5 text-left hover:bg-surface-card"
                  @click="openAlbum(al.uri)"
                >
                  <SpotifyArt :url="al.imageUrl" kind="album" :size="38" />
                  <span class="min-w-0 flex-1">
                    <span class="block truncate text-[14px] font-medium text-ink">{{ al.name }}</span>
                    <span class="block truncate text-[12px] text-muted-soft">{{ [al.artists, al.year].filter(Boolean).join(" · ") }}</span>
                  </span>
                  <Disc3 :size="15" class="shrink-0 text-muted-soft" />
                </button>
                <button v-if="results.more.albums" :class="SHOW_MORE" @click="showMore('album')">Show more albums</button>
              </template>

              <!-- Playlists -->
              <template v-if="results.playlists.length">
                <p class="caption-upper px-4 pt-3 pb-1 text-muted-soft">Playlists</p>
                <button
                  v-for="p in results.playlists"
                  :key="p.uri"
                  class="flex w-full items-center gap-3 rounded-lg px-3 py-1.5 text-left hover:bg-surface-card"
                  @click="openPlaylist(p)"
                >
                  <SpotifyArt :url="p.imageUrl" kind="playlist" :size="38" />
                  <span class="min-w-0 flex-1">
                    <span class="block truncate text-[14px] font-medium text-ink">{{ p.name }}</span>
                    <span class="block truncate text-[12px] text-muted-soft">{{ playlistDetail(p) }}</span>
                  </span>
                </button>
                <button v-if="results.more.playlists" :class="SHOW_MORE" @click="showMore('playlist')">Show more playlists</button>
              </template>
            </template>
          </div>
        </section>

        <!-- Playlists tab -->
        <section v-else-if="tab === 'playlists'" class="p-2">
          <p v-if="tug.playlists.length === 0" class="px-4 py-10 text-center text-[13px] text-muted">No playlists on your account.</p>
          <button
            v-for="p in tug.playlists"
            :key="p.uri"
            class="flex w-full items-center gap-3 rounded-lg px-3 py-1.5 text-left hover:bg-surface-card"
            @click="openPlaylist(p)"
          >
            <SpotifyArt :url="p.imageUrl" kind="playlist" :size="40" />
            <span class="min-w-0 flex-1">
              <span class="block truncate text-[14px] font-medium text-ink">{{ p.name }}</span>
              <span class="block truncate text-[12px] text-muted-soft">{{ playlistDetail(p) }}</span>
            </span>
            <span class="shrink-0 text-[11px] text-muted-soft">{{ p.owned ? "Open" : "Play" }}</span>
          </button>
        </section>

        <!-- Recent tab -->
        <section v-else-if="tab === 'recent'" class="p-2">
          <div v-if="tabLoading && !recent" class="flex items-center justify-center gap-2 py-12 text-[13px] text-muted">
            <Loader2 :size="16" class="animate-spin" /> Loading…
          </div>
          <template v-else>
            <SpotifyTrackRow
              v-for="(t, i) in recent ?? []"
              :key="t.uri + '-' + i"
              :track="t"
              :owned-playlists="ownedPlaylists"
              @open-album="openAlbum"
              @open-artist="openArtist"
            />
            <p v-if="recent && recent.length === 0" class="px-4 py-10 text-center text-[13px] text-muted">Nothing played recently.</p>
          </template>
        </section>

        <!-- Your top tab -->
        <section v-else-if="tab === 'top'" class="p-2">
          <div class="flex items-center gap-1 px-3 pb-2">
            <button :class="[TAB_BTN, topMode === 'tracks' ? 'bg-surface-cream-strong text-ink' : 'text-muted']" @click="topMode = 'tracks'">Songs</button>
            <button :class="[TAB_BTN, topMode === 'artists' ? 'bg-surface-cream-strong text-ink' : 'text-muted']" @click="topMode = 'artists'">Artists</button>
            <select
              v-model="topRange"
              class="ml-auto rounded-md border border-hairline bg-canvas px-2 py-1 text-[12px] text-body"
              aria-label="Time range"
            >
              <option value="short_term">Last 4 weeks</option>
              <option value="medium_term">Last 6 months</option>
              <option value="long_term">All time</option>
            </select>
          </div>
          <div v-if="tabLoading" class="flex items-center justify-center gap-2 py-12 text-[13px] text-muted">
            <Loader2 :size="16" class="animate-spin" /> Loading…
          </div>
          <template v-else-if="topMode === 'tracks'">
            <SpotifyTrackRow
              v-for="t in topTracks ?? []"
              :key="t.uri"
              :track="t"
              :owned-playlists="ownedPlaylists"
              @open-album="openAlbum"
              @open-artist="openArtist"
            />
            <p v-if="topTracks && topTracks.length === 0" class="px-4 py-10 text-center text-[13px] text-muted">No top songs yet.</p>
          </template>
          <template v-else>
            <button
              v-for="a in topArtists ?? []"
              :key="a.uri"
              class="flex w-full items-center gap-3 rounded-lg px-3 py-1.5 text-left hover:bg-surface-card"
              @click="openArtist(a.uri)"
            >
              <SpotifyArt :url="a.imageUrl" kind="artist" :size="40" />
              <span class="min-w-0 flex-1 truncate text-[14px] font-medium text-ink">{{ a.name }}</span>
            </button>
            <p v-if="topArtists && topArtists.length === 0" class="px-4 py-10 text-center text-[13px] text-muted">No top artists yet.</p>
          </template>
        </section>

        <!-- Up next (queue) tab -->
        <section v-else-if="tab === 'queue'" class="p-2">
          <div v-if="tabLoading && !queue" class="flex items-center justify-center gap-2 py-12 text-[13px] text-muted">
            <Loader2 :size="16" class="animate-spin" /> Loading…
          </div>
          <template v-else-if="queue">
            <template v-if="queue.currentlyPlaying">
              <p class="caption-upper px-4 pt-2 pb-1 text-muted-soft">Now playing</p>
              <SpotifyTrackRow
                :track="queue.currentlyPlaying"
                :owned-playlists="ownedPlaylists"
                @open-album="openAlbum"
                @open-artist="openArtist"
              />
            </template>
            <p class="caption-upper px-4 pt-3 pb-1 text-muted-soft">Next up</p>
            <SpotifyTrackRow
              v-for="(t, i) in queue.queue"
              :key="t.uri + i"
              :track="t"
              :owned-playlists="ownedPlaylists"
              @open-album="openAlbum"
              @open-artist="openArtist"
            />
            <p v-if="queue.queue.length === 0" class="px-4 py-8 text-center text-[13px] text-muted">Nothing queued.</p>
          </template>
        </section>
      </div>
    </div>
  </div>
</template>
