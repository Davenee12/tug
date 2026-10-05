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
