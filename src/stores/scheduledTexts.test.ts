import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";

let saved: Record<string, string> = {};
vi.mock("../lib/ipc", () => ({
  api: {
    getSettings: vi.fn(async () => ({ ...saved })),
    setSetting: vi.fn(async (key: string, value: string) => {
      saved[key] = value;
    }),
  },
}));
const sends: [string, string][] = [];
const notes: string[] = [];
let connected = true;
let outcome: "sent" | "unconfirmed" | "failed" | "error" = "sent";
let release: (() => void) | null = null;
vi.mock("./tug", () => ({
  useTugStore: () => ({
    status: { services: { messages: connected } },
    async sendMessage(address: string, text: string) {
      sends.push([address, text]);
      if (release === null) return outcome;
      await new Promise<void>((r) => (release = r));
      return outcome;
    },
    notify: (_kind: string, text: string) => void notes.push(text),
    openThread: () => undefined,
  }),
}));

const { useScheduledTextsStore, SCHEDULED_SETTING } = await import("./scheduledTexts");
const zoe = { address: "+15550100142", name: "Zoe", threadKey: "zoe" };

describe("send later store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    saved = {};
    sends.length = 0;
    notes.length = 0;
    connected = true;
    outcome = "sent";
    release = null;
  });

  it("sends a due text once, and says so", async () => {
    const s = useScheduledTextsStore();
    const now = Date.now();
    await s.schedule(zoe, "Running late", now + 1000);
    await s.check(now);
    expect(sends).toEqual([]);
    await s.check(now + 2000);
    await s.check(now + 3000);
    expect(sends).toEqual([["+15550100142", "Running late"]]);
    expect(notes).toEqual(["Sent your scheduled text to Zoe."]);
    expect(JSON.parse(saved[SCHEDULED_SETTING])).toEqual([]);
  });

  it("is off the saved list before it's sent, and a look during the send doesn't send it again", async () => {
    const s = useScheduledTextsStore();
    const now = Date.now();
    await s.schedule(zoe, "Running late", now);
    release = () => undefined;
    const first = s.check(now + 1000);
    await Promise.resolve();
    await new Promise((r) => setTimeout(r, 0));
    expect(JSON.parse(saved[SCHEDULED_SETTING])).toEqual([]);
    await s.check(now + 2000);
    release();
    await first;
    expect(sends).toHaveLength(1);
  });

  it("waits for the phone, and holds a text that's too late", async () => {
    const s = useScheduledTextsStore();
    const now = Date.now();
    await s.schedule(zoe, "Running late", now);
    connected = false;
    await s.check(now + 60_000);
    expect(sends).toEqual([]);
    await s.check(now + 31 * 60_000);
    expect(s.list[0].state).toBe("held");
    expect(notes[0]).toMatch(/^Your text to Zoe wasn't sent: it was due at /);
    connected = true;
    await s.check(now + 32 * 60_000);
    expect(sends).toEqual([]);
    await s.sendNow(s.list[0].id);
    expect(sends).toHaveLength(1);
    expect(s.list).toEqual([]);
  });

  it("a send that recorded nothing comes back held instead of vanishing", async () => {
    const s = useScheduledTextsStore();
    const now = Date.now();
    await s.schedule(zoe, "Running late", now);
    outcome = "error";
    await s.check(now + 1000);
    expect(s.list).toHaveLength(1);
    expect(s.list[0].state).toBe("held");
  });

  it("discard removes it", async () => {
    const s = useScheduledTextsStore();
    await s.schedule(zoe, "Running late", Date.now() + 60_000);
    s.discard(s.list[0].id);
    expect(s.list).toEqual([]);
    expect(s.inThread("zoe")).toEqual([]);
  });
});
