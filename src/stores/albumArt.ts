// The Now Playing card's album art and colours. Which art to show is decided by lib/albumArt
// (`artChoice`: Spotify's first, else Apple's catalog while Show album art is on); this store
// fetches the cover through the backend (`album_art`, which caches it on disk, debounces skips and
// rate-limits lookups) and reads its colours once per new cover (lib/artCanvas).

import { defineStore } from "pinia";
import { computed, shallowRef, watch } from "vue";
import { api } from "../lib/ipc";
import { artChoice, cardTint, type ArtChoice, type CardTint } from "../lib/albumArt";
import { paletteFromImage, themeColors } from "../lib/artCanvas";
import { latestOnly } from "../lib/latest";
import { useTugStore } from "./tug";

/** Covers kept in memory for albums played this session (the disk cache holds the rest). */
const MEMO_COVERS = 12;
/** Tints kept for covers seen this session, so going back to an album doesn't read it again. */
const MEMO_TINTS = 12;

/** A small most-recently-used memo: setting a key moves it to the end, the oldest falls off. */
function remember<V>(memo: Map<string, V>, key: string, value: V, max: number) {
  memo.delete(key);
  memo.set(key, value);
  while (memo.size > max) memo.delete(memo.keys().next().value as string);
}

export const useAlbumArtStore = defineStore("albumArt", () => {
  const tug = useTugStore();

  const choice = computed<ArtChoice>(() => {
    // Only for a song the card is showing.
    const shown = tug.status.services.media && tug.nowPlaying.title != null;
    return artChoice({
      settingOn: tug.settings.albumArt,
      spotifyActive: tug.spotifyActive,
      spotifyVerified: tug.spotifyTrackVerified,
      spotifyArt: tug.spotifyPlayer?.albumArt ?? null,
      artist: shown ? tug.trackView.artist : null,
      album: shown ? tug.trackView.album : null,
    });
  });

  // ---- Covers from Apple's catalog ----
  /** Answers by album this session: a data URI, or null when Apple has no cover for it. */
  const covers = shallowRef(new Map<string, string | null>());
  const lookups = latestOnly();
  let pending: string | null = null;

  const lookupKey = computed(() => (choice.value.kind === "lookup" ? choice.value.key : null));
  watch(
    [lookupKey, () => tug.pageVisible],
    ([key, visible]) => {
      if (!key || covers.value.has(key) || pending === key) return;
      // Hidden in the tray: nothing is looked up; the song showing when tug comes back is.
      if (!visible) return;
      const c = choice.value;
      if (c.kind !== "lookup") return;
      const live = lookups.begin();
      pending = key;
      api
        .albumArt(c.artist, c.album)
        .then((src) => {
          // A newer lookup replaced this one (the backend gave up on it, so its empty answer
          // means nothing): it isn't kept.
          if (!live()) return;
          const next = new Map(covers.value);
          remember(next, key, src, MEMO_COVERS);
          covers.value = next;
        })
        // Offline or the lookup failed: the placeholder, asked again next time the album plays.
        .catch(() => undefined)
        .finally(() => {
          if (pending === key) pending = null;
        });
    },
    { immediate: true },
  );

  /** The cover to show, or null for the placeholder. */
  const src = computed<string | null>(() => {
    const c = choice.value;
    if (c.kind === "spotify") return c.src;
    if (c.kind === "lookup") return covers.value.get(c.key) ?? null;
    return null;
  });

  // ---- Colours ----
  /** The card's tint from the cover, or null for the default colours. */
  const tint = shallowRef<CardTint | null>(null);
  const tints = new Map<string, CardTint | null>();
  const reads = latestOnly();
  watch(
    src,
    async (cover) => {
      const live = reads.begin();
      if (!cover) {
        tint.value = null;
        return;
      }
      if (tints.has(cover)) {
        tint.value = tints.get(cover) ?? null;
        return;
      }
      const palette = await paletteFromImage(cover);
      const theme = themeColors();
      const next = palette && theme ? cardTint(palette, theme) : null;
      remember(tints, cover, next, MEMO_TINTS);
      if (live()) tint.value = next;
    },
    { immediate: true },
  );

  return { choice, src, tint };
});
