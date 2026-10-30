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

  /** The first call answers when the test says; every later one saves at once. */
  function firstSlow(name: string) {
    const first = deferred();
    let calls = 0;
    impl[name] = () => (calls++ === 0 ? first.promise : Promise.resolve());
    return first;
  }

  it("on → off → on: the first save failing last doesn't undo the third press", async () => {
    const tug = useTugStore();
    tug.settings = { ...tug.settings, doNotDisturb: false };
    const first = firstSlow("setSetting");
    const a = tug.setSetting("doNotDisturb", true);
    await tug.setSetting("doNotDisturb", false);
    await tug.setSetting("doNotDisturb", true);
    first.reject("Couldn't save");
    expect(await a).toBe(false);
    expect(tug.settings.doNotDisturb).toBe(true);
  });

  it("setAdvertising: off → on → off with the first failing last keeps it off", async () => {
    const tug = useTugStore();
    expect(tug.advertiseEnabled).toBe(true);
    const first = firstSlow("setAdvertising");
    const a = tug.setAdvertising(false);
    await tug.setAdvertising(true);
    await tug.setAdvertising(false);
    first.reject("Bluetooth is off");
    await a;
    expect(tug.advertiseEnabled).toBe(false);
  });

  it("the type-the-code switch: on → off → on with the first save failing last stays on", async () => {
    const tug = useTugStore();
    tug.settings = { ...tug.settings, typeCodeHotkey: false };
    tug.codeHotkey = { state: "off", keys: tug.settings.typeCodeKeys };
    impl.setCodeHotkey = async (...args: never[]) => ({ state: args[0] ? "on" : "off", keys: args[1] });
    const first = firstSlow("setSetting");
    const keys = tug.settings.typeCodeKeys;
    const a = tug.setCodeHotkey(true, keys);
    await flush();
    await tug.setCodeHotkey(false, keys);
    await tug.setCodeHotkey(true, keys);
    first.reject("Couldn't save");
    await a;
    expect(tug.settings.typeCodeHotkey).toBe(true);
    expect(tug.codeHotkey.state).toBe("on");
    // The backend isn't put back to off behind the newest press.
    const registrations = called.filter(([name]) => name === "setCodeHotkey").map(([, args]) => args[0]);
    expect(registrations).toEqual([true, false, true]);
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

  it("the quick text switch starts off, and turning it on registers it and saves its own settings", async () => {
    const tug = useTugStore();
    expect(tug.settings.quickTextHotkey).toBe(false);
    impl.setQuickTextHotkey = async (...args: never[]) => ({ state: args[0] ? "on" : "off", keys: args[1] });
    await tug.setQuickTextHotkey(true, "ctrl+shift+u");
    expect(tug.settings.quickTextHotkey).toBe(true);
    expect(tug.settings.quickTextKeys).toBe("ctrl+shift+u");
    expect(tug.quickTextHotkey).toEqual({ state: "on", keys: "ctrl+shift+u" });
    const saved = called.filter(([name]) => name === "setSetting").map(([, args]) => args);
    expect(saved).toEqual([
      ["ui.quickTextHotkey", "true"],
      ["ui.quickTextKeys", "ctrl+shift+u"],
    ]);
    // Code fill's shortcut is left alone.
    expect(called.some(([name]) => name === "setCodeHotkey")).toBe(false);
  });

  it("the quick text switch stays on with the in-use warning, and never registers a shortcut it doesn't offer", async () => {
    const tug = useTugStore();
    impl.setQuickTextHotkey = async (...args: never[]) => ({ state: "inUse", keys: args[1] });
    await tug.setQuickTextHotkey(true, "ctrl+shift+v");
    expect(tug.settings.quickTextHotkey).toBe(true);
    expect(tug.quickTextHotkey.state).toBe("inUse");
    expect(called.find(([name]) => name === "setQuickTextHotkey")?.[1]).toEqual([true, "ctrl+shift+y"]);
  });
});

