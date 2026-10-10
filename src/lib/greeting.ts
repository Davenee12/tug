// The one-line hello at the top of the Feed: the time of day and, with weather set up, what it's
// like out and a light suggestion. "Good morning ☀️ 84° and sunny. Pool weather."

import { describe, hourLabel, temp, wet, type Forecast, type Sky, type Unit } from "./weather";

export type Bucket = "morning" | "afternoon" | "evening" | "night";

const BUCKETS: Bucket[] = ["morning", "afternoon", "evening", "night"];

/** Morning 5–12, afternoon 12–17, evening 17–22, night 22–5, by this PC's clock. */
export function bucketOf(date: Date): Bucket {
  const h = date.getHours();
  if (h >= 5 && h < 12) return "morning";
  if (h >= 12 && h < 17) return "afternoon";
  if (h >= 17 && h < 22) return "evening";
  return "night";
}

const EMOJI: Record<Sky, [string, string]> = {
  clear: ["☀️", "🌙"],
  partly: ["⛅", "☁️"],
  cloudy: ["☁️", "☁️"],
  fog: ["🌫️", "🌫️"],
  drizzle: ["🌦️", "🌧️"],
  rain: ["🌧️", "🌧️"],
  snow: ["🌨️", "🌨️"],
  storm: ["⛈️", "⛈️"],
};

const SKY_WORD: Record<Sky, [string, string]> = {
  clear: ["sunny", "clear"],
  partly: ["partly cloudy", "partly cloudy"],
  cloudy: ["cloudy", "cloudy"],
  fog: ["foggy", "foggy"],
  drizzle: ["drizzly", "drizzly"],
  rain: ["rainy", "rainy"],
  snow: ["snowy", "snowy"],
  storm: ["stormy", "stormy"],
};

/** Hot and cold, in °C (what the forecast carries). 28 °C ≈ 82 °F, 10 °C = 50 °F. */
const HOT_C = 28;
const CHILLY_C = 10;
const COLD_C = 0;
const MILD_C = 18;

/** "9 PM", "noon", "midnight", for the middle of a sentence. */
function untilLabel(iso: string): string {
  const l = hourLabel(iso);
  return l === "Noon" ? "noon" : l === "12 AM" ? "midnight" : l;
}

/** What it's like out: "84° and sunny", "Rain until 9 PM", "Storms nearby". */
function conditions(f: Forecast, sky: Sky, unit: Unit): string {
  if (sky === "storm") return "Storms nearby";
  if (sky === "rain" || sky === "drizzle" || sky === "snow") {
    const dry = f.hours.slice(1, 13).find((h) => (h.rain ?? 0) < 50 && !wet(h.code));
    const what = sky === "snow" ? "Snow" : sky === "drizzle" ? "Drizzle" : "Rain";
    if (dry) return `${what} until ${untilLabel(dry.time)}`;
  }
  return `${temp(f.now.temp, unit)}° and ${SKY_WORD[sky][f.now.day ? 0 : 1]}`;
}

/** A light suggestion for the moment, a few to choose from, or none. */
function suggestions(sky: Sky, c: number, when: Bucket): string[] {
  const out = when === "morning" || when === "afternoon";
  switch (sky) {
    case "storm":
      return ["Maybe skip the walk.", "A good time to stay in."];
    case "rain":
    case "drizzle":
      return out ? ["Bring an umbrella.", "Umbrella weather."] : ["A good night to stay in.", "Cozy night in."];
    case "snow":
      return ["Bundle up.", "Take it easy on the roads."];
    case "fog":
      return ["Take it slow out there."];
  }
  if (c >= HOT_C) return out ? ["Pool weather.", "Stay cool out there.", "A day for something cold."] : ["A warm one tonight."];
  if (c <= COLD_C) return ["Bundle up.", "Coat weather."];
  if (c <= CHILLY_C) return ["Grab a jacket."];
  if (c >= MILD_C && sky !== "cloudy") {
    if (out) return ["Nice day for a walk.", "A good day to get outside."];
    if (when === "evening") return ["A nice evening to be outside."];
  }
  return [];
}

/**
 * The Feed's greeting. Deterministic: the same moment and forecast always give the same line,
 * and the variant only moves on with the day and the part of the day, so it never flickers.
 * No name, ever.
 */
export function greeting(date: Date, forecast: Forecast | null, unit: Unit): string {
  const when = bucketOf(date);
  const hello = `Good ${when}`;
  if (!forecast) return hello;
  const { sky } = describe(forecast.now.code);
  const emoji = EMOJI[sky][forecast.now.day ? 0 : 1];
  const ideas = suggestions(sky, forecast.now.temp, when);
  const idea = ideas.length ? ` ${ideas[(date.getDate() + BUCKETS.indexOf(when)) % ideas.length]}` : "";
  return `${hello} ${emoji} ${conditions(forecast, sky, unit)}.${idea}`;
}
