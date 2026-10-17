import { describe, expect, it } from "vitest";
import {
  CALM_SEAS,
  EMPTY_DAILY,
  FRESH_MS,
  KEEP_DAYS,
  conditionsFrom,
  conditionsLine,
  currentStreak,
  dailySeed,
  dayBefore,
  finishDaily,
  handlingFor,
  localDate,
  parseDaily,
  sailedToday,
  type Conditions,
  type DailyRecord,
} from "./tugboatDaily";
import { CALM, FOLLOW_TAU, RESPONSE, newRun, spawnRow } from "./tugboatRun";
import type { Forecast } from "./weather";

const NOW = Date.UTC(2026, 9, 16, 15, 0);

/** A forecast fetched a minute ago with these current conditions. */
function forecast(now: Partial<Forecast["now"]>, fetchedAt = NOW - 60_000): Forecast {
  return {
    fetchedAt,
    now: { time: "2026-10-16T15:00", temp: 12, feels: 11, code: 0, day: true, wind: 0, humidity: 70, ...now },
    hours: [],
    days: [],
  };
}

const live = (c: Partial<Conditions>): Conditions => ({ ...CALM_SEAS, live: true, sky: "clear", ...c });

describe("today's course", () => {
  it("is named by the date on this PC's clock", () => {
    expect(localDate(new Date(2026, 0, 5, 23, 59))).toBe("2026-01-05");
    expect(localDate(new Date(2026, 11, 31, 0, 0))).toBe("2026-12-31");
  });

  it("knows the day before, across months, years and leap days", () => {
    expect(dayBefore("2026-10-16")).toBe("2026-10-15");
    expect(dayBefore("2026-03-01")).toBe("2026-02-28");
    expect(dayBefore("2024-03-01")).toBe("2024-02-29");
    expect(dayBefore("2026-01-01")).toBe("2025-12-31");
  });

  it("the same date is the same course; different dates are different courses", () => {
    const rows = (date: string) => {
      const run = newRun(dailySeed(date));
      return Array.from({ length: 15 }, () => spawnRow(run).map((th) => `${th.kind}@${th.x.toFixed(3)}`)).join("|");
    };
    expect(dailySeed("2026-10-16")).toBe(dailySeed("2026-10-16"));
    expect(rows("2026-10-16")).toBe(rows("2026-10-16"));
    expect(rows("2026-10-16")).not.toBe(rows("2026-10-17"));
    // A year of days, every seed its own.
    const seeds = new Set<number>();
    let d = "2027-01-01";
    for (let i = 0; i < 365; i++) {
      d = dayBefore(d);
      seeds.add(dailySeed(d));
    }
    expect(seeds.size).toBe(365);
  });
});

