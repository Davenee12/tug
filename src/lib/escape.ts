// Esc on the Settings page goes back to where you were, but only when nothing else took that
// keypress. Pop-ups (search, New message, pairing, a ringing call) close on Esc in their focus
// trap, which runs first (capture phase) and marks the event handled. By the time Settings'
// own listener saw it the pop-up was already gone, so one Esc used to close the pop-up AND
// Settings behind it: Settings seemed to open and close by itself.

/** Whether this keydown should close Settings. Pure, so it's tested. */
export function escClosesSettings(e: Pick<KeyboardEvent, "key" | "defaultPrevented">, covered: boolean): boolean {
  return e.key === "Escape" && !e.defaultPrevented && !covered;
}
