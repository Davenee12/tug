// Keeping the window's texts in step with the backend's when an event may have been missed.
//
// Texts reach the window only as `message` events while it runs; a missed one stays missing until
// a restart. A reply sent from a Windows pop-up was reported as never showing in its conversation,
// although the backend stored and announced it like any other send and the conversation logic
// files it correctly (format.test.ts). So two cheap safety nets make sure the window *has* it: the
// pop-up press carries the stored reply, and coming back to the window after a while re-reads the
// newest texts. Both only add what's missing (by id) and never overwrite: a live event may already
// have moved a send further along (accepted → sent) than the copy a press or a re-read carries.
// Whatever they add is reported in the log, so a missed event shows up there.

import type { SmsMessage } from "../types/protocol";

/** Re-read the newest texts on coming back only after being away at least this long. */
export const RESYNC_AFTER_MS = 60_000;
/** How many of the newest texts a re-read compares (cheap; what a missed event could cover). */
export const RESYNC_LIMIT = 200;

/** The window's order for texts: oldest first by arrival, ties by id (as `list_messages` gives them). */
export function byArrival(a: SmsMessage, b: SmsMessage): number {
  return a.receivedAt - b.receivedAt || a.id - b.id;
}

/** The texts in `incoming` the window doesn't have yet (by id), each once. */
export function missingMessages(have: readonly SmsMessage[], incoming: readonly SmsMessage[]): SmsMessage[] {
  const known = new Set(have.map((m) => m.id));
  const out: SmsMessage[] = [];
  for (const m of incoming) {
    if (known.has(m.id)) continue;
    known.add(m.id);
    out.push(m);
  }
  return out;
}

/**
 * A text stored this recently may still have its `message` event on the way; a re-read leaves it
 * to that event, so only texts that really were missed get added (and reported as missed).
 */
export const EVENT_GRACE_MS = 5_000;

/** The texts in a re-read old enough that their event should already have arrived. */
export function settledMessages(list: readonly SmsMessage[], now: number, graceMs = EVENT_GRACE_MS): SmsMessage[] {
  return list.filter((m) => m.receivedAt <= now - graceMs);
}

/** Whether coming back to the window warrants a re-read: away (hidden or unfocused) long enough. */
export function shouldResync(awaySince: number | null, now: number, minAwayMs = RESYNC_AFTER_MS): boolean {
  return awaySince !== null && now - awaySince >= minAwayMs;
}
