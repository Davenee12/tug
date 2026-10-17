// Tugboat Run's daily challenge, the pure parts: today's date and the course seed it gives (the
// same course for everyone that day), what the player's real weather does to the water, the line
// the start screen says about it, and the streak of days with a finished daily run. Harbor Stack's
// daily stack uses the same date, seed (with its own name in it), weather and streak rules.
//
// Weather is only ever what tug's weather card already has: no forecast (weather off, not set up,
// or nothing fresh) means calm seas, said as such, never made up.

import { CALM, FOLLOW_TAU, RESPONSE, type Handling } from "./tugboatRun";
import { describe as describeSky, wind as windText, withCurrentHour, type Forecast, type Sky, type Unit } from "./weather";

// --- Today's course ---

/** "2026-10-16": the date on this PC's clock, in its own time zone. */
export function localDate(now: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

/** The calendar day before `date` ("2026-03-01" → "2026-02-28"), untouched by clock changes. */
export function dayBefore(date: string): string {
  const [y, m, d] = date.split("-").map(Number);
  const t = new Date(Date.UTC(y, m - 1, d - 1));
  return t.toISOString().slice(0, 10);
}

/**
 * The course seed for a day: a 32-bit hash (FNV-1a) of the game and the date, the same on every PC.
 * Each game has its own daily (Harbor Stack's day isn't Tugboat Run's course reused).
 */
export function dailySeed(date: string, game: "tugboat-run" | "harbor-stack" = "tugboat-run"): number {
  let h = 0x811c9dc5;
  for (const ch of `${game}/${date}`) {
    h ^= ch.charCodeAt(0);
    h = Math.imul(h, 0x01000193);
  }
  return h | 0;
}

// --- Today's water, from the weather ---

export interface Conditions {
  /** From the player's real weather. False means calm seas: weather off, not set up, or stale. */
  live: boolean;
  sky: Sky | null;
  /** Wind speed, km/h (0 when calm). */
  windKmh: number;
  /** Where the wind blows from, compass degrees, when the forecast says. */
  windFrom: number | null;
  night: boolean;
  /** 0 dry, 1 drizzle, 2 rain or showers, 3 heavy rain or storms. */
  rain: 0 | 1 | 2 | 3;
  snow: boolean;
}

export const CALM_SEAS: Conditions = { live: false, sky: null, windKmh: 0, windFrom: null, night: false, rain: 0, snow: false };

/** A forecast older than this is too old to sail by (tug refreshes every 15 minutes when it can). */
export const FRESH_MS = 3 * 60 * 60 * 1000;

/** The weather card's forecast as it stands now → today's water. Nothing fresh → calm seas. */
export function conditionsFrom(forecast: Forecast | null | undefined, nowMs: number): Conditions {
  if (!forecast || !(nowMs - forecast.fetchedAt < FRESH_MS) || nowMs < forecast.fetchedAt - 60_000) return CALM_SEAS;
  const now = withCurrentHour(forecast, nowMs).now;
  const code = now.code;
  const sky = describeSky(code).sky;
  let rain: Conditions["rain"] = 0;
  if (sky === "drizzle") rain = 1;
  else if (sky === "rain") rain = code === 65 || code === 82 ? 3 : 2;
  else if (sky === "storm") rain = 3;
  const wind = Number.isFinite(now.wind) ? Math.max(0, now.wind) : 0;
  const from = typeof now.windDir === "number" && Number.isFinite(now.windDir) ? now.windDir : null;
  return { live: true, sky, windKmh: wind, windFrom: from, night: !now.day, rain, snow: sky === "snow" };
}

/** Wind under this (km/h) doesn't move a tugboat. */
const STILL_WIND = 8;
/** The strongest sideways push wind gives, field units a second (a gale; the helm still wins). */
const MAX_DRIFT = 9;
const WAVES = [0, 5, 8, 11] as const;

/** What today's weather does to the water (see `Handling` in tugboatRun.ts). */
export function handlingFor(c: Conditions): Handling {
  if (!c.live) return CALM;
  const strength = Math.min(1, Math.max(0, (c.windKmh - STILL_WIND) / 42));
  // Wind blowing from the west pushes east (right); the channel runs north up the screen, so wind
  // along it barely pushes at all. Without a direction, it pushes to the right.
  const across = c.windFrom === null ? 1 : -Math.sin((c.windFrom * Math.PI) / 180);
  const drift = Math.round(strength * MAX_DRIFT * across * 100) / 100 || 0;
  return {
    speedScale: c.snow ? 0.82 : 1,
    response: c.snow ? RESPONSE * 0.38 : RESPONSE,
    followTau: c.snow ? FOLLOW_TAU * 2.4 : FOLLOW_TAU,
    drift,
    waves: WAVES[c.rain],
  };
}

const SKY_WORD: Record<Sky, string> = {
  clear: "clear",
  partly: "partly cloudy",
  cloudy: "cloudy",
  fog: "foggy",
  drizzle: "drizzly",
  rain: "rainy",
  snow: "snowy",
  storm: "stormy",
};

function list(items: string[]): string {
  return items.length <= 1 ? (items[0] ?? "") : `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

/** What a tugboat should expect from today's water (the start screen's "expect …"). */
export function boatOutlook(c: Conditions, h: Handling): string[] {
  const expect: string[] = [];
  if (c.snow) expect.push("icy, slower water");
  if (h.waves > 0) expect.push("waves");
  if (Math.abs(h.drift) >= 2) expect.push("a crosswind");
  if (c.night) expect.push("a lighthouse to steer by");
  return expect;
}

/**
 * The start screen's line about today: "Today: rainy, wind 12 mph — expect waves", or "Calm seas
 * today" when there's no real weather to go on. `outlook` says what the weather means for the game
 * (Tugboat Run's water by default; Harbor Stack has its own).
 */
export function conditionsLine(c: Conditions, unit: Unit, outlook: (c: Conditions, h: Handling) => string[] = boatOutlook): string {
  if (!c.live) return "Calm seas today";
  const sky = c.rain === 3 && c.sky === "rain" ? "pouring" : c.sky ? SKY_WORD[c.sky] : "fair";
  const wind = Math.round(c.windKmh) >= 1 ? `wind ${windText(c.windKmh, unit)}` : "no wind";
  const expect = outlook(c, handlingFor(c));
  const outlookText = expect.length ? `expect ${list(expect)}` : "smooth sailing";
  return `${c.night ? "Tonight" : "Today"}: ${sky}, ${wind} — ${outlookText}`;
}

// --- The streak ---

export interface DailyRecord {
  /** Days in a row with a finished daily run, ending on `last`. */
  streak: number;
  /** The newest day with a finished daily run ("2026-10-16"), if any. */
  last: string | null;
  /** The best daily score for each recent day. */
  bests: Record<string, number>;
}

export const EMPTY_DAILY: DailyRecord = { streak: 0, last: null, bests: {} };
/** Days of daily bests kept (the setting stays small). */
export const KEEP_DAYS = 31;

const DATE = /^\d{4}-\d{2}-\d{2}$/;

/** The saved record, whatever shape it's in (missing or damaged reads as a fresh start). */
export function parseDaily(raw: string | null | undefined): DailyRecord {
  try {
    const j = JSON.parse(raw ?? "null") as Partial<DailyRecord> | null;
    if (!j || typeof j !== "object") return EMPTY_DAILY;
    const last = typeof j.last === "string" && DATE.test(j.last) ? j.last : null;
    const streak = last && Number.isInteger(j.streak) && (j.streak as number) > 0 ? (j.streak as number) : 0;
    const bests: Record<string, number> = {};
    if (j.bests && typeof j.bests === "object") {
      for (const [d, s] of Object.entries(j.bests)) if (DATE.test(d) && Number.isFinite(s) && s >= 0) bests[d] = Math.floor(s);
    }
    return { streak: last ? Math.max(1, streak) : 0, last, bests };
  } catch {
    return EMPTY_DAILY;
  }
}

/**
 * A daily run finished: `date` is the day of the course it ran (the day it started, so a run
 * that ends after midnight still counts for the day it was). The day after the last one adds a
 * day to the streak; after a gap it starts again at 1; another run on a day already counted only
 * updates the best. A date older than the last (the clock went back) only records its best.
 */
export function finishDaily(rec: DailyRecord, date: string, score: number): { record: DailyRecord; newBest: boolean } {
  const s = Math.max(0, Math.floor(score));
  const before = rec.bests[date];
  const newBest = before === undefined || s > before;
  const bests = { ...rec.bests, [date]: newBest ? s : before };
  // Only the newest days are kept.
  const keep = Object.keys(bests).sort().slice(-KEEP_DAYS);
  const trimmed = Object.fromEntries(keep.map((d) => [d, bests[d]]));
  let { streak, last } = rec;
  if (last === null || date > last) {
    streak = last !== null && last === dayBefore(date) ? streak + 1 : 1;
    last = date;
  }
  return { record: { streak, last, bests: trimmed }, newBest };
}

/**
 * The streak as of `today`: alive if the last finished daily run was today or yesterday (today's
 * can still be sailed), otherwise broken, so 0. Read at the moment it's shown, so it's right after
 * midnight and after days away.
 */
export function currentStreak(rec: DailyRecord, today: string): number {
  if (rec.last === null) return 0;
  return rec.last === today || rec.last === dayBefore(today) ? rec.streak : 0;
}

/** Whether a daily run has been finished today. */
export function sailedToday(rec: DailyRecord, today: string): boolean {
  return rec.bests[today] !== undefined;
}
