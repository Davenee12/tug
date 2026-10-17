import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { PcAudioStatus } from "../types/protocol";

let handler: ((s: PcAudioStatus) => void) | null = null;
let answer: { resolve: (s: PcAudioStatus) => void } | null = null;
let initial: PcAudioStatus;
/** Set to hold the startup read open until the test resolves it. */
let heldRead: { resolve: (s: PcAudioStatus) => void } | null = null;
let holdRead = false;

vi.mock("../lib/ipc", () => ({
  api: {
    pcAudioStatus: vi.fn(() =>
      holdRead ? new Promise<PcAudioStatus>((resolve) => (heldRead = { resolve })) : Promise.resolve(initial),
    ),
    pcAudioSet: vi.fn(() => new Promise<PcAudioStatus>((resolve) => (answer = { resolve }))),
  },
  on: vi.fn(async (_event: string, fn: (s: PcAudioStatus) => void) => {
    handler = fn;
    return () => undefined;
  }),
  errorMessage: (e: unknown) => String(e),
}));

const notify = vi.fn();
vi.mock("./tug", () => ({ useTugStore: () => ({ connected: true, notify }) }));

const { usePcAudioStore } = await import("./pcAudio");

const base: PcAudioStatus = { rev: 1, supported: true, state: "off", problem: null, auto: false };
const flush = () => new Promise((r) => setTimeout(r, 0));

describe("pcAudio store ordering", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    handler = null;
    answer = null;
    initial = base;
    holdRead = false;
    heldRead = null;
    notify.mockClear();
  });

  it("a fast failure's event beats the slower answer to Play on this PC (no stuck Connecting)", async () => {
    const pc = usePcAudioStore();
    await pc.init();
    const pressed = pc.set(true);
    // The attempt fails at once; its event (rev 3) reaches the window before the command's
    // answer, which was composed earlier (rev 2, Connecting).
    handler!({ ...base, rev: 3, problem: "notFound" });
    answer!.resolve({ ...base, rev: 2, state: "connecting" });
    await pressed;
    expect(pc.status).toMatchObject({ rev: 3, state: "off", problem: "notFound" });
  });

  it("the answer is taken when it's the newest", async () => {
    const pc = usePcAudioStore();
    await pc.init();
    const pressed = pc.set(true);
    answer!.resolve({ ...base, rev: 2, state: "connecting" });
    await pressed;
    expect(pc.status?.state).toBe("connecting");
    handler!({ ...base, rev: 3, state: "on" });
    expect(pc.status?.state).toBe("on");
  });

  it("the startup read doesn't overwrite an event that overtook it", async () => {
    holdRead = true;
    const pc = usePcAudioStore();
    const started = pc.init();
    await flush(); // listener registered, read in flight
    handler!({ ...base, rev: 5, state: "on" });
    heldRead!.resolve({ ...base, rev: 4, state: "connecting" });
    await started;
    expect(pc.status).toMatchObject({ rev: 5, state: "on" });
  });
});
