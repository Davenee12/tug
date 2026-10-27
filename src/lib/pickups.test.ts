import { describe, expect, it } from "vitest";
import { BURST_MS, EMPTY, mondayOf, parsePickups, pickupsLine, record, thisWeek, totalLine } from "./pickups";

// Wednesday 2026-10-28, 10:00 local.
const WED = new Date(2026, 9, 28, 10, 0, 0).getTime();
const MINUTE = 60 * 1000;

describe("pickups", () => {
  it("counts from Monday's midnight, local time", () => {
    expect(mondayOf(WED)).toBe(new Date(2026, 9, 26, 0, 0, 0).getTime());
    const sunday = new Date(2026, 10, 1, 23, 59).getTime();
    // Sunday still belongs to Monday's week.
    expect(mondayOf(sunday)).toBe(new Date(2026, 9, 26).getTime());
    expect(mondayOf(new Date(2026, 10, 2, 0, 0).getTime())).toBe(new Date(2026, 10, 2).getTime());
  });

  it("counts each kind once per burst", () => {
    let p = record(EMPTY, "music", WED);
    p = record(p, "music", WED + 30 * 1000);
    p = record(p, "music", WED + 60 * 1000);
    expect(p.total).toBe(1);
    // The burst follows the last press: still the same pickup a minute after the third skip.
    p = record(p, "music", WED + 60 * 1000 + BURST_MS - 1);
    expect(p.total).toBe(1);
    p = record(p, "music", WED + 60 * 1000 + 2 * BURST_MS);
    expect(p.total).toBe(2);
  });

  it("different kinds are different pickups", () => {
    let p = record(EMPTY, "reply", WED);
    p = record(p, "code", WED + 1000);
    p = record(p, "call", WED + 2000);
    expect(p).toMatchObject({ total: 3, week: 3 });
  });

  it("starts each week afresh but keeps the total", () => {
    let p = record(EMPTY, "reply", WED);
    p = record(p, "code", WED + MINUTE);
    const nextWeek = WED + 7 * 24 * 60 * MINUTE;
    expect(thisWeek(p, nextWeek)).toBe(0);
    expect(pickupsLine(p, nextWeek)).toBeNull();
    p = record(p, "reply", nextWeek);
    expect(p).toMatchObject({ total: 3, week: 1, weekStart: mondayOf(nextWeek) });
  });

  it("a clock that went backwards still counts", () => {
    const p = record(record(EMPTY, "reply", WED), "reply", WED - 10 * MINUTE);
    expect(p.total).toBe(2);
  });

  it("says it plainly, and nothing before the first", () => {
    expect(pickupsLine(EMPTY, WED)).toBeNull();
    expect(totalLine(EMPTY)).toBeNull();
    const one = record(EMPTY, "code", WED);
    expect(pickupsLine(one, WED)).toBe("tug saved you 1 pickup of your phone this week");
    expect(totalLine(one)).toBe("1 time you didn't have to pick up your phone.");
    const two = record(one, "call", WED);
    expect(pickupsLine(two, WED)).toBe("tug saved you 2 pickups of your phone this week");
    expect(totalLine(two)).toBe("2 times you didn't have to pick up your phone.");
  });

  it("reads back what it saved, and starts over on anything odd", () => {
    const p = record(record(EMPTY, "reply", WED), "music", WED);
    expect(parsePickups(JSON.stringify(p))).toEqual(p);
    expect(parsePickups(undefined)).toEqual(EMPTY);
    expect(parsePickups("{nope")).toEqual(EMPTY);
    expect(parsePickups(JSON.stringify({ total: -4, week: "x", last: { reply: "soon", call: 5 } }))).toEqual({
      total: 0,
      week: 0,
      weekStart: 0,
      last: { call: 5 },
    });
  });
});
