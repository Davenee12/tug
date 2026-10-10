// format.ts's date words come from formatters built once (toLocale…String builds one per call,
// which was most of the cost of rendering a long conversation or call list). They must read
// exactly as toLocale…String does.
import { describe, expect, it } from "vitest";
import { clockTime, dayLabel, relativeTime } from "./format";

describe("date words", () => {
  const dates = [new Date(2026, 9, 5, 9, 7), new Date(2026, 9, 5, 21, 45), new Date(2025, 0, 31, 0, 0), new Date(2026, 1, 28, 12, 30)];

  it("clockTime reads as toLocaleTimeString", () => {
    for (const d of dates) expect(clockTime(d)).toBe(d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }));
  });

  it("relativeTime's dates read as toLocaleDateString", () => {
    const now = new Date(2026, 9, 7, 12).getTime();
    for (const d of dates) expect(relativeTime(d, now)).toBe(d.toLocaleDateString([], { month: "short", day: "numeric" }));
    expect(relativeTime(new Date(now - 30_000), now)).toBe("now");
    expect(relativeTime(new Date(now - 5 * 60_000), now)).toBe("5m");
  });

  it("dayLabel's weekdays and dates read as toLocaleDateString", () => {
    const now = new Date(2026, 9, 7, 12);
    const threeDaysAgo = new Date(2026, 9, 4, 8);
    expect(dayLabel(new Date(2026, 9, 7, 1), now)).toBe("Today");
    expect(dayLabel(new Date(2026, 9, 6, 23), now)).toBe("Yesterday");
    expect(dayLabel(threeDaysAgo, now)).toBe(threeDaysAgo.toLocaleDateString([], { weekday: "long" }));
    for (const d of dates.slice(2)) {
      expect(dayLabel(d, now)).toBe(d.toLocaleDateString([], { month: "long", day: "numeric", year: "numeric" }));
    }
  });

  it("an invalid date reads as it always did", () => {
    expect(clockTime(new Date(NaN))).toBe("Invalid Date");
  });
});
