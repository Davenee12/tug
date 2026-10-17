// The tug store's startup and refresh, against the interleavings the audit found: an event that
// lands while a read is in flight must win, and a failed read must not hold tug half-started.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { DeviceStatus, NowPlaying, PhoneNotification } from "../types/protocol";

// --- A minimal window/document: the store only needs timers and event hooks here. -------------
const noop = () => undefined;
vi.stubGlobal("document", {
  hidden: false,
  hasFocus: () => true,
  addEventListener: noop,
  removeEventListener: noop,
  documentElement: { style: { setProperty: noop }, classList: { toggle: noop, add: noop, remove: noop } },
});
vi.stubGlobal("window", {
  setTimeout: (fn: () => void, ms?: number) => setTimeout(fn, ms),
  clearTimeout: (id?: number) => clearTimeout(id),
  setInterval: (fn: () => void, ms?: number) => setInterval(fn, ms),
  clearInterval: (id?: number) => clearInterval(id),
  requestAnimationFrame: (fn: () => void) => setTimeout(fn, 0),
  cancelAnimationFrame: (id?: number) => clearTimeout(id),
  addEventListener: noop,
  removeEventListener: noop,
  matchMedia: () => ({ matches: false, addEventListener: noop, removeEventListener: noop }),
});

// --- The backend: every read answers at once unless a test holds it. -------------------------
type Handler = (payload: unknown) => void;
const handlers = new Map<string, Handler>();
const held = new Map<string, Array<{ resolve: (v: unknown) => void; reject: (e: unknown) => void }>>();
const holding = new Set<string>();
const failing = new Map<string, number>();
const calls: string[] = [];
let answers: Record<string, unknown> = {};

const api = new Proxy(
  {},
  {
    get: (_t, name: string) =>
      (..._args: unknown[]) => {
        calls.push(name);
        const fails = failing.get(name) ?? 0;
        if (fails > 0) {
          failing.set(name, fails - 1);
          return Promise.reject(new Error(`${name} failed`));
        }
        if (holding.has(name)) {
          holding.delete(name);
          return new Promise((resolve, reject) => {
            held.set(name, [...(held.get(name) ?? []), { resolve, reject }]);
          });
        }
        return Promise.resolve(answers[name]);
      },
  },
);

vi.mock("../lib/ipc", () => ({
  api,
  on: async (event: string, fn: Handler) => {
    handlers.set(event, fn);
    return () => handlers.delete(event);
  },
  errorMessage: (e: unknown) => String(e),
}));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: async () => "0.0.0" }));
vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: async () => true,
  requestPermission: async () => "granted",
}));

const { useTugStore } = await import("./tug");

const status = (extra: Partial<DeviceStatus>): DeviceStatus => ({
  radio: "on",
  peripheralSupported: true,
  advertising: "on",
  device: { id: "dev", name: "Phone", model: null },
  connection: "connected",
  battery: 80,
  services: { notifications: true, media: true, battery: true, messages: true },
  lastError: null,
  lastErrorAt: null,
  pairingStale: false,
  awaitingPhoneAllow: false,
  awaitingUnlock: false,
  reconnecting: false,
  messagesError: null,
  contactsError: null,
  contactsShared: true,
  contactsOff: false,
  textsPairing: "ok",
  textsDevice: null,
  liveTexts: "active",
  ...extra,
});

const nowPlaying = (rev: number, title: string): NowPlaying => ({
  rev,
  player: "Music",
  state: "playing",
  rate: 1,
  elapsed: 0,
  elapsedAt: 0,
  volume: null,
  title,
  artist: null,
  album: null,
  duration: null,
  repeat: null,
  available: [],
});

const notification = (id: number, extra: Partial<PhoneNotification> = {}): PhoneNotification => ({
  id,
  appId: "com.example.app",
  appName: "Example",
  category: "other",
  title: "Title",
  subtitle: "",
  message: "Body",
  postedAt: null,
  receivedAt: 1,
  flags: { silent: false, important: false, preExisting: true, positiveAction: false, negativeAction: false },
  positiveLabel: "",
  negativeLabel: "",
  removedAt: null,
  live: true,
  fresh: false,
  ...extra,
});

const settle = () => new Promise((r) => setTimeout(r, 0));
const emit = (event: string, payload: unknown) => handlers.get(event)!(payload);
const release = (name: string, value: unknown) => held.get(name)!.shift()!.resolve(value);

