// The weather card's harbor: what the little scene shows, worked out from the forecast and the
// time at the place. The sun and moon where they are between the place's real sunrise and sunset,
// the moon's real phase, clouds from the cloud cover, rain or snow at the strength that's falling,
// fog, a storm's lightning, and waves and the tug's flag from the real wind on the Beaufort scale.
// Also the card's two new lines: rain to the quarter-hour, and "before you head out", which only
// tug can say because it sees your phone's battery as well as the weather.
//
// Pure: the component passes the time; weatherHarborDraw draws it.

import { describe, quarterOf, type Day, type Forecast, type Quarter } from "./weather";

export type Phase = "night" | "dawn" | "day" | "dusk";

export interface HarborScene {
  phase: Phase;
  /** Along the day's arc, 0 at sunrise to 1 at sunset, and how high (0–1); null at night. */
  sun: { at: number; height: number } | null;
  /** Along the night's arc, its height, its phase (0 new, 0.5 full) and how much is lit (0–1). */
  moon: { at: number; height: number; phase: number; lit: number } | null;
  /** How much of the sky is cloud, 0–1. */
  clouds: number;
  /** How hard it's raining or snowing, 0–1. */
  rain: number;
  snow: number;
  fog: boolean;
  storm: boolean;
  wind: Wind;
}

export interface Wind {
  /** Beaufort force, 0–12. */
  force: number;
  name: string;
  /** Where it blows from, degrees, when known. */
  from: number | null;
  /** How much it pushes east: 1 blowing due east, −1 due west, 0 north or south (or unknown). */
  push: number;
}

const MINUTE = 60_000;
/** Sunrise and sunset glow this long either side. */
const TWILIGHT_MIN = 40;

/** A place-local "2026-10-05T14:37" as minutes, for arithmetic between local times (never shown). */
export function localMinutes(local: string): number | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/.exec(local);
  if (!m) return null;
  return Date.UTC(+m[1], +m[2] - 1, +m[3], +m[4], +m[5]) / MINUTE;
}

/** The place's time now as "YYYY-MM-DDTHH:MM", in its own zone when known. */
export function placeNow(nowMs: number, timeZone?: string): string {
  let parts: Intl.DateTimeFormatPart[];
  try {
    parts = new Intl.DateTimeFormat("en-CA", {
      timeZone,
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      hourCycle: "h23",
    }).formatToParts(new Date(nowMs));
  } catch {
    return placeNow(nowMs); // an unknown zone name: the PC's own
  }
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? "00";
  return `${get("year")}-${get("month")}-${get("day")}T${get("hour")}:${get("minute")}`;
}

// ---- Wind on the Beaufort scale ----

const BEAUFORT_KMH = [1, 6, 12, 20, 29, 39, 50, 62, 75, 89, 103, 118];
const BEAUFORT_NAMES = [
  "Calm",
  "Light air",
  "Light breeze",
  "Gentle breeze",
  "Moderate breeze",
  "Fresh breeze",
  "Strong breeze",
  "Near gale",
  "Gale",
  "Strong gale",
  "Storm",
  "Violent storm",
  "Hurricane force",
];

/** The Beaufort force for a wind speed in km/h. */
export function beaufort(kmh: number): number {
  const i = BEAUFORT_KMH.findIndex((limit) => kmh < limit);
  return i < 0 ? 12 : i;
}

const COMPASS = ["north", "northeast", "east", "southeast", "south", "southwest", "west", "northwest"];

/** "southwest" for 225°. */
export function compass(deg: number): string {
  return COMPASS[Math.round((((deg % 360) + 360) % 360) / 45) % 8];
}

export function windOf(kmh: number, from: number | null | undefined): Wind {
  const force = beaufort(kmh);
  const known = typeof from === "number" && Number.isFinite(from);
  // It blows toward the opposite of where it comes from. Which side of the scene east is, the
  // drawing decides (the harbor faces the equator).
  const push = known ? Math.round(Math.sin(((from + 180) * Math.PI) / 180) * 100) / 100 : 0;
  return { force, name: BEAUFORT_NAMES[force], from: known ? from : null, push };
}

/** "Force 4 · moderate breeze from the southwest", the way a harbor would say it. */
export function seaLine(w: Wind): string {
  if (w.force === 0) return "Force 0 · flat calm";
  const what = w.name.charAt(0).toLowerCase() + w.name.slice(1);
  return `Force ${w.force} · ${what}${w.from !== null ? ` from the ${compass(w.from)}` : ""}`;
}

