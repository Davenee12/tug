import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { TUGBOAT_OFF } from "../lib/tugboat";
import type { TugboatStatus } from "../types/protocol";

function deferred<T = void>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((res) => (resolve = res));
  return { promise, resolve };
}
let stop = deferred();
let startWith: () => Promise<TugboatStatus>;
vi.mock("../lib/ipc", () => ({
  api: {
    tugboatStop: () => stop.promise,
    tugboatStart: () => startWith(),
  },
  on: async () => () => {},
}));

const { useTugboatStore } = await import("./tugboat");

const running: TugboatStatus = { ...TUGBOAT_OFF, phase: "waiting", url: "http://192.0.2.1:1234/#s" } as TugboatStatus;

describe("Tugboat panel open/close races", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
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
