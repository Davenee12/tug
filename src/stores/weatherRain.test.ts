import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { Forecast } from "../lib/weather";

const saved: Record<string, string> = {};
vi.mock("../lib/ipc", () => ({
  api: {
    getSettings: vi.fn(async () => ({ ...saved })),
    setSetting: vi.fn(async (k: string, v: string) => void (saved[k] = v)),
  },
  errorMessage: (e: unknown) => String(e),
}));
const popped: Array<[number, string, string]> = [];
vi.mock("./tug", () => ({
  RAIN_TOAST_ID: 7,
  useTugStore: () => ({
    status: { connection: "connected", battery: 22, services: { battery: true } },
    popInfo: async (id: number, title: string, body: string) => void popped.push([id, title, body]),
  }),
}));

const { useWeatherStore } = await import("./weather");

/** Dry now (17:15 UTC), rain from 17:30. */
const forecast = (): Forecast => ({
  fetchedAt: Date.now(),
  timezone: "UTC",
  now: { time: "2026-10-05T17:15", temp: 12, feels: 10, code: 3, day: true, wind: 10, humidity: 80 },
  hours: [],
  days: [],
  quarters: ["17:15", "17:30", "17:45", "18:00", "18:15", "18:30", "18:45", "19:00"].map((t, i) => ({
    time: `2026-10-05T${t}`,
    precip: i === 1 || i === 2 ? 0.5 : 0,
  })),
});

describe("rain soon pop-ups", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    popped.length = 0;
    for (const k of Object.keys(saved)) delete saved[k];
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(Date.UTC(2026, 9, 5, 17, 20));
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("pops up once per spell, with the battery, only when turned on", () => {
    const w = useWeatherStore();
    w.place = { name: "Portland, Oregon", latitude: 45.5, longitude: -122.7 };
    w.forecast = forecast();
    w.checkRain();
    expect(popped).toEqual([]);

    w.setRainAlerts(true);
    expect(saved["ui.rainAlert"]).toBe("on");
    expect(popped).toHaveLength(1);
    expect(popped[0][0]).toBe(7);
    expect(popped[0][1]).toBe("Rain soon in Portland");
    expect(popped[0][2]).toMatch(/Your phone's at 22%/);
    expect(saved["ui.rainAlertLast"]).toBe("2026-10-05T17:30");

    // The next refresh tick: the same spell, told already.
    w.checkRain();
    expect(popped).toHaveLength(1);
  });

  it("remembers the spell it told about across a restart", async () => {
    // init starts the refresh clock: just enough of a browser for it.
    const noop = () => undefined;
    vi.stubGlobal("window", { setInterval: () => 1, clearInterval: noop, addEventListener: noop, removeEventListener: noop });
    vi.stubGlobal("document", { addEventListener: noop, removeEventListener: noop, visibilityState: "visible" });
    saved["ui.rainAlert"] = "on";
    saved["ui.rainAlertLast"] = "2026-10-05T17:30";
    const w = useWeatherStore();
    await w.init();
    expect(w.rainAlerts).toBe(true);
    w.place = { name: "Portland, Oregon", latitude: 45.5, longitude: -122.7 };
    w.forecast = forecast();
    w.checkRain();
    expect(popped).toEqual([]);
    w.dispose();
  });
});
