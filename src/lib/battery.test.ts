import { describe, expect, it } from "vitest";
import { batteryAlert } from "./battery";

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