// ---- The moon ----

/** A new moon (2000-01-06 18:14 UTC) and the length of a lunar month. */
const NEW_MOON_MS = Date.UTC(2000, 0, 6, 18, 14);
const SYNODIC_DAYS = 29.530588853;

/** How far through its cycle the moon is: 0 new, 0.25 first quarter, 0.5 full, 0.75 last quarter. */
export function moonPhase(nowMs: number): number {
  const days = (nowMs - NEW_MOON_MS) / 86_400_000;
  return (((days / SYNODIC_DAYS) % 1) + 1) % 1;
}

export function moonName(phase: number): string {
  const names = ["New moon", "Waxing crescent", "First quarter", "Waxing gibbous", "Full moon", "Waning gibbous", "Last quarter", "Waning crescent"];
  return names[Math.round(phase * 8) % 8];
}

// ---- The scene ----

/** How hard it's coming down, from the weather code and the rain measured now. */
function fall(code: number, precipMm: number | undefined): { rain: number; snow: number } {
  const byCode: Record<number, [number, number]> = {
    51: [0.2, 0], 53: [0.3, 0], 55: [0.4, 0], 56: [0.3, 0], 57: [0.4, 0],
    61: [0.4, 0], 63: [0.65, 0], 65: [0.9, 0], 66: [0.5, 0], 67: [0.8, 0],
    71: [0, 0.35], 73: [0, 0.6], 75: [0, 0.9], 77: [0, 0.3],
    80: [0.5, 0], 81: [0.7, 0], 82: [0.95, 0], 85: [0, 0.6], 86: [0, 0.9],
    95: [0.8, 0], 96: [0.85, 0], 99: [0.95, 0],
  };
  const [rain, snow] = byCode[code] ?? [0, 0];
  // What's measured falling now (the last quarter-hour; 1 mm in one is a downpour) lifts it, but
  // never invents rain the weather code doesn't have.
  const measured = precipMm !== undefined && (rain > 0 || snow > 0) ? Math.min(1, precipMm) : 0;
  return { rain: rain > 0 ? Math.max(rain, measured) : 0, snow: snow > 0 ? Math.max(snow, measured) : 0 };
}

function cloudsOf(f: Forecast): number {
  if (typeof f.now.clouds === "number") return Math.min(1, Math.max(0, f.now.clouds / 100));
  const sky = describe(f.now.code).sky;
  return { clear: 0.1, partly: 0.45, cloudy: 0.9, fog: 0.7, drizzle: 0.8, rain: 0.9, snow: 0.9, storm: 1 }[sky];
}

/**
 * A day's sunrise and sunset as local minutes, when they're real: on the day, sunset after sunrise.
 * Near the poles the sun can stay up or down all day, and the service's placeholder times for that
 * mustn't be drawn or shown.
 */
export function sunTimes(day: Day | undefined): { rise: number; set: number } | null {
  const rise = day?.sunrise ? localMinutes(day.sunrise) : null;
  const set = day?.sunset ? localMinutes(day.sunset) : null;
  const start = day ? localMinutes(`${day.date}T00:00`) : null;
  if (rise === null || set === null || start === null) return null;
  const onTheDay = (m: number) => m >= start - 12 * 60 && m <= start + 36 * 60;
  return set > rise && onTheDay(rise) && onTheDay(set) ? { rise, set } : null;
}

/** Where the sun or moon is along an arc from `start` to `end` (local minutes). */
function arc(now: number, start: number, end: number): { at: number; height: number } {
  const at = Math.min(1, Math.max(0, (now - start) / (end - start)));
  return { at, height: Math.sin(Math.PI * at) };
}

/**
 * The harbor at `nowMs`. `f` is the forecast with the current hour applied (weather's
 * `withCurrentHour`). Without sunrise and sunset (an older cached forecast) the sun sits high by
 * day and the moon by night.
 */
