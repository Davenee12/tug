// The Windows pop-up policy, pure so it can be unit-tested: given what's happening and the user's
// settings, should this notification pop up? Everything side-effecting (Windows permission, the
// rate limiter, the code de-dupe) stays in the store; this decides the policy only.
//
// The rules, in order:
//   • Windows alerts off        → nothing pops up.
//   • An incoming call          → rings through quiet hours and DND; held only if the user mutes
//                                 calls explicitly, and even then a VIP rings.
//   • The app is muted          → held (muting an app wins, even for a VIP).
//   • A VIP (text/call)         → pops up even during quiet hours or DND.
//   • Quiet hours or DND active → held.
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
}

/**
 * Should this notification or message raise a Windows pop-up, by the settings alone? `now` is the
 * local time to judge quiet hours against. See the rules at the top of the file.
 */
export function shouldPopUp(event: PopupEvent, settings: UiSettings, now: Date): boolean {
  if (!settings.toasts) return false;
  if (event.isCall) {
    // Calls ring through quiet hours and DND; only an explicit call mute silences them, and a VIP
    // rings even then.
    return event.isVip || !settings.muteCalls;
  }
  if (settings.mutedApps.includes(event.appId)) return false;
  if (event.isVip) return true;
  if (settings.doNotDisturb || inQuietHours(settings.quietHours, now)) return false;
  return true;
}