describe("tug store: startup and refresh ordering", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    handlers.clear();
    held.clear();
    holding.clear();
    failing.clear();
    calls.length = 0;
    answers = {
      getStatus: status({ rev: 1 }),
      getNowPlaying: nowPlaying(1, "Read"),
      listNotifications: [],
      listMessages: [],
      getContacts: [],
      getCalls: [],
      getSettings: {},
      getAutostart: false,
      spotifyStatus: { connected: false },
    };
  });
  afterEach(() => {
    useTugStore().dispose();
    vi.useRealTimers();
  });

  it("an event that lands while the startup status read is in flight wins (stuck on Connecting)", async () => {
    holding.add("getStatus");
    const tug = useTugStore();
    const started = tug.init();
    await settle();
    emit("device-status", status({ rev: 5, connection: "connected" }));
    release("getStatus", status({ rev: 4, connection: "connecting" }));
    await started;
    expect(tug.status.connection).toBe("connected");
    expect(tug.status.rev).toBe(5);
  });

  it("an older status event after a newer one is dropped", async () => {
    const tug = useTugStore();
    await tug.init();
    emit("device-status", status({ rev: 7, connection: "connected" }));
    emit("device-status", status({ rev: 6, connection: "connecting" }));
    expect(tug.status.connection).toBe("connected");
  });

  it("an older status can't mark live notifications dead", async () => {
    answers.listNotifications = [notification(1)];
    const tug = useTugStore();
    await tug.init();
    emit("device-status", status({ rev: 9, connection: "connected" }));
    emit("device-status", status({ rev: 8, connection: "disconnected" }));
    expect(tug.notifications[0].live).toBe(true);
  });

  it("a Now Playing event that lands while the startup read is in flight wins", async () => {
    holding.add("getNowPlaying");
    const tug = useTugStore();
    const started = tug.init();
    await settle();
    emit("now-playing", nowPlaying(3, "New song"));
    release("getNowPlaying", nowPlaying(2, "Old song"));
    await started;
    expect(tug.nowPlaying.title).toBe("New song");
  });

  it("a failed startup read doesn't hold pop-ups, and is read again", async () => {
    vi.useFakeTimers();
    failing.set("getContacts", 1);
    answers.getContacts = [{ name: "Sam", numbers: ["+15550100"] }];
    // Already low at launch: the battery pop-up waits for startup to finish.
    answers.getStatus = status({ rev: 1, battery: 5 });
    const tug = useTugStore();
    await tug.init();
    await vi.advanceTimersByTimeAsync(0);
    expect(calls).toContain("showToast");
    expect(tug.contacts).toEqual([]);
    await vi.advanceTimersByTimeAsync(2_000);
    expect(calls.filter((c) => c === "getContacts")).toHaveLength(2);
    expect(tug.contacts).toHaveLength(1);
  });

  it("says so once, plainly, when a retry fails too, and keeps trying", async () => {
    vi.useFakeTimers();
    failing.set("getCalls", 3);
    const tug = useTugStore();
    await tug.init();
    expect(tug.flash).toBeNull();
    await vi.advanceTimersByTimeAsync(2_000);
    expect(tug.flash?.text).toBe("tug couldn't load everything yet. It keeps trying by itself.");
    await vi.advanceTimersByTimeAsync(5_000 + 15_000);
    expect(calls.filter((c) => c === "getCalls")).toHaveLength(4);
  });

  it("a removal that lands during a notification refresh isn't undone", async () => {
    answers.listNotifications = [notification(1)];
    const tug = useTugStore();
    await tug.init();
    // A contacts update re-reads the loaded notifications; hold that read open.
    holding.add("listNotifications");
    emit("contacts", []);
    await settle();
    emit("notification-removed", 1);
    // The read was taken before the phone cleared it.
    release("listNotifications", [notification(1)]);
    await settle();
    expect(tug.notifications[0].removedAt).not.toBeNull();
    expect(tug.notifications[0].live).toBe(false);
  });

  it("a removal that lands during the startup read isn't undone either", async () => {
    holding.add("listNotifications");
    const tug = useTugStore();
    const started = tug.init();
    await settle();
    emit("notification-removed", 2);
    release("listNotifications", [notification(2)]);
    await started;
    expect(tug.notifications[0]).toMatchObject({ id: 2, live: false });
    expect(tug.notifications[0].removedAt).not.toBeNull();
  });
});