export function harborScene(f: Forecast, nowMs: number): HarborScene {
  const now = localMinutes(placeNow(nowMs, f.timezone)) ?? 0;
  const sunToday = sunTimes(f.days[0]);
  const sunTomorrow = sunTimes(f.days[1]);
  const phaseNow = moonPhase(nowMs);
  const lit = (1 - Math.cos(2 * Math.PI * phaseNow)) / 2;

  let phase: Phase;
  let sun: HarborScene["sun"] = null;
  let moon: HarborScene["moon"] = null;
  if (!sunToday) {
    phase = f.now.day ? "day" : "night";
    if (f.now.day) sun = { at: 0.5, height: 1 };
    else moon = { at: 0.5, height: 1, phase: phaseNow, lit };
  } else if (now >= sunToday.rise && now <= sunToday.set) {
    const { rise, set } = sunToday;
    sun = arc(now, rise, set);
    phase = now - rise < TWILIGHT_MIN ? "dawn" : set - now < TWILIGHT_MIN ? "dusk" : "day";
  } else {
    // The night runs from this evening's sunset to tomorrow's sunrise, or from last night's to this
    // morning's (yesterday's sunset is close enough to today's less a day).
    const { rise, set } = sunToday;
    const nextRise = sunTomorrow && sunTomorrow.rise > set ? sunTomorrow.rise : rise + 24 * 60;
    const [start, end] = now > set ? [set, nextRise] : [set - 24 * 60, rise];
    moon = { ...arc(now, start, end), phase: phaseNow, lit };
    phase = now > set && now - set < TWILIGHT_MIN ? "dusk" : now < rise && rise - now < TWILIGHT_MIN ? "dawn" : "night";
  }

  const code = f.now.code;
  const { rain, snow } = fall(code, f.now.precip);
  return {
    phase,
    sun,
    moon,
    clouds: cloudsOf(f),
    rain,
    snow,
    fog: code === 45 || code === 48,
    storm: code >= 95,
    wind: windOf(f.now.gusts !== undefined ? Math.max(f.now.wind, f.now.gusts * 0.7) : f.now.wind, f.now.windDir),
  };
}

/**
 * The harbor's small caption: "Sunset 6:40 PM" by day; by night the moon and the next sunrise,
 * "Waxing gibbous · Sunrise 7:12 AM". Times are the place's own.
 */
export function skyCaption(s: HarborScene, f: Forecast, nowLocal: string): string | null {
  const [today, tomorrow] = f.days;
  const real = (d: Day | undefined) => (sunTimes(d) ? d : undefined);
  if (s.sun) {
    const sun = sunTimes(today);
    if (!sun) return null;
    const now = localMinutes(nowLocal) ?? 0;
    const golden = now >= sun.set - GOLDEN_MIN && now <= sun.set;
    return `${golden ? "Golden hour · " : ""}Sunset ${wallClock(today.sunset!)}`;
  }
  const moon = s.moon ? moonName(s.moon.phase) : null;
  const next = real(today) && nowLocal < today.sunrise! ? today.sunrise : real(tomorrow)?.sunrise;
  const rise = next ? `Sunrise ${wallClock(next)}` : null;
  return [moon, rise].filter(Boolean).join(" · ") || null;
}

/** The golden hour, near enough: the last hour of sunlight. */
const GOLDEN_MIN = 60;

function hoursAndMinutes(min: number): string {
  const h = Math.floor(min / 60);
  const m = Math.round(min % 60);
  return h ? `${h} h${m ? ` ${m} min` : ""}` : `${m} min`;
}

/**
 * For the card's details: "Daylight 10 h 48 min, 2 min less tomorrow · Golden hour from about
 * 5:06 PM". Null without real sunrise and sunset (an older forecast, or a polar day or night).
 */
export function daylightLine(f: Forecast): string | null {
  const today = sunTimes(f.days[0]);
  if (!today) return null;
  const length = today.set - today.rise;
  let daylight = `Daylight ${hoursAndMinutes(length)}`;
  const tomorrow = sunTimes(f.days[1]);
  if (tomorrow) {
    const change = tomorrow.set - tomorrow.rise - length;
    if (change !== 0) daylight += `, ${Math.abs(change)} min ${change > 0 ? "more" : "less"} tomorrow`;
  }
  return `${daylight} · Golden hour from about ${clockOf(today.set - GOLDEN_MIN)}`;
}

// ---- Rain to the quarter-hour ----

/** Rain this light in a quarter-hour (mm) is a few spots, not rain. */
const WET_MM = 0.1;

