import { describe as group, expect, it } from "vitest";
import {
  dayLabel,
  defaultUnit,
  describe,
  hourLabel,
  localTime,
  outlook,
  parseForecast,
  placeName,
  temp,
  wind,
  type Forecast,
} from "./weather";

function hours(codes: number[], rain: number[] = []) {
  return codes.map((code, i) => ({
    time: `2026-10-05T${String(13 + i).padStart(2, "0")}:00`,
    temp: 20,
    code,
    day: true,
    rain: rain[i] ?? 0,
  }));
}

function forecast(nowCode: number, codes: number[], rain: number[] = []): Forecast {
  return {
    fetchedAt: 0,
    now: { time: "2026-10-05T13:10", temp: 20, feels: 20, code: nowCode, day: true, wind: 10, humidity: 50 },
    hours: hours(codes, rain),
    days: [],
  };
}

group("weather", () => {
  it("picks °F for the US and °C elsewhere", () => {
    expect(defaultUnit("en-US")).toBe("f");
    expect(defaultUnit("en-GB")).toBe("c");
    expect(defaultUnit("es")).toBe("c");
  });

  it("converts units for display", () => {
    expect(temp(0, "f")).toBe(32);
    expect(temp(21.6, "c")).toBe(22);
    expect(temp(-40, "f")).toBe(-40);
    expect(wind(16.09, "f")).toBe("10 mph");
    expect(wind(16.09, "c")).toBe("16 km/h");
  });

  it("names WMO codes", () => {
    expect(describe(0)).toEqual({ label: "Clear", sky: "clear" });
    expect(describe(2).sky).toBe("partly");
    expect(describe(65)).toEqual({ label: "Heavy rain", sky: "rain" });
    expect(describe(81).sky).toBe("rain");
    expect(describe(75).label).toBe("Heavy snow");
    expect(describe(96).sky).toBe("storm");
  });

  it("labels hours like a person would", () => {
    expect(hourLabel("2026-10-05T00:00")).toBe("12 AM");
    expect(hourLabel("2026-10-05T07:00")).toBe("7 AM");
    expect(hourLabel("2026-10-05T12:00")).toBe("Noon");
    expect(hourLabel("2026-10-05T16:00")).toBe("4 PM");
  });

  it("says the one thing worth knowing", () => {
    // Dry now, rain from 4 PM (index 3 → 16:00).
    expect(outlook(forecast(1, [1, 1, 2, 61, 63]))).toBe("Rain likely around 4 PM");
    // A high chance counts even if the code isn't wet yet.
    expect(outlook(forecast(1, [1, 2, 3], [0, 10, 70]))).toBe("Rain likely around 3 PM");
    expect(outlook(forecast(0, [1, 1, 71]))).toBe("Snow likely around 3 PM");
    // Raining now: when it stops.
    expect(outlook(forecast(63, [63, 61, 3, 2]))).toBe("Should ease off around 3 PM");
    // Nothing to say on a dry day.
    expect(outlook(forecast(0, [0, 1, 2, 3]))).toBeNull();
  });

  it("parses the forecast starting from the current hour", () => {
    const f = parseForecast(
      {
        current: {
          time: "2026-10-05T14:15",
          temperature_2m: 22,
          apparent_temperature: 23,
          weather_code: 2,
          is_day: 1,
          wind_speed_10m: 12,
          relative_humidity_2m: 40,
        },
        hourly: {
          time: ["2026-10-05T13:00", "2026-10-05T14:00", "2026-10-05T15:00"],
          temperature_2m: [21, 22, 23],
          weather_code: [1, 2, 3],
          is_day: [1, 1, 0],
          precipitation_probability: [0, 5, 60],
        },
        daily: {
          time: ["2026-10-05", "2026-10-06"],
          weather_code: [2, 61],
          temperature_2m_max: [25, 19],
          temperature_2m_min: [14, 12],
          precipitation_probability_max: [5, 80],
        },
      },
      123,
    );
    expect(f.fetchedAt).toBe(123);
    expect(f.now).toMatchObject({ temp: 22, code: 2, day: true });
    expect(f.hours.map((h) => h.time)).toEqual(["2026-10-05T14:00", "2026-10-05T15:00"]);
    expect(f.hours[1]).toMatchObject({ temp: 23, day: false, rain: 60 });
    expect(f.days[1]).toEqual({ date: "2026-10-06", code: 61, high: 19, low: 12, rain: 80 });
  });

  it("names a located place like a person would", () => {
    expect(placeName({ city: "Orlando", locality: "Meadow Woods", principalSubdivision: "Florida", countryCode: "US" })).toBe(
      "Orlando, Florida",
    );
    expect(placeName({ city: "", locality: "Kissimmee", principalSubdivision: "Florida", countryCode: "US" })).toBe(
      "Kissimmee, Florida",
    );
    expect(placeName({ city: "Madrid", principalSubdivision: "Madrid", countryCode: "ES", countryName: "Spain" })).toBe(
      "Madrid, Spain",
    );
    expect(placeName({ countryCode: "US" })).toBeNull();
  });

  it("shows the time in the place's own zone", () => {
    const at = new Date("2026-10-05T01:58:00Z");
    const fmt = (tz: string) => at.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit", timeZone: tz });
    expect(localTime(at, "America/New_York")).toBe(fmt("America/New_York"));
    expect(localTime(at, "Asia/Tokyo")).toBe(fmt("Asia/Tokyo"));
    expect(localTime(at, "Not/AZone")).toBe(at.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" }));
  });

  it("keeps the place's time zone from the forecast", () => {
    const f = parseForecast(
      {
        timezone: "America/New_York",
        current: { time: "2026-10-05T21:00", temperature_2m: 1, apparent_temperature: 1, weather_code: 0, is_day: 0, wind_speed_10m: 0, relative_humidity_2m: 0 },
        hourly: { time: [], temperature_2m: [], weather_code: [], is_day: [] },
        daily: { time: [], weather_code: [], temperature_2m_max: [], temperature_2m_min: [] },
      },
      0,
    );
    expect(f.timezone).toBe("America/New_York");
  });

  it("labels days", () => {
    expect(dayLabel("2026-10-05", "2026-10-05")).toBe("Today");
    expect(dayLabel("2026-10-06", "2026-10-05")).toBe(new Date(2026, 9, 6).toLocaleDateString(undefined, { weekday: "short" }));
  });
});
