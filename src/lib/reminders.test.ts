import { describe, expect, it } from "vitest";
import {
  addReminder,
  choices,
  MAX_REMINDERS,
  parseReminders,
  reminderFor,
  reminderId,
  reminderTitle,
  removeReminder,
  splitDue,
  whenLabel,
  type Reminder,
} from "./reminders";
import { clockTime } from "./format";

const MINUTE = 60 * 1000;
// Tuesday 2026-10-27.
const day = (h: number, m = 0, d = 27) => new Date(2026, 9, d, h, m, 0, 0);

function r(over: Partial<Reminder> = {}): Reminder {
  return { id: "a", at: day(15).getTime(), title: "Jane Doe", body: "Are we still on?", notificationId: 7, threadKey: null, createdAt: day(14).getTime(), ...over };
}

describe("reminders", () => {
  it("offers soon, later, this evening while it's a while off, and tomorrow morning", () => {
    const now = day(10, 15);
    const c = choices(now);
    expect(c.map((x) => x.at)).toEqual([
      now.getTime() + 20 * MINUTE,
      now.getTime() + 60 * MINUTE,
      now.getTime() + 180 * MINUTE,
      day(18).getTime(),
      day(9, 0, 28).getTime(),
    ]);
    expect(c[3].label).toBe(`This evening (${clockTime(day(18))})`);
    expect(c[4].label).toBe(`Tomorrow morning (${clockTime(day(9, 0, 28))})`);
    // Close to (or after) 6 PM, "this evening" isn't offered.
    expect(choices(day(16, 45)).map((x) => x.label)).not.toContain(`This evening (${clockTime(day(18))})`);
    expect(choices(day(22)).length).toBe(4);
  });

  it("says when, in the person's own clock, with tomorrow and later days named", () => {
    const now = day(10).getTime();
    expect(whenLabel(day(15, 30).getTime(), now)).toBe(clockTime(day(15, 30)));
    expect(whenLabel(day(9, 0, 28).getTime(), now)).toBe(`Tomorrow ${clockTime(day(9, 0, 28))}`);
    expect(whenLabel(day(9, 0, 30).getTime(), now)).toMatch(new RegExp(`${clockTime(day(9, 0, 30))}$`));
  });

  it("keeps one reminder per conversation or notification, soonest first", () => {
    let list = addReminder([], r({ id: "1", at: day(15).getTime() }));
    list = addReminder(list, r({ id: "2", at: day(12).getTime(), notificationId: 8 }));
    expect(list.map((x) => x.id)).toEqual(["2", "1"]);
    list = addReminder(list, r({ id: "3", at: day(16).getTime() }));
    expect(list.map((x) => x.id)).toEqual(["2", "3"]);
    // A conversation's reminder is per conversation, whichever notification it started from.
    let t = addReminder([], r({ id: "x", threadKey: "jane", notificationId: 1 }));
    t = addReminder(t, r({ id: "y", threadKey: "jane", notificationId: 2 }));
    expect(t.map((x) => x.id)).toEqual(["y"]);
    expect(removeReminder(t, "y")).toEqual([]);
  });

  it("caps how many are kept", () => {
    let list: Reminder[] = [];
    for (let i = 0; i < MAX_REMINDERS + 5; i++) list = addReminder(list, r({ id: `${i}`, notificationId: i, at: day(12).getTime() + i }));
    expect(list).toHaveLength(MAX_REMINDERS);
  });

  it("finds a row's reminder by its conversation or any of its notifications", () => {
    const list = [r({ id: "n", notificationId: 7 }), r({ id: "t", threadKey: "zoe", notificationId: null })];
    expect(reminderFor(list, { threadKey: null, notificationIds: [5, 7] })?.id).toBe("n");
    expect(reminderFor(list, { threadKey: "zoe", notificationIds: [] })?.id).toBe("t");
    expect(reminderFor(list, { threadKey: "sam", notificationIds: [7] })).toBeNull();
  });

  it("splits what's due from what's still to come", () => {
    const list = [r({ id: "past", at: day(9).getTime() }), r({ id: "now", at: day(10).getTime() }), r({ id: "later", at: day(11).getTime() })];
    const { due, pending } = splitDue(list, day(10).getTime());
    expect(due.map((x) => x.id)).toEqual(["past", "now"]);
    expect(pending.map((x) => x.id)).toEqual(["later"]);
  });

  it("a reminder that came due while tug was closed says when it was due", () => {
    const x = r({ at: day(15).getTime() });
    expect(reminderTitle(x, day(15, 1).getTime())).toBe("Reminder · Jane Doe");
    expect(reminderTitle(x, day(17).getTime())).toBe(`Reminder (was due ${clockTime(day(15))}) · Jane Doe`);
  });

  it("makes distinct ids", () => {
    expect(reminderId(1000, () => 0.5)).not.toBe(reminderId(1000, () => 0.25));
  });

  it("reads back what it saved and leaves out anything odd", () => {
    const good = [r({ id: "b", at: day(16).getTime() }), r({ id: "a", at: day(15).getTime(), threadKey: "jane", notificationId: null })];
    expect(parseReminders(JSON.stringify(good)).map((x) => x.id)).toEqual(["a", "b"]);
    expect(parseReminders(undefined)).toEqual([]);
    expect(parseReminders("{nope")).toEqual([]);
    expect(parseReminders(JSON.stringify({ id: "x" }))).toEqual([]);
    const odd = [{ id: "n", at: "soon", title: "x", body: "", notificationId: 1 }, { id: "t", at: 5, title: "x", body: "" }, null];
    expect(parseReminders(JSON.stringify(odd))).toEqual([]);
  });
});
