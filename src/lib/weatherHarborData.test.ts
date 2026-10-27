import { afterEach, describe, expect, it, vi } from "vitest";
import { fetchForecast, parseForecast, quarterOf } from "./weather";

const base = {
  current: {
    time: "2026-10-05T14:30",
    temperature_2m: 12,
    apparent_temperature: 10,
    weather_code: 61,
    is_day: 1,
    wind_speed_10m: 24,
    wind_direction_10m: 225,
    relative_humidity_2m: 80,
  },
  hourly: { time: ["2026-10-05T14:00"], temperature_2m: [12], weather_code: [61], is_day: [1], precipitation_probability: [70] },
  daily: { time: ["2026-10-05"], weather_code: [61], temperature_2m_max: [14], temperature_2m_min: [8], precipitation_probability_max: [80] },
};

describe("the harbor's extra weather", () => {
  it("reads clouds, gusts, rain now, sunrise, sunset and the next two hours", () => {
    const f = parseForecast(
      {
        ...base,
        current: { ...base.current, cloud_cover: 90, wind_gusts_10m: 41.5, precipitation: 0.6 },
        daily: { ...base.daily, sunrise: ["2026-10-05T07:12"], sunset: ["2026-10-05T18:40"] },
        // Each is the rain in the quarter *before* its time: 14:45's 0.2 mm fell from 14:30.
        minutely_15: {
          time: ["2026-10-05T14:15", "2026-10-05T14:30", "2026-10-05T14:45", "2026-10-05T15:00", "2026-10-05T15:15"],
          precipitation: [0.4, 0.3, 0.2, 0, null],
        },
      },
      1,
    );
    expect(f.now).toMatchObject({ clouds: 90, gusts: 41.5, precip: 0.6 });
    expect(f.days[0]).toMatchObject({ sunrise: "2026-10-05T07:12", sunset: "2026-10-05T18:40" });
    // Kept by when each quarter starts, from the current one on; a missing value is left out.
    expect(f.quarters).toEqual([
      { time: "2026-10-05T14:30", precip: 0.2 },
      { time: "2026-10-05T14:45", precip: 0 },
    ]);
  });

  it("is happy without any of it (an older answer, or the base request)", () => {
    const f = parseForecast(base, 1);
    expect(f.now.clouds).toBeUndefined();
    expect(f.days[0].sunrise).toBeUndefined();
    expect(f.quarters).toBeUndefined();
  });

  it("finds the quarter-hour a time falls in", () => {
    expect(quarterOf("2026-10-05T14:37")).toBe("2026-10-05T14:30");
    expect(quarterOf("2026-10-05T14:00")).toBe("2026-10-05T14:00");
    expect(quarterOf("2026-10-05T14:59")).toBe("2026-10-05T14:45");
  });
});

describe("fetching", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("asks again for exactly the old fields if the service refuses the new ones", async () => {
    const asked: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string) => {
        asked.push(url);
        if (url.includes("minutely_15")) return new Response("{}", { status: 400 });
        return new Response(JSON.stringify(base), { status: 200 });
      }),
    );
    const f = await fetchForecast({ name: "Here", latitude: 51.5, longitude: -0.13 });
    expect(asked).toHaveLength(2);
    expect(asked[1]).not.toContain("cloud_cover");
    expect(f.now.temp).toBe(12);
  });

  it("asks once when the service takes them", async () => {
    const fetcher = vi.fn(async () => new Response(JSON.stringify(base), { status: 200 }));
    vi.stubGlobal("fetch", fetcher);
    await fetchForecast({ name: "Here", latitude: 51.5, longitude: -0.13 });
    expect(fetcher).toHaveBeenCalledTimes(1);
  });

  it("still fails plainly when the service is down", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("", { status: 503 })));
    await expect(fetchForecast({ name: "Here", latitude: 51.5, longitude: -0.13 })).rejects.toThrow("Forecast unavailable (503)");
  });
});