describe("weather → today's water", () => {
  it("no weather, or none fresh, is calm seas", () => {
    expect(conditionsFrom(null, NOW)).toEqual(CALM_SEAS);
    expect(conditionsFrom(undefined, NOW)).toEqual(CALM_SEAS);
    expect(conditionsFrom(forecast({ code: 63 }, NOW - FRESH_MS - 1), NOW)).toEqual(CALM_SEAS);
    // A forecast from the future (the clock moved back) isn't trusted either.
    expect(conditionsFrom(forecast({ code: 63 }, NOW + 3_600_000), NOW)).toEqual(CALM_SEAS);
    expect(handlingFor(CALM_SEAS)).toEqual(CALM);
    expect(conditionsLine(CALM_SEAS, "f")).toBe("Calm seas today");
  });

  it("reads rain, snow, night and wind from the current conditions", () => {
    expect(conditionsFrom(forecast({ code: 51 }), NOW).rain).toBe(1);
    expect(conditionsFrom(forecast({ code: 63 }), NOW).rain).toBe(2);
    expect(conditionsFrom(forecast({ code: 80 }), NOW).rain).toBe(2);
    expect(conditionsFrom(forecast({ code: 65 }), NOW).rain).toBe(3);
    expect(conditionsFrom(forecast({ code: 95 }), NOW).rain).toBe(3);
    const snow = conditionsFrom(forecast({ code: 73 }), NOW);
    expect(snow.snow && snow.rain === 0).toBe(true);
    const night = conditionsFrom(forecast({ code: 0, day: false, wind: 20, windDir: 270 }), NOW);
    expect(night).toEqual({ live: true, sky: "clear", windKmh: 20, windFrom: 270, night: true, rain: 0, snow: false });
    // An older cached forecast without a wind direction still has its speed.
    expect(conditionsFrom(forecast({ wind: 12 }), NOW).windFrom).toBeNull();
  });

  it("rain makes waves, snow makes it slow and icy, wind pushes across", () => {
    expect(handlingFor(live({ sky: "rain", rain: 2 })).waves).toBeGreaterThan(handlingFor(live({ sky: "drizzle", rain: 1 })).waves);
    expect(handlingFor(live({ sky: "storm", rain: 3 })).waves).toBeGreaterThan(handlingFor(live({ sky: "rain", rain: 2 })).waves);
    const icy = handlingFor(live({ sky: "snow", snow: true }));
    expect(icy.speedScale).toBeLessThan(1);
    expect(icy.response).toBeLessThan(RESPONSE);
    expect(icy.followTau).toBeGreaterThan(FOLLOW_TAU);
    // Wind from the west blows the boat right (east), from the east left; along the channel barely.
    expect(handlingFor(live({ windKmh: 40, windFrom: 270 })).drift).toBeGreaterThan(5);
    expect(handlingFor(live({ windKmh: 40, windFrom: 90 })).drift).toBeLessThan(-5);
    expect(Math.abs(handlingFor(live({ windKmh: 40, windFrom: 0 })).drift)).toBeLessThan(0.01);
    // A breeze doesn't move a tugboat; a gale is capped.
    expect(handlingFor(live({ windKmh: 6, windFrom: 270 })).drift).toBe(0);
    expect(handlingFor(live({ windKmh: 200, windFrom: 270 })).drift).toBeLessThanOrEqual(9);
    // Clear and still: the usual water.
    expect(handlingFor(live({}))).toEqual(CALM);
  });

  it("says what today is like on the start screen, in the weather card's units", () => {
    expect(conditionsLine(live({ sky: "rain", rain: 2, windKmh: 19, windFrom: 0 }), "f")).toBe("Today: rainy, wind 12 mph — expect waves");
    expect(conditionsLine(live({ sky: "rain", rain: 2, windKmh: 19, windFrom: 0 }), "c")).toBe("Today: rainy, wind 19 km/h — expect waves");
    expect(conditionsLine(live({ sky: "rain", rain: 3, windKmh: 40, windFrom: 260 }), "f")).toBe(
      "Today: pouring, wind 25 mph — expect waves and a crosswind",
    );
    expect(conditionsLine(live({ sky: "snow", snow: true, windKmh: 5 }), "f")).toBe("Today: snowy, wind 3 mph — expect icy, slower water");
    expect(conditionsLine(live({ night: true, windKmh: 0 }), "f")).toBe("Tonight: clear, no wind — expect a lighthouse to steer by");
    expect(conditionsLine(live({ sky: "partly", windKmh: 6 }), "c")).toBe("Today: partly cloudy, wind 6 km/h — smooth sailing");
  });
});

