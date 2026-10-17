import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { nextTick, reactive } from "vue";

/** Each album_art call, with a way to answer it. */
let calls: Array<{ artist: string; album: string; resolve: (src: string | null) => void }> = [];

vi.mock("../lib/ipc", () => ({
  api: {
    albumArt: vi.fn(
      (artist: string, album: string) => new Promise<string | null>((resolve) => calls.push({ artist, album, resolve })),
    ),
  },
}));

const paletteFromImage = vi.fn(async (_src: string) => ({ dominant: { r: 240, g: 230, b: 200 }, accent: { r: 220, g: 160, b: 60 } }));
vi.mock("../lib/artCanvas", () => ({
  paletteFromImage: (src: string) => paletteFromImage(src),
  themeColors: () => ({ text: { r: 160, g: 157, b: 150 }, base: { r: 37, g: 35, b: 32 } }),
}));

// Just the parts of the main store album art reads.
const tug = reactive({
  status: { services: { media: true } },
  nowPlaying: { title: "Teardrop" as string | null },
  settings: { albumArt: true },
  spotifyActive: false,
  spotifyTrackVerified: false,
  spotifyPlayer: null as { albumArt: string | null } | null,
  trackView: { artist: "Massive Attack" as string | null, album: "Mezzanine" as string | null },
  pageVisible: true,
});
vi.mock("./tug", () => ({ useTugStore: () => tug }));

const { useAlbumArtStore: makeStore } = await import("./albumArt");

// Each test's store, stopped afterwards so its watchers don't follow the next test's changes.
let made: Array<ReturnType<typeof makeStore>> = [];
const useAlbumArtStore = () => {
  const store = makeStore();
  made.push(store);
  return store;
};

const flush = async () => {
  await nextTick();
  await new Promise((r) => setTimeout(r, 0));
};

describe("album art store", () => {
  afterEach(() => {
    for (const store of made) store.$dispose();
    made = [];
  });

  beforeEach(() => {
    setActivePinia(createPinia());
    calls = [];
    paletteFromImage.mockClear();
    Object.assign(tug, {
      settings: { albumArt: true },
      spotifyActive: false,
      spotifyTrackVerified: false,
      spotifyPlayer: null,
      trackView: { artist: "Massive Attack", album: "Mezzanine" },
      pageVisible: true,
    });
  });

  it("looks the album up once, shows it and tints the card from it", async () => {
    const art = useAlbumArtStore();
    await flush();
    expect(calls).toHaveLength(1);
    expect(calls[0]).toMatchObject({ artist: "Massive Attack", album: "Mezzanine" });
    calls[0].resolve("data:image/jpeg;base64,COVER");
    await flush();
    expect(art.src).toBe("data:image/jpeg;base64,COVER");
    expect(art.tint).not.toBeNull();
    // Another song from the same album: no second lookup, no second canvas pass.
    tug.nowPlaying.title = "Angel";
    await flush();
    expect(calls).toHaveLength(1);
    expect(paletteFromImage).toHaveBeenCalledTimes(1);
  });

  it("switched off: nothing is looked up and the placeholder shows", async () => {
    tug.settings.albumArt = false;
    const art = useAlbumArtStore();
    await flush();
    expect(calls).toHaveLength(0);
    expect(art.src).toBeNull();
    expect(art.tint).toBeNull();
  });

  it("Spotify's art takes precedence: no lookup while Spotify is the player", async () => {
    Object.assign(tug, { spotifyActive: true, spotifyTrackVerified: false, spotifyPlayer: { albumArt: "data:image/jpeg;base64,SPOTIFY" } });
    const art = useAlbumArtStore();
    await flush();
    // Not yet sure Spotify describes this song: the placeholder, and still no lookup.
    expect(art.src).toBeNull();
    tug.spotifyTrackVerified = true;
    await flush();
    expect(art.src).toBe("data:image/jpeg;base64,SPOTIFY");
    expect(calls).toHaveLength(0);
  });

  it("hidden in the tray: no lookup until tug is on screen again", async () => {
    tug.pageVisible = false;
    useAlbumArtStore();
    await flush();
    expect(calls).toHaveLength(0);
    tug.pageVisible = true;
    await flush();
    expect(calls).toHaveLength(1);
  });

  it("skipping on: a late answer for the last album never shows on this one", async () => {
    const art = useAlbumArtStore();
    await flush();
    tug.trackView = { artist: "Portishead", album: "Dummy" };
    await flush();
    expect(calls).toHaveLength(2);
    // The backend gave up on the first lookup (empty answer), and it arrives last.
    calls[1].resolve("data:image/jpeg;base64,DUMMY");
    calls[0].resolve(null);
    await flush();
    expect(art.src).toBe("data:image/jpeg;base64,DUMMY");
    // Back to the first album: its empty answer wasn't remembered, so it's asked again.
    tug.trackView = { artist: "Massive Attack", album: "Mezzanine" };
    await flush();
    expect(calls).toHaveLength(3);
  });
});