describe("the quick text composer", () => {
  it("gives the keyboard back once when it closes, and sends nothing", async () => {
    const tug = useTugStore();
    tug.openQuickText();
    expect(tug.quickTextOpen).toBe(true);
    expect(tug.overlayOpen).toBe(true);
    tug.closeQuickText(true);
    tug.closeQuickText(true);
    expect(tug.quickTextOpen).toBe(false);
    expect(called.filter(([name]) => name === "quickTextDone").map(([, args]) => args)).toEqual([[true]]);
    expect(called.some(([name]) => name === "sendMessage")).toBe(false);
  });

  it("keeps tug in front when a text didn't go", () => {
    const tug = useTugStore();
    tug.openQuickText();
    tug.closeQuickText(false);
    expect(called.filter(([name]) => name === "quickTextDone").map(([, args]) => args)).toEqual([[false]]);
  });

  it("covers New message and search rather than opening under them", () => {
    const tug = useTugStore();
    tug.pickerOpen = true;
    tug.searchOpen = true;
    tug.openQuickText();
    expect(tug.pickerOpen).toBe(false);
    expect(tug.searchOpen).toBe(false);
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

describe("unread texts count Messages only", () => {
  let nid = 500;
  const note = (appId: string, title: string): PhoneNotification => ({
    id: nid++,
    appId,
    appName: null,
    category: "social",
    title,
    subtitle: "",
    message: "hi",
    postedAt: null,
    // After the store's "seen since" (its creation), so each one is new.
    receivedAt: Date.now() + 60_000,
    flags: { silent: false, important: false, preExisting: false, positiveAction: false, negativeAction: true },
    positiveLabel: "",
    negativeLabel: "Clear",
    removedAt: null,
    live: true,
    fresh: false,
  });

  it("WhatsApp, Teams and other chat apps never add to the Messages badge, tray or taskbar dot", async () => {
    const tug = useTugStore();
    tug.notifications = [
      note("com.apple.MobileSMS", "Jane Doe"),
      note("com.apple.MobileSMS", "Jane Doe"),
      note("net.whatsapp.WhatsApp", "Jane Doe"),
      note("com.microsoft.skype.teams", "Jane Doe"),
      note("com.tinyspeck.chatlyio", "Sam"),
      note("com.example.newchat", "Sam"),
    ];
    await flush();
    expect(tug.unreadTexts).toBe(2);
    // Only other apps waiting: nothing unread in Messages, and the taskbar is told so.
    tug.notifications = [note("net.whatsapp.WhatsApp", "Sam"), note("com.microsoft.skype.teams", "Sam")];
    await flush();
    expect(tug.unreadTexts).toBe(0);
    expect(called.filter(([name]) => name === "setUnread").at(-1)?.[1]).toEqual([0]);
  });
});

describe("music and videos on this PC pause while the iPhone rings", () => {
  const call = (id: number): PhoneNotification => ({
    id,
    appId: "com.apple.mobilephone",
    appName: null,
    category: "incomingCall",
    title: "Jane Doe",
    subtitle: "",
    message: "Incoming Call",
    postedAt: null,
    receivedAt: Date.now(),
    flags: { silent: false, important: true, preExisting: false, positiveAction: true, negativeAction: true },
    positiveLabel: "Answer",
    negativeLabel: "Decline",
    removedAt: null,
    live: true,
    fresh: true,
  });
  const media = () => called.filter(([name]) => name.startsWith("pcMedia")).map(([name]) => name);
  /** Read afresh (TypeScript would keep `flash` narrowed to null after the test clears it). */
  const flashOf = (tug: ReturnType<typeof useTugStore>) => tug.flash;
  const stopRinging = (tug: ReturnType<typeof useTugStore>) =>
    (tug.notifications = tug.notifications.map((n) => ({ ...n, live: false, removedAt: Date.now() })));

  afterEach(() => vi.useRealTimers());

  it("pauses when a call rings, and plays again when it's declined in tug", async () => {
    const tug = useTugStore();
    impl.pcMediaPause = () => Promise.resolve(1);
    tug.notifications = [call(1)];
    await flush();
    expect(media()).toEqual(["pcMediaPause"]);
    expect(await tug.respondToCall(tug.notifications[0], false)).toBe(true);
    await flush();
    expect(media()).toEqual(["pcMediaPause", "pcMediaResume"]);
    // The phone dropping the call afterwards doesn't play anything twice.
    stopRinging(tug);
    await flush();
    expect(media()).toEqual(["pcMediaPause", "pcMediaResume"]);
  });

  it("knows a call declined here even when the phone drops it before Decline answers", async () => {
    const tug = useTugStore();
    impl.pcMediaPause = () => Promise.resolve(1);
    tug.notifications = [call(2)];
    await flush();
    const declining = deferred();
    impl.performAction = () => declining.promise;
    const done = tug.respondToCall(tug.notifications[0], false);
    stopRinging(tug);
    await flush();
    expect(media()).toEqual(["pcMediaPause", "pcMediaResume"]);
    declining.resolve();
    expect(await done).toBe(true);
    expect(media()).toEqual(["pcMediaPause", "pcMediaResume"]);
  });

  it("plays again only after the pause has answered", async () => {
    const tug = useTugStore();
    const pausing = deferred<number>();
    impl.pcMediaPause = () => pausing.promise;
    tug.notifications = [call(3)];
    await flush();
    await tug.respondToCall(tug.notifications[0], false);
    await flush();
    expect(media()).toEqual(["pcMediaPause"]);
    pausing.resolve(1);
    await flush();
    expect(media()).toEqual(["pcMediaPause", "pcMediaResume"]);
  });

  it("offers Resume after a call answered in tug instead of playing by itself", async () => {
    const tug = useTugStore();
    impl.pcMediaPause = () => Promise.resolve(1);
    tug.notifications = [call(4)];
    await flush();
    await tug.respondToCall(tug.notifications[0], true);
    await flush();
    expect(media()).toEqual(["pcMediaPause"]);
    // "Answered" shows first; the offer follows once it's gone.
    expect(tug.flash?.text).toBe("Answered. Talk on your iPhone.");
    tug.flash = null;
    await flush();
    const offer = flashOf(tug);
    expect(offer?.text).toBe("tug paused what was playing on this PC for your call.");
    offer?.action?.run();
    await flush();
    expect(media()).toEqual(["pcMediaPause", "pcMediaResume"]);
  });

  it("offers nothing when nothing was playing", async () => {
    const tug = useTugStore();
    impl.pcMediaPause = () => Promise.resolve(0);
    tug.notifications = [call(5)];
    await flush();
    await tug.respondToCall(tug.notifications[0], true);
    tug.flash = null;
    await flush();
    expect(flashOf(tug)).toBeNull();
  });

  it("treats a call that stopped ringing with no missed call as answered", async () => {
    vi.useFakeTimers();
    const tug = useTugStore();
    impl.pcMediaPause = () => Promise.resolve(1);
    tug.notifications = [call(6)];
    await flush();
    stopRinging(tug);
    await flush();
    vi.advanceTimersByTime(10_000);
    await flush();
    // Nothing plays by itself; Resume is offered instead.
    expect(media()).toEqual(["pcMediaPause"]);
    expect(tug.flash?.action?.label).toBe("Resume");
  });

  it("does nothing with the setting off", async () => {
    const tug = useTugStore();
    tug.settings = { ...tug.settings, pauseMediaOnCall: false };
    tug.notifications = [call(7)];
    await flush();
    expect(media()).toEqual([]);
  });
});
