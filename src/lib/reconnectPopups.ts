// Pop-ups for what arrived while the iPhone was reconnecting.
//
// After a link outage iOS replays everything still on the phone and flags all of it pre-existing,
// including notifications that arrived during the gap. tug skips pre-existing ones (they'd be
// backlog), so a text that came in during a reconnect never popped up. The backend now says whether
// an event's row is new to tug (`fresh`): a replayed notification that is new, posted after the
// link was lost and recent, is one the person hasn't been told about. A handful pop up one by one;
// more than that become one summary pop-up.

import type { PhoneNotification } from "../types/protocol";
import { notificationTime } from "./format";

/** Only gap arrivals this recent pop up; older ones are backlog by the time the link is back. */
export const GAP_POPUP_WINDOW_MS = 10 * 60 * 1000;
/** The phone's clock and the PC's can disagree a little; allow this much before the outage. */
export const GAP_CLOCK_SLACK_MS = 60 * 1000;
/** More than this many gap arrivals become one summary pop-up instead of one each. */
export const GAP_POPUP_MAX = 3;
/** Replays arrive one event at a time; collect them for this long before deciding. */
export const GAP_COLLECT_MS = 1500;

/**
 * A replayed (pre-existing) notification that arrived while the link was down: new to tug, posted
 * after the link was lost (`lostAt`, null when no outage was seen this launch) and recent.
 */
export function arrivedDuringGap(n: PhoneNotification, lostAt: number | null, now: number): boolean {
  if (!n.flags.preExisting || !n.fresh || lostAt === null) return false;
  const posted = notificationTime(n).getTime();
  return posted >= lostAt - GAP_CLOCK_SLACK_MS && now - posted <= GAP_POPUP_WINDOW_MS;
}

export type GapPlan = { kind: "each"; items: PhoneNotification[] } | { kind: "summary"; count: number };

/** One pop-up each for a few, a single summary for more. */
export function planGapPopups(items: PhoneNotification[]): GapPlan {
  return items.length > GAP_POPUP_MAX ? { kind: "summary", count: items.length } : { kind: "each", items };
}

/** The summary pop-up's text (end-user copy). */
export function gapSummaryText(count: number): string {
  return `${count} notification${count === 1 ? "" : "s"} arrived while your iPhone was reconnecting`;
}