export interface NextRain {
  /** "Rain from about 5:45 PM, for about 30 minutes", or null when the next two hours are dry. */
  line: string | null;
  /** When it starts: "now", a clock time, or null when it doesn't. */
  start: string | null;
  /** The quarters still ahead, for the strip under the line. */
  quarters: Quarter[];
}

let clockFormat: Intl.DateTimeFormat | null = null;

/**
 * The place's "2026-10-05T17:45" as "5:45 PM", in the PC's own style. Read as UTC so the PC's
 * daylight-saving change can't move a time at the place by an hour.
 */
function wallClock(local: string): string {
  const m = localMinutes(local);
  return m === null ? "" : clockOf(m);
}

function clockOf(localMin: number): string {
  clockFormat ??= new Intl.DateTimeFormat([], { hour: "numeric", minute: "2-digit", timeZone: "UTC" });
  return clockFormat.format(new Date(localMin * MINUTE));
}

/** The harbor reads this far ahead: two hours, a quarter at a time. */
const AHEAD = 8;

function minutesWord(n: number): string {
  if (n < 60) return `about ${n} minutes`;
  if (n < 90) return "about an hour";
  return `about ${Math.round(n / 30) / 2} hours`;
}

/** The next two hours' rain (or snow), from the quarters still ahead of the place's `nowLocal`. */
export function nextRain(quarters: Quarter[] | undefined, nowLocal: string, what: "Rain" | "Snow" = "Rain"): NextRain {
  const ahead = (quarters ?? []).filter((q) => q.time.slice(0, 16) >= quarterOf(nowLocal)).slice(0, AHEAD);
  const wet = (q: Quarter) => q.precip >= WET_MM;
  const none = { line: null, start: null, quarters: ahead };
  if (ahead.length < 2) return none;
  if (wet(ahead[0])) {
    const dry = ahead.findIndex((q) => !wet(q));
    const line =
      dry < 0
        ? `${what} for the next ${ahead.length >= AHEAD ? "2 hours" : `${ahead.length * 15} minutes`} or more`
        : `${what} easing off around ${wallClock(ahead[dry].time)}`;
    return { line, start: "now", quarters: ahead };
  }
  const first = ahead.findIndex(wet);
  if (first < 0) return none;
  const stop = ahead.findIndex((q, i) => i > first && !wet(q));
  const start = wallClock(ahead[first].time);
  const from = `${what} from about ${start}`;
  return { line: stop < 0 ? from : `${from}, for ${minutesWord((stop - first) * 15)}`, start, quarters: ahead };
}

// ---- Before you head out ----

/** A phone at or below this is worth a word before going out in the wet or the cold. */
export const LOW_FOR_OUTSIDE = 30;

/**
 * The one thing only tug can say: the weather about to happen and your phone's battery, together.
 * Rain or snow in the next two hours (`start`, from `nextRain`; the card shows that line too), or
 * a freezing feels-like, with the phone at 30% or less. Null otherwise.
 */
export function beforeYouGo(opts: {
  battery: number | null;
  start: string | null;
  what?: "Rain" | "Snow";
  feelsC: number;
  feelsLabel: string;
}): string | null {
  const { battery, start, what = "Rain", feelsC, feelsLabel } = opts;
  if (battery === null || battery > LOW_FOR_OUTSIDE) return null;
  const fall = what === "Snow" ? "snow" : "rain";
  // The rain line sits just above this one, so it isn't said twice.
  if (start === "now") return `Your phone's at ${battery}%. Charge it before you head out in the ${fall}.`;
  if (start) return `Your phone's at ${battery}%. Charge it before the ${fall} at ${start}.`;
  if (feelsC <= 0) {
    return `It feels like ${feelsLabel} out and your phone's at ${battery}%. Cold drains batteries faster, so charge it before you go.`;
  }
  return null;
}

/** The quarters' strip: how tall each bar is, 0–1 (a downpour, over 1 mm in a quarter-hour, fills it). */
export function rainBars(quarters: Quarter[]): number[] {
  return quarters.map((q) => (q.precip < WET_MM ? 0 : Math.min(1, 0.3 + q.precip / 1.5)));
}

/** For tests and the dev preview: minutes from now as a place-local quarter time. */
export function quarterAfter(nowLocal: string, minutes: number): string {
  const base = localMinutes(quarterOf(nowLocal)) ?? 0;
  const d = new Date((base + minutes) * MINUTE);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}T${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}`;
}
