// The Windows pop-up policy, pure so it can be unit-tested: given what's happening and the user's
// settings, should this notification pop up? Everything side-effecting (Windows permission, the
// rate limiter, the code de-dupe) stays in the store; this decides the policy only.
//
// The rules, in order:
//   • Windows pop-ups off       → nothing pops up.
//   • An incoming call          → rings through quiet hours and DND; held only if the user mutes
//                                 calls explicitly, and even then a VIP rings.
//   • The app is muted          → held (muting an app wins, even for a VIP).
//   • A VIP (text/call)         → pops up even during quiet hours, DND or Focus mode.
//   • Quiet hours or DND active → held.
//   • Focused (Focus mode)      → held for the summary at the end of the focus, except a
//                                 verification code: it's needed now (and code fill copies it
//                                 whatever the pop-up does).
//   • Otherwise                 → pops up.

import type { QuietHours, UiSettings } from "../types/protocol";

/** "HH:MM" → minutes since midnight, or null if it doesn't parse. */
function minutesOfDay(hhmm: string): number | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(hhmm.trim());
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  if (h > 23 || min > 59) return null;
  return h * 60 + min;
}

/**
 * Whether `now` falls inside the quiet-hours schedule. A window that ends before it starts runs
 * overnight: its evening part belongs to the day it starts on, and its morning part (after
 * midnight) belongs to that same chosen day — so "Mon 22:00–07:00" covers Mon night into Tue
 * morning. An empty day list means every day.
 */
export function inQuietHours(q: QuietHours, now: Date): boolean {
  if (!q.enabled) return false;
  const start = minutesOfDay(q.start);
  const end = minutesOfDay(q.end);
  if (start === null || end === null || start === end) return false;
  const day = now.getDay();
  const mins = now.getHours() * 60 + now.getMinutes();
  const runs = (d: number) => q.days.length === 0 || q.days.includes(d);
  if (start < end) {
    // Same-day window, e.g. 09:00–17:00.
    return runs(day) && mins >= start && mins < end;
  }
  // Overnight window: evening on the chosen day, morning on the day after it.
  const prevDay = (day + 6) % 7;
  return (runs(day) && mins >= start) || (runs(prevDay) && mins < end);
}

/** What the store knows about one pop-up candidate, distilled to what the policy needs. */
export interface PopupEvent {
  /** The notification's app bundle id (Messages for a text), for per-app mute. */
  appId: string;
  /** A call ringing on the phone right now. */
  isCall: boolean;
  /** The sender (texter or caller) is on the "always let through" list. */
  isVip: boolean;
  /** It carries a one-time code: time-sensitive, so Focus mode lets it through. */
  isCode?: boolean;
}

/**
 * What happens to a pop-up: it shows, it's held (by a switch the person set), or it's held for
 * Focus mode's summary, which counts only those.
 */
export type PopupDecision = "show" | "held" | "focus";

/**
 * What this notification or message's pop-up does, by the settings and whether Focus mode says
 * the person is focused right now. `now` is the local time to judge quiet hours against. See the
 * rules at the top of the file.
 */
export function popupDecision(event: PopupEvent, settings: UiSettings, now: Date, focused = false): PopupDecision {
  if (!settings.toasts) return "held";
  if (event.isCall) {
    // Calls ring through quiet hours, DND and Focus mode; only an explicit call mute silences
    // them, and a VIP rings even then.
    return event.isVip || !settings.muteCalls ? "show" : "held";
  }
  if (settings.mutedApps.includes(event.appId)) return "held";
  if (event.isVip) return "show";
  if (settings.doNotDisturb || inQuietHours(settings.quietHours, now)) return "held";
  // After DND and quiet hours: what they hold was never going to pop up, so the summary leaves it out.
  if (focused && settings.focus.enabled && !event.isCode) return "focus";
  return "show";
}

/** Should this notification or message raise a Windows pop-up right now? (`popupDecision` says why not.) */
export function shouldPopUp(event: PopupEvent, settings: UiSettings, now: Date, focused = false): boolean {
  return popupDecision(event, settings, now, focused) === "show";
}
