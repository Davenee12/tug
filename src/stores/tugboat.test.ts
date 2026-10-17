import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { TUGBOAT_OFF } from "../lib/tugboat";
import type { TugboatStatus } from "../types/protocol";

// One backend stand-in for both suites: the ordering tests (#132) hold reads and answers until
// they release them; the open/close race tests (#134) control when Stop answers.
const handlers = new Map<string, (payload: unknown) => void>();
const held: Record<string, (s: TugboatStatus) => void> = {};
const hold = (name: string) => () => new Promise<TugboatStatus>((resolve) => (held[name] = resolve));

function deferred<T = void>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((res) => (resolve = res));
  return { promise, resolve };
}
let stop = deferred();
let startWith: () => Promise<TugboatStatus>;

vi.mock("../lib/ipc", () => ({
  api: {
    tugboatStatus: vi.fn(hold("tugboatStatus")),
    tugboatStart: vi.fn(() => startWith()),
    tugboatStop: vi.fn(() => stop.promise),
  },
  on: vi.fn(async (event: string, fn: (payload: unknown) => void) => {
    handlers.set(event, fn);
    return () => handlers.delete(event);
  }),
}));

const { useTugboatStore } = await import("./tugboat");

const waiting = (rev: number): TugboatStatus => ({
  ...TUGBOAT_OFF,
  rev,
  phase: "waiting",
  url: "http://192.0.2.1:5000/#secret",
  address: "192.0.2.1:5000",
});
const off = (rev: number): TugboatStatus => ({ ...TUGBOAT_OFF, rev });
const flush = () => new Promise((r) => setTimeout(r, 0));
const init = (t: ReturnType<typeof useTugboatStore>) => t.init({ onSkipped: () => undefined, onText: () => undefined });

describe("tugboat store ordering", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    handlers.clear();
    stop = deferred();
    stop.resolve(); // Stop answers at once here
    startWith = hold("tugboatStart");
  });

  it("the startup read doesn't overwrite an event that overtook it", async () => {
    const t = useTugboatStore();
    const started = init(t);
    await flush(); // listeners in, read in flight
    handlers.get("tugboat-status")!(waiting(3));
    held.tugboatStatus(off(2));
    await started;
    expect(t.status.phase).toBe("waiting");
  });

  it("Start's answer composed before a newer event is ignored", async () => {
    const t = useTugboatStore();
    const started = init(t);
    await flush();
    held.tugboatStatus(off(1));
    await started;
    const starting = t.start();
    handlers.get("tugboat-status")!({ ...waiting(3), phase: "connected", phone: "iPhone" });
    held.tugboatStart(waiting(2));
    await starting;
    expect(t.status.phase).toBe("connected");
  });

  it("after Close, a copy composed before it can't bring the session back; a newer one can", async () => {
    const t = useTugboatStore();
    const started = init(t);
    await flush();
    held.tugboatStatus(waiting(4));
    await started;
    await t.close();
    expect(t.status.phase).toBe("off");
    handlers.get("tugboat-status")!(waiting(3)); // a progress update composed before the stop
    expect(t.status.phase).toBe("off");
    handlers.get("tugboat-status")!(waiting(6));
    expect(t.status.phase).toBe("waiting");
  });
});

const running: TugboatStatus = { ...TUGBOAT_OFF, phase: "waiting", url: "http://192.0.2.1:1234/#s" } as TugboatStatus;

describe("Tugboat panel open/close races", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    handlers.clear();
    stop = deferred();
    startWith = async () => running;
  });

  it("a slow stop doesn't blank a session reopened meanwhile", async () => {
    const t = useTugboatStore();
    const closing = t.close();
    await t.show(); // opened again before the stop answered
    expect(t.status.url).toBe(running.url);
    stop.resolve();
    await closing;
    expect(t.open).toBe(true);
    expect(t.status.url).toBe(running.url);
  });

  it("a plain close still shows Tugboat off", async () => {
    const t = useTugboatStore();
    await t.show();
    const closing = t.close();
    stop.resolve();
    await closing;
    expect(t.open).toBe(false);
    expect(t.status.phase).toBe("off");
  });

  it("a start that answers after the panel closed doesn't bring the code back", async () => {
    const t = useTugboatStore();
    const started = deferred<TugboatStatus>();
    startWith = () => started.promise;
    const showing = t.show();
    const closing = t.close();
    stop.resolve();
    await closing;
    started.resolve(running);
    await showing;
    expect(t.open).toBe(false);
    expect(t.status.url).toBeNull();
  });
});
