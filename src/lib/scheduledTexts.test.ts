import { describe, expect, it } from "vitest";
import { addScheduled, forThread, held, LATE_MS, MAX_SCHEDULED, parseScheduled, plan, withoutScheduled, type ScheduledText } from "./scheduledTexts";

const T0 = new Date(2026, 9, 27, 18, 0).getTime();

function t(over: Partial<ScheduledText> = {}): ScheduledText {
  return {
    id: "a",
    address: "+15550100142",
    name: "Zoe",
    threadKey: "zoe",
    text: "Running late, start without me",
    at: T0,
    createdAt: T0 - 3_600_000,
    state: "waiting",
    ...over,
  };
}

describe("send later", () => {
  it("sends what's due on time while the phone is connected", () => {
    const list = [t({ id: "due" }), t({ id: "later", at: T0 + 60_000 })];
    expect(plan(list, T0 + 1000, true)).toEqual({ send: [list[0]], hold: [] });
  });

  it("waits while the phone isn't connected, then sends once it is", () => {
    const list = [t()];
    expect(plan(list, T0 + 60_000, false)).toEqual({ send: [], hold: [] });
    expect(plan(list, T0 + 10 * 60_000, true).send).toHaveLength(1);
  });

  it("never sends a text more than half an hour late by itself", () => {
    const list = [t()];
    expect(plan(list, T0 + LATE_MS, true).send).toHaveLength(1);
    expect(plan(list, T0 + LATE_MS + 1, true)).toEqual({ send: [], hold: [list[0]] });
    // Held whether or not the phone is there.
    expect(plan(list, T0 + LATE_MS + 1, false).hold).toHaveLength(1);
  });

  it("leaves held texts for the person", () => {
    const list = held([t()], ["a"]);
    expect(list[0].state).toBe("held");
    expect(plan(list, T0 + 2 * LATE_MS, true)).toEqual({ send: [], hold: [] });
  });

  it("keeps them soonest first, per conversation, and refuses beyond the cap", () => {
    let list: ScheduledText[] = [];
    list = addScheduled(list, t({ id: "2", at: T0 + 2 })) ?? [];
    list = addScheduled(list, t({ id: "1", at: T0 + 1 })) ?? [];
    list = addScheduled(list, t({ id: "x", threadKey: "sam", at: T0 })) ?? [];
    expect(list.map((x) => x.id)).toEqual(["x", "1", "2"]);
    expect(forThread(list, "zoe").map((x) => x.id)).toEqual(["1", "2"]);
    expect(withoutScheduled(list, "1").map((x) => x.id)).toEqual(["x", "2"]);
    let full: ScheduledText[] = [];
    for (let i = 0; i < MAX_SCHEDULED; i++) full = addScheduled(full, t({ id: `${i}` }))!;
    expect(addScheduled(full, t({ id: "one too many" }))).toBeNull();
  });

  it("reads back what it saved and leaves out anything odd", () => {
    const list = [t({ id: "b", at: T0 + 5 }), t({ id: "a", state: "held" })];
    expect(parseScheduled(JSON.stringify(list)).map((x) => [x.id, x.state])).toEqual([
      ["a", "held"],
      ["b", "waiting"],
    ]);
    expect(parseScheduled(undefined)).toEqual([]);
    expect(parseScheduled("{")).toEqual([]);
    const odd = [{ ...t(), text: "   " }, { ...t(), at: "6pm" }, { ...t(), address: "" }, null, 7];
    expect(parseScheduled(JSON.stringify(odd))).toEqual([]);
  });
});
