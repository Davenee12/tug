import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { TUGBOAT_OFF } from "../lib/tugboat";
import type { TugboatStatus } from "../types/protocol";

const handlers = new Map<string, (payload: unknown) => void>();
const held: Record<string, (s: TugboatStatus) => void> = {};
const hold = (name: string) => () => new Promise<TugboatStatus>((resolve) => (held[name] = resolve));

vi.mock("../lib/ipc", () => ({
  api: {
    tugboatStatus: vi.fn(hold("tugboatStatus")),
    tugboatStart: vi.fn(hold("tugboatStart")),
    tugboatStop: vi.fn(async () => undefined),
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
