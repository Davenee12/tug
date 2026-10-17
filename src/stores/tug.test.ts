import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { PhoneNotification } from "../types/protocol";

// The store runs in Node here: just enough of a browser for it to be created (never init()'d).
vi.stubGlobal("window", globalThis);
vi.stubGlobal("document", {
  visibilityState: "visible",
  hidden: false,
  hasFocus: () => true,
  documentElement: { style: {} },
  addEventListener: () => {},
  removeEventListener: () => {},
});

/** Each test sets the backend calls it cares about; anything else answers with nothing. */
const impl: Record<string, (...args: never[]) => Promise<unknown>> = {};
const called: Array<[string, unknown[]]> = [];
vi.mock("../lib/ipc", () => ({
  api: new Proxy(
    {},
    {
      get:
        (_t, name: string) =>
        (...args: never[]) => {
          called.push([name, args]);
          return impl[name] ? impl[name](...args) : Promise.resolve(undefined);
        },
    },
  ),
  on: async () => () => {},
  errorMessage: (e: unknown) => String(e),
}));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: async () => "0.0.0" }));
vi.mock("@tauri-apps/plugin-notification", () => ({ isPermissionGranted: async () => true, requestPermission: async () => "granted" }));

const { useTugStore } = await import("./tug");

const flush = async () => {
  for (let i = 0; i < 10; i++) await Promise.resolve();
};
function deferred<T = void>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => ((resolve = res), (reject = rej)));
  return { promise, resolve, reject };
}
const errors = (tug: ReturnType<typeof useTugStore>) => (tug.flash?.kind === "error" ? [tug.flash] : []);

beforeEach(() => {
  setActivePinia(createPinia());
  for (const k of Object.keys(impl)) delete impl[k];
  called.length = 0;
});

describe("a switch whose save fails goes back", () => {
  it("setSetting reverts and says why", async () => {
    const tug = useTugStore();
    impl.setSetting = () => Promise.reject("Couldn't save");
    const before = tug.settings.doNotDisturb;
    const ok = await tug.setSetting("doNotDisturb", !before);
    expect(ok).toBe(false);
    expect(tug.settings.doNotDisturb).toBe(before);
    expect(errors(tug).length).toBeGreaterThan(0);
  });

  it("setSetting keeps the change when it saves", async () => {
    const tug = useTugStore();
    const before = tug.settings.doNotDisturb;
    expect(await tug.setSetting("doNotDisturb", !before)).toBe(true);
    expect(tug.settings.doNotDisturb).toBe(!before);
  });

  it("an older failed save doesn't undo a newer change", async () => {
    const tug = useTugStore();
    const first = deferred();
    impl.setSetting = () => first.promise;
    const before = tug.settings.doNotDisturb;
    const a = tug.setSetting("doNotDisturb", !before);
    impl.setSetting = () => Promise.resolve();
    await tug.setSetting("doNotDisturb", before);
    first.reject("Couldn't save");
    await a;
    expect(tug.settings.doNotDisturb).toBe(before);
  });

  it("setAdvertising reverts when it fails", async () => {
    const tug = useTugStore();
    impl.setAdvertising = () => Promise.reject("Bluetooth is off");
    expect(tug.advertiseEnabled).toBe(true);
    await tug.setAdvertising(false);
    expect(tug.advertiseEnabled).toBe(true);
    expect(errors(tug).length).toBeGreaterThan(0);
  });

  it("the type-the-code switch goes back if the shortcut can't be set up", async () => {
    const tug = useTugStore();
    tug.settings = { ...tug.settings, typeCodeHotkey: false };
    impl.setCodeHotkey = () => Promise.reject("Couldn't register");
    await tug.setCodeHotkey(true, tug.settings.typeCodeKeys);
    expect(tug.settings.typeCodeHotkey).toBe(false);
    // Nothing was saved for a shortcut that never took.
    expect(called.some(([name]) => name === "setSetting")).toBe(false);
  });

  it("the type-the-code switch stays on with the in-use warning when another app holds the keys", async () => {
    const tug = useTugStore();
    tug.settings = { ...tug.settings, typeCodeHotkey: false };
    impl.setCodeHotkey = async (...args: never[]) => ({ state: "inUse", keys: args[1] });
    await tug.setCodeHotkey(true, tug.settings.typeCodeKeys);
    expect(tug.settings.typeCodeHotkey).toBe(true);
    expect(tug.codeHotkey.state).toBe("inUse");
  });

  it("the type-the-code switch goes back, and the shortcut too, if the choice can't be saved", async () => {
    const tug = useTugStore();
    tug.settings = { ...tug.settings, typeCodeHotkey: false };
    tug.codeHotkey = { state: "off", keys: tug.settings.typeCodeKeys };
    impl.setCodeHotkey = async (...args: never[]) => ({ state: args[0] ? "on" : "off", keys: args[1] });
    impl.setSetting = () => Promise.reject("Couldn't save");
    await tug.setCodeHotkey(true, tug.settings.typeCodeKeys);
    expect(tug.settings.typeCodeHotkey).toBe(false);
    expect(tug.codeHotkey.state).toBe("off");
    const registrations = called.filter(([name]) => name === "setCodeHotkey").map(([, args]) => args[0]);
    expect(registrations).toEqual([true, false]);
  });
});

