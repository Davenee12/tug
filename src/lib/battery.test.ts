import { describe, expect, it } from "vitest";
import { ALERTED_KEEP_MS, batteryAlert, restoreBatteryAlerted, saveBatteryAlerted } from "./battery";

describe("batteryAlert", () => {
  it("alerts at 20% and again at 10%, once each", () => {
    let alerted: number | null = null;
    const seen: Array<number | null> = [];
    for (const level of [40, 25, 21, 20, 19, 15, 11, 10, 9, 5]) {
      const r = batteryAlert(level, alerted);
      alerted = r.alerted;
      seen.push(r.alert);
    }
    expect(seen).toEqual([null, null, null, 20, null, null, null, 10, null, null]);
  });

  it("says the lowest level reached when it jumps past both", () => {
    expect(batteryAlert(8, null)).toEqual({ alert: 10, alerted: 10 });
  });

  it("re-arms once the phone has clearly charged", () => {
    let r = batteryAlert(18, null);
    expect(r.alert).toBe(20);
    r = batteryAlert(22, r.alerted);
    expect(r.alert).toBeNull(); // wobbling around 20% doesn't repeat it
    r = batteryAlert(30, r.alerted);
    expect(r.alerted).toBeNull();
    expect(batteryAlert(20, r.alerted).alert).toBe(20);
  });

  it("ignores an unknown level", () => {
    expect(batteryAlert(null, 20)).toEqual({ alert: null, alerted: 20 });
  });
});

describe("the saved alert mark", () => {
  const now = Date.parse("2026-10-16T09:00:00Z");

  it("survives a restart, so 20% isn't announced twice in one discharge", () => {
    const saved = saveBatteryAlerted(20, now);
    const restored = restoreBatteryAlerted(saved, now + 60_000);
    expect(restored).toBe(20);
    // Still at 19% after the restart: no second 20% alert, but 10% still comes.
    expect(batteryAlert(19, restored)).toEqual({ alert: null, alerted: 20 });
    expect(batteryAlert(10, restored)).toEqual({ alert: 10, alerted: 10 });
  });

  it("is forgotten once charged, when stale, or when it doesn't make sense", () => {
    expect(restoreBatteryAlerted(saveBatteryAlerted(null, now), now)).toBeNull();
    expect(restoreBatteryAlerted(saveBatteryAlerted(10, now), now + ALERTED_KEEP_MS + 1)).toBeNull();
    expect(restoreBatteryAlerted(undefined, now)).toBeNull();
    expect(restoreBatteryAlerted("not json", now)).toBeNull();
    expect(restoreBatteryAlerted(JSON.stringify({ level: 37, at: now }), now)).toBeNull();
    expect(restoreBatteryAlerted(JSON.stringify({ level: 20, at: now + 3_600_000 }), now)).toBeNull();
  });
});