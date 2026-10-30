// "Rain soon": a Windows pop-up shortly before rain (or snow) starts at the weather card's place,
// off until it's turned on in Settings › Weather. It reads the same quarter-hour rain as the card's
// line (lib/weatherHarbor `nextRain`), so the two never disagree, and adds the phone's battery when
// it's low, which only tug can.
//
// Pure: the weather store passes the time, the forecast and the battery, and shows the pop-up.

import type { Quarter } from "./weather";
import { LOW_FOR_OUTSIDE, localMinutes, nextRain } from "./weatherHarbor";

/** Tell this far ahead at most: sooner is a surprise, earlier is often wrong. */
export const RAIN_ALERT_LEAD_MIN = 20;
/** A spell starting this soon after the last one told about is the same weather: say nothing. */
export const RAIN_ALERT_GAP_MIN = 120;

export interface RainAlert {
  title: string;
  body: string;
  /** The start of the spell told about (place-local "2026-10-05T17:45"), kept so it's told once. */
  spell: string;
}

/**
 * The pop-up due at the place's `nowLocal`, if any: rain starting within RAIN_ALERT_LEAD_MIN
 * minutes (not already falling), not the spell `lastSpell` already told about, nor one starting
 * within RAIN_ALERT_GAP_MIN of it. `place` is the card's place name, `battery` the phone's level
 * when known.
 */
export function rainAlert(opts: {
  quarters: Quarter[] | undefined;
  nowLocal: string;
  what: "Rain" | "Snow";
  place: string;
  battery: number | null;
  lastSpell: string | null;
}): RainAlert | null {
  const { quarters, nowLocal, what, place, battery, lastSpell } = opts;
  const soon = nextRain(quarters, nowLocal, what);
  if (!soon.line || !soon.start || soon.start === "now") return null;
  const first = soon.quarters.find((q) => q.precip >= 0.1);
  const now = localMinutes(nowLocal);
  const start = first ? localMinutes(first.time) : null;
  if (!first || now === null || start === null) return null;
  if (start - now > RAIN_ALERT_LEAD_MIN) return null;
  const last = lastSpell ? localMinutes(lastSpell) : null;
  if (last !== null && start >= last && start - last < RAIN_ALERT_GAP_MIN) return null;
  const town = place.split(",")[0].trim();
  const low = battery !== null && battery <= LOW_FOR_OUTSIDE ? ` Your phone's at ${battery}%: charge it before you head out.` : "";
  return {
    title: `${what} soon${town ? ` in ${town}` : ""}`,
    body: `${soon.line}.${low}`,
    spell: first.time.slice(0, 16),
  };
}
