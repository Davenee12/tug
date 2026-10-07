import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { Forecast, Place } from "../lib/weather";

vi.mock("../lib/ipc", () => ({
  api: { getSettings: vi.fn(async () => ({})), setSetting: vi.fn(async () => undefined) },
  errorMessage: (e: unknown) => String(e),
}));

const pending: Array<{ place: Place; resolve: (f: Forecast) => void }> = [];
vi.mock("../lib/weather", async (orig) => ({
  ...(await orig<typeof import("../lib/weather")>()),
  fetchForecast: (place: Place) => new Promise<Forecast>((resolve) => pending.push({ place, resolve })),
  nameFor: async () => null,
}));

const { useWeatherStore } = await import("./weather");

const oldPlace: Place = { name: "Old", latitude: 1, longitude: 1 };
const newPlace: Place = { name: "New", latitude: 2, longitude: 2 };
const forecastFor = (p: Place): Forecast => ({
  fetchedAt: Date.now(),
  now: { time: "2026-10-06T14:00", temp: p.latitude, feels: 0, code: 0, day: true, wind: 0, humidity: 0 },
  hours: [],
  days: [],
});
const flush = () => new Promise((r) => setTimeout(r, 0));

describe("weather store place changes", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    pending.length = 0;
  });

  it("never lands the old place's forecast after the place changes mid-download", async () => {
    const w = useWeatherStore();
    w.place = oldPlace;
    void w.refresh(true);
    expect(pending.map((r) => r.place)).toEqual([oldPlace]);

    const changed = w.setPlace(newPlace);
    await flush();
    // The new place's download starts even though the old one is still out.
    expect(pending.map((r) => r.place)).toEqual([oldPlace, newPlace]);

    pending[0].resolve(forecastFor(oldPlace));
    await flush();
    expect(w.forecast).toBeNull();
    expect(w.loading).toBe(true);

    pending[1].resolve(forecastFor(newPlace));
    await changed;
    expect(w.forecast?.now.temp).toBe(newPlace.latitude);
    expect(w.loading).toBe(false);
  });

  it("a routine refresh still waits for the one in flight", async () => {
    const w = useWeatherStore();
    w.place = oldPlace;
    void w.refresh(true);
    void w.refresh();
    expect(pending).toHaveLength(1);
  });
});
