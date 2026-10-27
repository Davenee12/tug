// Pickups saved: a gentle count of the times tug spared you reaching for the phone. A reply sent,
// a code copied or typed, a call answered or returned, the music controlled: each is a moment you'd
// otherwise have picked the iPhone up. A burst of the same kind (skipping five songs, three replies
// in one conversation) is one pickup, because it would have been one pickup of the phone.
//
// Kept on this PC only, in tug's settings (`ui.pickups`). Pure: the store passes the time.

/** What was done from the PC instead of the phone. */
export type PickupKind = "reply" | "code" | "call" | "music";

/** The same kind again within this long is the same pickup. */
export const BURST_MS = 2 * 60 * 1000;

export interface Pickups {
  /** Every pickup saved since tug started counting. */
  total: number;
  /** This week's, from `weekStart`. */
  week: number;
  /** Local midnight on the Monday the current week began (ms). */
  weekStart: number;
  /** When each kind last counted, for the burst rule (ms). */
  last: Partial<Record<PickupKind, number>>;
}

export const EMPTY: Pickups = { total: 0, week: 0, weekStart: 0, last: {} };

/** Local midnight on the Monday of `now`'s week. */
export function mondayOf(now: number): number {
  const d = new Date(now);
  d.setHours(0, 0, 0, 0);
  const sinceMonday = (d.getDay() + 6) % 7;
  d.setDate(d.getDate() - sinceMonday);
  return d.getTime();
}

/** One more pickup of `kind` at `now`, unless it's part of a burst; a new week starts its count afresh. */
export function record(p: Pickups, kind: PickupKind, now: number): Pickups {
  const weekStart = mondayOf(now);
  const week = weekStart === p.weekStart ? p.week : 0;
  const prev = p.last[kind];
  if (prev !== undefined && now >= prev && now - prev < BURST_MS) {
    return { ...p, week, weekStart, last: { ...p.last, [kind]: now } };
  }
  return { total: p.total + 1, week: week + 1, weekStart, last: { ...p.last, [kind]: now } };
}

/** This week's count as of `now` (0 once a new week has begun). */
export function thisWeek(p: Pickups, now: number): number {
  return mondayOf(now) === p.weekStart ? p.week : 0;
}

/** What the Feed's greeting adds, or null before there's anything to say. */
export function pickupsLine(p: Pickups, now: number): string | null {
  const week = thisWeek(p, now);
  if (week === 0) return null;
  return `tug saved you ${week} ${week === 1 ? "pickup" : "pickups"} of your phone this week`;
}

/** Settings › About: the all-time count, or null before the first one. */
export function totalLine(p: Pickups): string | null {
  if (p.total === 0) return null;
  return `${p.total} ${p.total === 1 ? "time" : "times"} you didn't have to pick up your phone.`;
}

/** The saved value, whatever shape it's in: anything odd starts the count from nothing. */
export function parsePickups(raw: string | undefined): Pickups {
  try {
    const v = JSON.parse(raw ?? "") as Partial<Pickups>;
    const whole = (n: unknown) => (typeof n === "number" && Number.isFinite(n) && n >= 0 ? Math.floor(n) : 0);
    const last: Pickups["last"] = {};
    for (const k of ["reply", "code", "call", "music"] as PickupKind[]) {
      const t = v.last?.[k];
      if (typeof t === "number" && Number.isFinite(t)) last[k] = t;
    }
    return { total: whole(v.total), week: whole(v.week), weekStart: whole(v.weekStart), last };
  } catch {
    return EMPTY;
  }
}
