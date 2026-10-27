// Remind me: put a text or notification aside and have tug bring it back at a better time, as a
// Windows pop-up (the notification's own, with its buttons, while it's still in tug) and a note in
// the window. Reminders are kept on this PC only, in tug's settings (`ui.reminders`), so they
// survive a restart; one that came due while tug was closed comes up when it starts.
//
// Pure: the store passes the time and does the saving, the timer and the pop-up.

import { clockTime } from "./format";

export interface Reminder {
  id: string;
  /** When to bring it back (ms). */
  at: number;
  /** Who or what it's about, as the row shows it ("Jane Doe", "Gmail"). */
  title: string;
  /** The line it's about, so the reminder still says something if the notification is gone. */
  body: string;
  /** The notification, for its own pop-up and to open it. */
  notificationId: number | null;
  /** The conversation to open, for a person's texts. */
  threadKey: string | null;
  createdAt: number;
}

/** What the row's Remind me menu offers. */
export interface Choice {
  label: string;
  at: number;
}

export const MAX_REMINDERS = 50;
const MINUTE = 60 * 1000;

function at(now: Date, dayOffset: number, hour: number): number {
  const d = new Date(now);
  d.setDate(d.getDate() + dayOffset);
  d.setHours(hour, 0, 0, 0);
  return d.getTime();
}

/**
 * The times on offer from `now`: soon, later today, this evening (only while it's still a while
 * off) and tomorrow morning. Labels say when in the person's own clock format.
 */
export function choices(now: Date): Choice[] {
  const t = now.getTime();
  const out: Choice[] = [
    { label: "In 20 minutes", at: t + 20 * MINUTE },
    { label: "In 1 hour", at: t + 60 * MINUTE },
    { label: "In 3 hours", at: t + 180 * MINUTE },
  ];
  const evening = at(now, 0, 18);
  if (evening - t >= 90 * MINUTE) out.push({ label: `This evening (${clockTime(new Date(evening))})`, at: evening });
  const morning = at(now, 1, 9);
  out.push({ label: `Tomorrow morning (${clockTime(new Date(morning))})`, at: morning });
  return out;
}

/** When a pending reminder comes back, for the row's chip: "3:30 PM", or "Tomorrow 9:00 AM". */
export function whenLabel(when: number, now: number): string {
  const time = clockTime(new Date(when));
  const day = (ms: number) => {
    const d = new Date(ms);
    d.setHours(0, 0, 0, 0);
    return d.getTime();
  };
  const days = Math.round((day(when) - day(now)) / (24 * 60 * MINUTE));
  if (days <= 0) return time;
  if (days === 1) return `Tomorrow ${time}`;
  return `${new Date(when).toLocaleDateString([], { weekday: "short" })} ${time}`;
}

/** The list with `r` added, soonest first. A second reminder for the same thing replaces the first. */
export function addReminder(list: Reminder[], r: Reminder): Reminder[] {
  const rest = list.filter((x) => !sameTarget(x, r));
  return [...rest, r].sort((a, b) => a.at - b.at).slice(0, MAX_REMINDERS);
}

export function removeReminder(list: Reminder[], id: string): Reminder[] {
  return list.filter((r) => r.id !== id);
}

function sameTarget(a: Pick<Reminder, "notificationId" | "threadKey">, b: Pick<Reminder, "notificationId" | "threadKey">) {
  if (a.threadKey && b.threadKey) return a.threadKey === b.threadKey;
  return a.notificationId !== null && a.notificationId === b.notificationId;
}

/** The pending reminder for a row: its conversation, or any of its notifications. */
export function reminderFor(list: Reminder[], target: { threadKey: string | null; notificationIds: number[] }): Reminder | null {
  return (
    list.find((r) =>
      target.threadKey ? r.threadKey === target.threadKey : r.notificationId !== null && target.notificationIds.includes(r.notificationId),
    ) ?? null
  );
}

/** Split into what's due at `now` and what's still to come. */
export function splitDue(list: Reminder[], now: number): { due: Reminder[]; pending: Reminder[] } {
  return { due: list.filter((r) => r.at <= now), pending: list.filter((r) => r.at > now) };
}

/** A fresh id, unique enough for a list of at most 50 on one PC. */
export function reminderId(now: number, random = Math.random): string {
  return `${now.toString(36)}-${Math.floor(random() * 36 ** 6).toString(36)}`;
}

/** What the reminder's pop-up title says; one that came due while tug was closed says when it was due. */
export function reminderTitle(r: Reminder, now: number): string {
  const late = now - r.at > 5 * MINUTE;
  return late ? `Reminder (was due ${whenLabel(r.at, r.at)}) · ${r.title}` : `Reminder · ${r.title}`;
}

/** The saved list, whatever shape it's in: anything odd is left out. */
export function parseReminders(raw: string | undefined): Reminder[] {
  let v: unknown;
  try {
    v = JSON.parse(raw ?? "[]");
  } catch {
    return [];
  }
  if (!Array.isArray(v)) return [];
  const out: Reminder[] = [];
  for (const x of v as Partial<Reminder>[]) {
    if (!x || typeof x.id !== "string" || typeof x.at !== "number" || !Number.isFinite(x.at)) continue;
    if (typeof x.title !== "string" || typeof x.body !== "string") continue;
    const notificationId = typeof x.notificationId === "number" ? x.notificationId : null;
    const threadKey = typeof x.threadKey === "string" ? x.threadKey : null;
    if (notificationId === null && threadKey === null) continue;
    out.push({
      id: x.id,
      at: x.at,
      title: x.title,
      body: x.body,
      notificationId,
      threadKey,
      createdAt: typeof x.createdAt === "number" ? x.createdAt : x.at,
    });
  }
  return out.sort((a, b) => a.at - b.at).slice(0, MAX_REMINDERS);
}
