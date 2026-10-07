// Weather for the Feed's glance card: Open-Meteo (free, no key). Only the chosen place's
// coordinates are sent, rounded to ~1 km. Fetched in °C / km/h and converted for display.

import { api } from "./ipc";

export interface Place {
  name: string;
  latitude: number;
  longitude: number;
}

export interface Hour {
  time: string; // place-local ISO, e.g. 2026-10-05T14:00
  temp: number;
  code: number;
  day: boolean;
  rain: number | null; // precipitation probability, %
}

export interface Day {
  date: string; // 2026-10-05
  code: number;
  high: number;
  low: number;
  rain: number | null;
}

export interface Forecast {
  fetchedAt: number;
  /** IANA zone of the place, e.g. America/New_York (absent in caches from before it was kept). */
  timezone?: string;
  now: { time: string; temp: number; feels: number; code: number; day: boolean; wind: number; humidity: number };
  hours: Hour[]; // from the current hour, next 24
  days: Day[]; // today first, 7 days
}

export type Unit = "f" | "c";

/**
 * How often the card asks for a fresh forecast. Open-Meteo updates current conditions every
 * 15 minutes.
 */
export const REFRESH_MS = 15 * 60 * 1000;

/**
 * Whether a forecast is due a refresh. A minute early on purpose: the timer starts before the
 * download finishes (and stamps `fetchedAt`), so a tick exactly one period later would otherwise
 * find it a moment too fresh, skip, and leave the numbers an extra period old.
 */
export function isStale(fetchedAt: number, nowMs: number): boolean {
  return nowMs - fetchedAt >= REFRESH_MS - 60_000;
}

/** The place's current hour as "YYYY-MM-DDTHH", the form Open-Meteo's local times start with. */
export function placeHour(nowMs: number, timeZone?: string): string {
  const parts = new Intl.DateTimeFormat("en-CA", {
    timeZone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    hourCycle: "h23",
  }).formatToParts(new Date(nowMs));
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? "00";
  return `${get("year")}-${get("month")}-${get("day")}T${get("hour")}`;
}

/**
 * Between downloads, "now" follows the clock through the hourly forecast: once the place's hour
 * moves past the one the forecast was fetched in, temperature, sky and day/night come from that
 * hour and past hours drop off the strip. Feels-like, wind and humidity stay from the last
 * download (the hourly series doesn't carry them).
 */
export function withCurrentHour(f: Forecast, nowMs: number): Forecast {
  const key = placeHour(nowMs, f.timezone);
  const withToday = currentDays(f, key.slice(0, 10));
  if (f.now.time.slice(0, 13) >= key) return withToday;
  const i = withToday.hours.findIndex((h) => h.time.slice(0, 13) === key);
  if (i < 0) return withToday;
  const h = withToday.hours[i];
  return { ...withToday, now: { ...f.now, time: h.time, temp: h.temp, code: h.code, day: h.day }, hours: withToday.hours.slice(i) };
}

/**
 * Past midnight at the place, yesterday drops off the daily list so the first day really is
 * "Today" (offline, the last download can be from the day before). Kept as-is if every day has
 * passed: an old outlook beats an empty one.
 */
function currentDays(f: Forecast, today: string): Forecast {
  const i = f.days.findIndex((d) => d.date >= today);
  return i <= 0 ? f : { ...f, days: f.days.slice(i) };
}

/** °F where people use it (US and a few others), °C everywhere else. */
export function defaultUnit(locale = navigator.language): Unit {
  const region = locale.split("-")[1]?.toUpperCase();
  return region && ["US", "LR", "MM", "BS", "BZ", "KY", "PW"].includes(region) ? "f" : "c";
}

export const temp = (c: number, unit: Unit) => Math.round(unit === "f" ? (c * 9) / 5 + 32 : c);
export const wind = (kmh: number, unit: Unit) => (unit === "f" ? `${Math.round(kmh / 1.609)} mph` : `${Math.round(kmh)} km/h`);

export type Sky = "clear" | "partly" | "cloudy" | "fog" | "drizzle" | "rain" | "snow" | "storm";

/** WMO weather code → label and sky family. */
export function describe(code: number): { label: string; sky: Sky } {
  if (code === 0) return { label: "Clear", sky: "clear" };
  if (code === 1) return { label: "Mostly clear", sky: "clear" };
  if (code === 2) return { label: "Partly cloudy", sky: "partly" };
  if (code === 3) return { label: "Cloudy", sky: "cloudy" };
  if (code === 45 || code === 48) return { label: "Fog", sky: "fog" };
  if (code >= 51 && code <= 57) return { label: code >= 56 ? "Freezing drizzle" : "Drizzle", sky: "drizzle" };
  if (code >= 61 && code <= 67) {
    const label = code >= 66 ? "Freezing rain" : code === 61 ? "Light rain" : code === 65 ? "Heavy rain" : "Rain";
    return { label, sky: "rain" };
  }
  if ((code >= 71 && code <= 77) || code === 85 || code === 86) return { label: code === 75 ? "Heavy snow" : "Snow", sky: "snow" };
  if (code >= 80 && code <= 82) return { label: code === 82 ? "Heavy showers" : "Showers", sky: "rain" };
  if (code >= 95) return { label: code === 95 ? "Thunderstorms" : "Storms with hail", sky: "storm" };
  return { label: "—", sky: "cloudy" };
}

const wet = (code: number) => ["drizzle", "rain", "snow", "storm"].includes(describe(code).sky);