describe("the streak", () => {
  const play = (rec: DailyRecord, ...days: Array<[string, number]>) => days.reduce((r, [d, s]) => finishDaily(r, d, s).record, rec);

  it("counts days in a row with a finished daily run", () => {
    let r = play(EMPTY_DAILY, ["2026-10-14", 300]);
    expect(r).toEqual({ streak: 1, last: "2026-10-14", bests: { "2026-10-14": 300 } });
    r = play(r, ["2026-10-15", 200], ["2026-10-16", 400]);
    expect(r.streak).toBe(3);
    expect(currentStreak(r, "2026-10-16")).toBe(3);
  });

  it("another run on the same day keeps the streak and only raises the best", () => {
    const r = play(EMPTY_DAILY, ["2026-10-16", 300]);
    const lower = finishDaily(r, "2026-10-16", 250);
    expect(lower.newBest).toBe(false);
    expect(lower.record).toEqual(r);
    const higher = finishDaily(r, "2026-10-16", 420);
    expect(higher.newBest).toBe(true);
    expect(higher.record).toEqual({ streak: 1, last: "2026-10-16", bests: { "2026-10-16": 420 } });
    expect(sailedToday(higher.record, "2026-10-16")).toBe(true);
    expect(sailedToday(higher.record, "2026-10-17")).toBe(false);
  });

  it("is still alive the next day (today's run can still be sailed), and broken after a missed day", () => {
    const r = play(EMPTY_DAILY, ["2026-10-15", 1], ["2026-10-16", 1]);
    expect(currentStreak(r, "2026-10-17")).toBe(2);
    expect(currentStreak(r, "2026-10-18")).toBe(0);
    expect(currentStreak(EMPTY_DAILY, "2026-10-18")).toBe(0);
    // Skipping a day starts again at 1.
    expect(play(r, ["2026-10-18", 1]).streak).toBe(1);
  });

  it("across midnight: a run counts for the day its course was", () => {
    // Started at 23:58 on the 16th, sunk at 00:01 on the 17th: it's the 16th's run.
    const r = play(EMPTY_DAILY, ["2026-10-15", 1], ["2026-10-16", 1]);
    expect(r.streak).toBe(2);
    expect(currentStreak(r, "2026-10-17")).toBe(2);
    // And the 17th's own run then carries it on.
    expect(play(r, ["2026-10-17", 1]).streak).toBe(3);
  });

  it("across month ends, and with the clock moved back", () => {
    expect(play(EMPTY_DAILY, ["2026-02-28", 1], ["2026-03-01", 1]).streak).toBe(2);
    const r = play(EMPTY_DAILY, ["2026-10-15", 1], ["2026-10-16", 1]);
    // A run dated before the last one (the PC's clock went back) records its best, nothing more.
    const back = finishDaily(r, "2026-10-10", 900).record;
    expect(back.streak).toBe(2);
    expect(back.last).toBe("2026-10-16");
    expect(back.bests["2026-10-10"]).toBe(900);
  });

  it("keeps only recent days' bests", () => {
    let r = EMPTY_DAILY;
    let d = "2026-12-31";
    for (let i = 0; i < KEEP_DAYS + 20; i++) d = dayBefore(d);
    for (let i = 0; i < KEEP_DAYS + 20; i++) {
      r = finishDaily(r, d, i).record;
      const [y, m, dd] = d.split("-").map(Number);
      d = new Date(Date.UTC(y, m - 1, dd + 1)).toISOString().slice(0, 10);
    }
    expect(Object.keys(r.bests)).toHaveLength(KEEP_DAYS);
    expect(r.streak).toBe(KEEP_DAYS + 20);
  });

  it("reads back what it saved, and shrugs off anything damaged", () => {
    const r = play(EMPTY_DAILY, ["2026-10-15", 120], ["2026-10-16", 340]);
    expect(parseDaily(JSON.stringify(r))).toEqual(r);
    expect(parseDaily(undefined)).toEqual(EMPTY_DAILY);
    expect(parseDaily("not json")).toEqual(EMPTY_DAILY);
    expect(parseDaily("[1,2]").streak).toBe(0);
    expect(parseDaily(JSON.stringify({ streak: -4, last: "yesterday", bests: { x: 1, "2026-10-16": -5 } }))).toEqual(EMPTY_DAILY);
    // A streak without a valid number still counts the last day it has.
    expect(parseDaily(JSON.stringify({ streak: "lots", last: "2026-10-16", bests: {} })).streak).toBe(1);
  });
});