describe("Spotify shuffle and repeat keep what was pressed", () => {
  beforeEach(() => vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] }));
  afterEach(() => vi.useRealTimers());

  async function playing() {
    const tug = useTugStore();
    // Spotify's read lags: it still says what it was before the press.
    impl.spotifyPlayer = async () => ({ shuffle: false, repeat: "off" });
    tug.spotify = { ...tug.spotify, connected: true };
    tug.nowPlaying = { ...tug.nowPlaying, player: "Spotify" };
    // Let the store's own read for the song go by, then count only what the presses cause.
    await flush();
    vi.advanceTimersByTime(10_000);
    await flush();
    tug.spotifyPlayer = { shuffle: false, repeat: "off" } as never;
    called.length = 0;
    return tug;
  }
  const reads = () => called.filter(([name]) => name === "spotifyPlayer").length;

  it("doesn't read the player straight after a change (it would flip the button back)", async () => {
    const tug = await playing();
    await tug.toggleSpotifyShuffle();
    await flush();
    expect(tug.spotifyPlayer?.shuffle).toBe(true);
    expect(reads()).toBe(0);
    await tug.cycleSpotifyRepeat();
    await flush();
    expect(tug.spotifyPlayer?.repeat).toBe("all");
    expect(reads()).toBe(0);
  });

  it("reads once things have settled, once for a burst of presses", async () => {
    const tug = await playing();
    await tug.toggleSpotifyShuffle();
    vi.advanceTimersByTime(1000);
    await tug.cycleSpotifyRepeat();
    vi.advanceTimersByTime(1000);
    expect(reads()).toBe(0);
    impl.spotifyPlayer = async () => ({ shuffle: true, repeat: "all" });
    vi.advanceTimersByTime(600);
    await flush();
    expect(reads()).toBe(1);
    expect(tug.spotifyPlayer?.shuffle).toBe(true);
  });

  it("still goes back when Spotify refuses", async () => {
    const tug = await playing();
    impl.spotifySetShuffle = () => Promise.reject("No active device");
    await tug.toggleSpotifyShuffle();
    expect(tug.spotifyPlayer?.shuffle).toBe(false);
  });

  it("an older press failing doesn't undo a newer one", async () => {
    const tug = await playing();
    const first = deferred();
    impl.spotifySetRepeat = () => first.promise;
    const a = tug.cycleSpotifyRepeat(); // off → all, slow
    impl.spotifySetRepeat = () => Promise.resolve();
    await tug.cycleSpotifyRepeat(); // all → one
    first.reject("rate limited");
    await a;
    expect(tug.spotifyPlayer?.repeat).toBe("one");
  });
});

describe("loading older notifications", () => {
  const n = (id: number) => ({ id, removedAt: null }) as unknown as PhoneNotification;

  it("asks for one page at a time, however often it's called", async () => {
    const tug = useTugStore();
    tug.notifications = [n(100)];
    const page = deferred<PhoneNotification[]>();
    impl.listNotifications = () => page.promise;
    const a = tug.loadMore();
    const b = tug.loadMore();
    expect(called.filter(([name]) => name === "listNotifications")).toHaveLength(1);
    page.resolve([n(99)]);
    expect(await a).toBe(true);
    expect(await b).toBe(true);
    expect(tug.notifications.map((x) => x.id)).toEqual([100, 99]);
    // A short page was the last: nothing more to ask for.
    expect(tug.hasMore).toBe(false);
    expect(await tug.loadMore()).toBe(false);
    expect(called.filter(([name]) => name === "listNotifications")).toHaveLength(1);
  });

  it("drops a page that lands after the list was replaced", async () => {
    const tug = useTugStore();
    tug.notifications = [n(100)];
    const page = deferred<PhoneNotification[]>();
    impl.listNotifications = () => page.promise;
    const a = tug.loadMore();
    tug.notifications = [];
    page.resolve([n(99)]);
    expect(await a).toBe(false);
    expect(tug.notifications).toEqual([]);
  });
});
