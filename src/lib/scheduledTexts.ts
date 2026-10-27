// Send later: a text written now, sent through the iPhone at a time you pick. It goes once, at
// most: the store takes it off the list before it sends. If the phone isn't connected when it's due,
// it waits; a text more than LATE_MS late (tug was closed, the phone was away) is never sent by
// itself, because "On my way" an hour late does more harm than good. It's held in its conversation
// for you to Send now or Discard.
//
// Kept on this PC only, in tug's settings (`ui.scheduledTexts`). Pure: the store passes the time.

export interface ScheduledText {
  id: string;
  /** The number it goes to, as the composer would send it. */
  address: string;
  /** Who it's to, for notes ("Sent your text to Jane"). */
  name: string;
  /** The conversation it belongs to. */
  threadKey: string;
  text: string;
  /** When to send it (ms). */
  at: number;
  createdAt: number;
  /** "waiting" to go; "held" when it couldn't go in time and waits for the person instead. */
  state: "waiting" | "held";
}

/** Due longer ago than this, a text isn't sent by itself. */
export const LATE_MS = 30 * 60 * 1000;
export const MAX_SCHEDULED = 50;

export interface Plan {
  /** Due, on time, and the phone can take it: send these now. */
  send: ScheduledText[];
  /** Due too long ago: hold these for the person. */
  hold: ScheduledText[];
}

/** What to do at `now`. `canSend`: the texts connection to the iPhone is up. */
export function plan(list: ScheduledText[], now: number, canSend: boolean): Plan {
  const send: ScheduledText[] = [];
  const hold: ScheduledText[] = [];
  for (const t of list) {
    if (t.state !== "waiting" || t.at > now) continue;
    if (now - t.at > LATE_MS) hold.push(t);
    else if (canSend) send.push(t);
  }
  return { send, hold };
}

/** The list with `t` added, soonest first (refused beyond MAX_SCHEDULED: null). */
export function addScheduled(list: ScheduledText[], t: ScheduledText): ScheduledText[] | null {
  if (list.length >= MAX_SCHEDULED) return null;
  return [...list, t].sort((a, b) => a.at - b.at);
}

export function withoutScheduled(list: ScheduledText[], id: string): ScheduledText[] {
  return list.filter((t) => t.id !== id);
}

export function held(list: ScheduledText[], ids: string[]): ScheduledText[] {
  return list.map((t) => (ids.includes(t.id) ? { ...t, state: "held" } : t));
}

/** A conversation's scheduled texts, soonest first. */
export function forThread(list: ScheduledText[], threadKey: string): ScheduledText[] {
  return list.filter((t) => t.threadKey === threadKey);
}

/** The saved list, whatever shape it's in: anything odd is left out. */
export function parseScheduled(raw: string | undefined): ScheduledText[] {
  let v: unknown;
  try {
    v = JSON.parse(raw ?? "[]");
  } catch {
    return [];
  }
  if (!Array.isArray(v)) return [];
  const out: ScheduledText[] = [];
  for (const x of v as Partial<ScheduledText>[]) {
    if (!x || typeof x !== "object") continue;
    const strings = [x.id, x.address, x.name, x.threadKey, x.text];
    if (strings.some((s) => typeof s !== "string") || !x.text?.trim() || !x.address) continue;
    if (typeof x.at !== "number" || !Number.isFinite(x.at)) continue;
    out.push({
      id: x.id!,
      address: x.address,
      name: x.name!,
      threadKey: x.threadKey!,
      text: x.text,
      at: x.at,
      createdAt: typeof x.createdAt === "number" ? x.createdAt : x.at,
      state: x.state === "held" ? "held" : "waiting",
    });
  }
  return out.sort((a, b) => a.at - b.at).slice(0, MAX_SCHEDULED);
}
