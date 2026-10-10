import { describe, expect, it } from "vitest";
import { bucketOf, greeting } from "./greeting";
import type { Forecast } from "./weather";

/** 8 October 2026 at a local hour (and minute). */
const at = (h: number, m = 0) => new Date(2026, 9, 8, h, m);

/** A forecast now (°C) with the next hours after it. `later` codes start an hour from now. */
function fc(code: number, c: number, day = true, later: { code: number; rain?: number }[] = []): Forecast {
  return {
    fetchedAt: 0,
    now: { time: "2026-10-08T14:00", temp: c, feels: c, code, day, wind: 5, humidity: 50 },
    hours: [
      { time: "2026-10-08T14:00", temp: c, code, day, rain: 0 },
      ...later.map((h, i) => ({ time: `2026-10-08T${String(15 + i).padStart(2, "0")}:00`, temp: c, code: h.code, day, rain: h.rain ?? 0 })),
    ],
    days: [],
  };
}
const rainy = (n: number) => Array.from({ length: n }, () => ({ code: 63, rain: 90 }));

describe("greeting", () => {
  it.each([
    [4, "night"],
    [5, "morning"],
    [11, "morning"],
    [12, "afternoon"],
    [16, "afternoon"],
    [17, "evening"],
    [21, "evening"],
    [22, "night"],
    [0, "night"],
  ])("hour %i is %s", (h, bucket) => {
    expect(bucketOf(at(h))).toBe(bucket);
  });

  it.each([
    [8, "Good morning"],
    [14, "Good afternoon"],
    [19, "Good evening"],
    [23, "Good night"],
  ])("without weather it's just the hello (%i:00)", (h, text) => {
    expect(greeting(at(h), null, "f")).toBe(text);
  });

  it.each<[string, Date, Forecast, "f" | "c", string]>([
    // Each sky family
    ["hot and sunny", new Date(2026, 9, 9, 9), fc(0, 29), "f", "Good morning ☀️ 84° and sunny. Pool weather."],
    ["hot, another day", at(9), fc(0, 29), "f", "Good morning ☀️ 84° and sunny. A day for something cold."],
    ["clear night", at(23), fc(0, 22, false), "f", "Good night 🌙 72° and clear."],
    ["partly cloudy", at(14), fc(2, 15), "c", "Good afternoon ⛅ 15° and partly cloudy."],
    ["cloudy", at(14), fc(3, 20), "c", "Good afternoon ☁️ 20° and cloudy."],
    ["fog", at(8), fc(45, 12), "c", "Good morning 🌫️ 12° and foggy. Take it slow out there."],
    ["drizzle easing", at(9), fc(51, 14, true, [{ code: 51 }, { code: 2 }]), "c", "Good morning 🌦️ Drizzle until 4 PM. Bring an umbrella."],
    ["rain until evening", at(19), fc(63, 16, false, [...rainy(5), { code: 3 }]), "f", "Good evening 🌧️ Rain until 8 PM. A good night to stay in."],
    ["rain all day", at(14), fc(63, 16, true, rainy(12)), "f", "Good afternoon 🌧️ 61° and rainy. Umbrella weather."],
    ["snow", at(8), fc(73, -3, true, rainy(12).map(() => ({ code: 73 }))), "f", "Good morning 🌨️ 27° and snowy. Bundle up."],
    ["storm", at(14), fc(95, 25), "f", "Good afternoon ⛈️ Storms nearby. A good time to stay in."],
    // Hot and cold
    ["hot evening", at(19), fc(0, 30), "f", "Good evening ☀️ 86° and sunny. A warm one tonight."],
    ["freezing", at(8), fc(0, -5), "f", "Good morning ☀️ 23° and sunny. Bundle up."],
    ["chilly", at(14), fc(3, 8), "c", "Good afternoon ☁️ 8° and cloudy. Grab a jacket."],
    ["mild and clear", at(14), fc(1, 22), "f", "Good afternoon ☀️ 72° and sunny. A good day to get outside."],
    ["light rain easing", at(9), fc(61, 14, true, [{ code: 61 }, { code: 61 }, { code: 0 }]), "c", "Good morning 🌧️ Rain until 5 PM. Bring an umbrella."],
  ])("%s", (_name, date, f, unit, text) => {
    expect(greeting(date, f, unit)).toBe(text);
  });

  it("says noon and midnight in words", () => {
    const f = fc(63, 14, true, [{ code: 0 }]);
    f.hours[1].time = "2026-10-08T12:00";
    expect(greeting(at(9), f, "c")).toContain("Rain until noon.");
    f.hours[1].time = "2026-10-09T00:00";
    expect(greeting(at(9), f, "c")).toContain("Rain until midnight.");
  });

  it("holds steady through the part of the day, no flicker", () => {
    const f = fc(0, 29);
    expect(greeting(at(12, 5), f, "f")).toBe(greeting(at(16, 59), f, "f"));
    expect(greeting(at(12), f, "f")).toBe(greeting(at(12), f, "f"));
  });

  it("never names anyone", () => {
    expect(greeting(at(9), fc(0, 20), "f")).not.toMatch(/,/);
  });
});
