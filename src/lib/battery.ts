// Low phone battery: a Windows pop-up when the iPhone drops to 20% and again at 10%, once per
// discharge. The phone only reports its level (not whether it's charging), so a climb back
// above RESET_AT counts as charged and re-arms the alerts.

export const LOW_BATTERY_LEVELS = [20, 10] as const;
const RESET_AT = 25;

/**
 * Given the newly reported level and the lowest level already alerted this discharge, which
 * alert (if any) to show now, and what to remember. Pure, so it's tested.
 */
export function batteryAlert(level: number | null, alerted: number | null): { alert: number | null; alerted: number | null } {
  if (level == null) return { alert: null, alerted };
  if (level >= RESET_AT) return { alert: null, alerted: null };
  // The lowest threshold reached that hasn't been announced yet (a jump from 30% to 8% says 10%).
  const due = [...LOW_BATTERY_LEVELS].reverse().find((l) => level <= l && (alerted == null || l < alerted));
  return due == null ? { alert: null, alerted } : { alert: due, alerted: due };
}

/** How long a remembered alert counts after a restart; older, it's forgotten (the phone may have charged meanwhile). */
export const ALERTED_KEEP_MS = 12 * 60 * 60 * 1000;

/**
 * The alert mark as saved in settings, so a restart of tug (or of the PC) mid-discharge doesn't
 * announce 20% again. Saved with when it was set; `null` once charged.
 */
export function saveBatteryAlerted(alerted: number | null, now: number): string {
  return JSON.stringify(alerted == null ? null : { level: alerted, at: now });
}

/** The saved mark, if it's recent and one of the alert levels; otherwise none. */
export function restoreBatteryAlerted(saved: string | undefined, now: number): number | null {
  if (!saved) return null;
  try {
    const v = JSON.parse(saved) as { level?: unknown; at?: unknown } | null;
    if (!v || typeof v.level !== "number" || typeof v.at !== "number") return null;
    if (!(LOW_BATTERY_LEVELS as readonly number[]).includes(v.level)) return null;
    if (now - v.at > ALERTED_KEEP_MS || v.at > now + 60_000) return null;
    return v.level;
  } catch {
    return null;
  }
}