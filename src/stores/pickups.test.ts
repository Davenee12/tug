import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";

let saved: Record<string, string> = {};
let readFails = false;
const writes: [string, string][] = [];
vi.mock("../lib/ipc", () => ({
  api: {
    getSettings: vi.fn(async () => {
      if (readFails) throw new Error("store busy");
      return { ...saved };
    }),
    setSetting: vi.fn(async (key: string, value: string) => {
      writes.push([key, value]);
      saved[key] = value;
    }),
  },
}));

const { usePickupsStore, PICKUPS_SETTING } = await import("./pickups");

describe("pickups store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    saved = {};
    readFails = false;
    writes.length = 0;
  });

  it("adds to what was saved and saves the new count", async () => {
    saved[PICKUPS_SETTING] = JSON.stringify({ total: 41, week: 0, weekStart: 0, last: {} });
    const p = usePickupsStore();
    await p.count("reply");
    expect(p.state.total).toBe(42);
    expect(JSON.parse(saved[PICKUPS_SETTING]).total).toBe(42);
    expect(p.greetingLine).toBe("tug saved you 1 pickup of your phone this week");
  });

  it("a burst saves nothing new", async () => {
    const p = usePickupsStore();
    await p.count("music");
    await p.count("music");
    expect(writes).toHaveLength(1);
  });

  it("never overwrites the saved count when it couldn't read it", async () => {
    readFails = true;
    const p = usePickupsStore();
    await p.count("code");
    expect(p.state.total).toBe(1);
    expect(writes).toHaveLength(0);
  });
});