/** "7 AM", "Noon" from a place-local ISO time. */
export function hourLabel(iso: string): string {
  const h = Number(iso.slice(11, 13));
  if (h === 0) return "12 AM";
  if (h === 12) return "Noon";
  return h < 12 ? `${h} AM` : `${h - 12} PM`;
}

/**
 * The one line worth saying right now, if any: rain or snow coming in the next 12 hours,
 * or easing off if it's falling now.
 */
export function outlook(f: Forecast): string | null {
  const next = f.hours.slice(1, 13);
  const likely = (h: Hour) => (h.rain ?? 0) >= 50 || wet(h.code);
  if (wet(f.now.code)) {
    const dry = next.find((h) => !likely(h));
    return dry ? `Should ease off around ${hourLabel(dry.time)}` : null;
  }
  const first = next.find(likely);
  if (!first) return null;
  const what = describe(first.code).sky === "snow" ? "Snow" : "Rain";
  return `${what} likely around ${hourLabel(first.time)}`;
}

const round = (n: number) => Math.round(n * 100) / 100;

/** A hung request must not leave the card stuck on "loading" (and Retry doing nothing). */
const TIMEOUT_MS = 15_000;

export async function fetchForecast(p: Place): Promise<Forecast> {
  const q = new URLSearchParams({
    latitude: String(round(p.latitude)),
    longitude: String(round(p.longitude)),
    current: "temperature_2m,apparent_temperature,weather_code,is_day,wind_speed_10m,relative_humidity_2m",
    hourly: "temperature_2m,weather_code,is_day,precipitation_probability",
    daily: "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max",
    timezone: "auto",
    forecast_days: "7",
  });
  const res = await fetch(`https://api.open-meteo.com/v1/forecast?${q}`, { signal: AbortSignal.timeout(TIMEOUT_MS) });
  if (!res.ok) throw new Error(`Forecast unavailable (${res.status})`);
  return parseForecast(await res.json(), Date.now());
}

export function parseForecast(j: any, fetchedAt: number): Forecast {
  const c = j.current;
  const h = j.hourly;
  const d = j.daily;
  const start = Math.max(0, (h.time as string[]).findIndex((t) => t.slice(0, 13) === String(c.time).slice(0, 13)));
  const hours: Hour[] = (h.time as string[]).slice(start, start + 24).map((time, i) => ({
    time,
    temp: h.temperature_2m[start + i],
    code: h.weather_code[start + i],
    day: h.is_day[start + i] === 1,
    rain: h.precipitation_probability?.[start + i] ?? null,
  }));
  const days: Day[] = (d.time as string[]).map((date, i) => ({
    date,
    code: d.weather_code[i],
    high: d.temperature_2m_max[i],
    low: d.temperature_2m_min[i],
    rain: d.precipitation_probability_max?.[i] ?? null,
  }));
  return {
    fetchedAt,
    timezone: typeof j.timezone === "string" ? j.timezone : undefined,
    now: {
      time: c.time,
      temp: c.temperature_2m,
      feels: c.apparent_temperature,
      code: c.weather_code,
      day: c.is_day === 1,
      wind: c.wind_speed_10m,
      humidity: c.relative_humidity_2m,
    },
    hours,
    days,
  };
}

export async function searchPlaces(query: string): Promise<Place[]> {
  const q = new URLSearchParams({ name: query.trim(), count: "5", language: "en", format: "json" });
  const res = await fetch(`https://geocoding-api.open-meteo.com/v1/search?${q}`, { signal: AbortSignal.timeout(TIMEOUT_MS) });
  if (!res.ok) throw new Error(`City search unavailable (${res.status})`);
  const j = await res.json();
  return ((j.results ?? []) as any[]).map((r) => ({
    name: [r.name, r.admin1 && r.admin1 !== r.name ? r.admin1 : null, r.country_code === "US" ? null : r.country]
      .filter(Boolean)
      .join(", "),
    latitude: r.latitude,
    longitude: r.longitude,
  }));
}

/**
 * A name for coordinates from "Use my location" ("Portland, Oregon"). BigDataCloud's
 * client-side reverse lookup (free, no key, meant for a device looking up itself), fetched
 * by the app's native side: from the web view the request never got an answer.
 */
export async function nameFor(latitude: number, longitude: number): Promise<string | null> {
  return placeName(JSON.parse(await api.placeLookup(latitude, longitude)));
}

export function placeName(j: { city?: string; locality?: string; principalSubdivision?: string; countryCode?: string; countryName?: string }): string | null {
  const town = j.city || j.locality;
  if (!town) return null;
  const region = j.principalSubdivision && j.principalSubdivision !== town ? j.principalSubdivision : null;
  const country = j.countryCode && j.countryCode !== "US" ? j.countryName : null;
  return [town, region, country].filter(Boolean).join(", ");
}

/** The time at the place, "9:58 PM", in its own zone when known. */
export function localTime(now: Date, timezone?: string): string {
  const opts: Intl.DateTimeFormatOptions = { hour: "numeric", minute: "2-digit" };
  try {
    return now.toLocaleTimeString(undefined, timezone ? { ...opts, timeZone: timezone } : opts);
  } catch {
    return now.toLocaleTimeString(undefined, opts); // unknown zone name
  }
}

/** "Today", "Tue" for a place-local date. */
export function dayLabel(date: string, today: string): string {
  if (date === today) return "Today";
  const [y, m, d] = date.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString(undefined, { weekday: "short" });
}
