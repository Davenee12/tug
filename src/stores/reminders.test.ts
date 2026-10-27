import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { Reminder } from "../lib/reminders";

let saved: Record<string, string> = {};
let readFails = false;
const writes: string[] = [];
vi.mock("../lib/ipc", () => ({
  api: {
    getSettings: vi.fn(async () => {
      if (readFails) throw new Error("store busy");
      return { ...saved };
    }),
    setSetting: vi.fn(async (key: string, value: string) => {
      writes.push(key);
      saved[key] = value;
    }),
  },
}));
const shown: Reminder[] = [];
vi.mock("./tug", () => ({ useTugStore: () => ({ showReminder: async (r: Reminder) => void shown.push(r) }) }));

const { useRemindersStore, REMINDERS_SETTING } = await import("./reminders");

const target = { title: "Jane Doe", body: "Are we still on?", notificationIds: [4, 9], threadKey: "jane" };

describe("reminders store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    saved = {};
    readFails = false;
    writes.length = 0;
    shown.length = 0;
  });

  it("saves a reminder against the row's newest notification and its conversation", async () => {
    const s = useRemindersStore();
    await s.remind(target, Date.now() + 60_000);
    expect(s.list).toHaveLength(1);
    expect(s.list[0]).toMatchObject({ notificationId: 9, threadKey: "jane", title: "Jane Doe" });
    expect(JSON.parse(saved[REMINDERS_SETTING])).toHaveLength(1);
    expect(s.forTarget({ threadKey: "jane", notificationIds: [] })?.id).toBe(s.list[0].id);
  });

  it("brings back what's due, once, and keeps the rest", async () => {
    const s = useRemindersStore();
    const now = Date.now();
    await s.remind(target, now + 1000);
    await s.remind({ ...target, threadKey: "zoe", title: "Zoe" }, now + 60 * 60_000);
    s.check(now + 2000);
    s.check(now + 3000);
    expect(shown.map((r) => r.title)).toEqual(["Jane Doe"]);
    expect(s.list.map((r) => r.title)).toEqual(["Zoe"]);
    expect(JSON.parse(saved[REMINDERS_SETTING])).toHaveLength(1);
  });

  it("brings back, at startup, what came due while tug was closed", async () => {
    saved[REMINDERS_SETTING] = JSON.stringify([
      { id: "a", at: Date.now() - 3_600_000, title: "Gmail", body: "Invoice", notificationId: 3, threadKey: null, createdAt: 0 },
    ]);
    const s = useRemindersStore();
    await s.init();
    s.dispose();
    expect(shown.map((r) => r.id)).toEqual(["a"]);
    expect(s.list).toEqual([]);
  });

  it("never wipes the saved reminders when it couldn't read them, and reads again next time", async () => {
    readFails = true;
    const s = useRemindersStore();
    // Not taken when it couldn't be kept: it wouldn't survive a restart.
    expect(await s.remind(target, Date.now() + 60_000)).toBe("unsaved");
    expect(s.list).toEqual([]);
    expect(writes).toHaveLength(0);
    readFails = false;
    expect(await s.remind(target, Date.now() + 60_000)).toBe("ok");
    expect(writes).toEqual([REMINDERS_SETTING]);
  });

  it("keeps one reminder for an app's stack after it gains a notification", async () => {
    const s = useRemindersStore();
    const gmail = { title: "Gmail", body: "Your order", notificationIds: [10], threadKey: null };
    await s.remind(gmail, Date.now() + 60_000);
    await s.remind({ ...gmail, notificationIds: [10, 11] }, Date.now() + 120_000);
    expect(s.list).toHaveLength(1);
    expect(s.list[0].notificationId).toBe(11);
  });

  it("cancelling removes it", async () => {
    const s = useRemindersStore();
    await s.remind(target, Date.now() + 60_000);
    s.cancel(s.list[0].id);
    expect(s.list).toEqual([]);
    expect(JSON.parse(saved[REMINDERS_SETTING])).toEqual([]);
  });
});
