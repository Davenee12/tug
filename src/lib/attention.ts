// Whether the person can actually see tug's window right now.
//
// Marking a conversation seen (which drops it from the unread count, the tray tooltip and the
// taskbar dot) must only happen when someone is looking at it. tug lives in the tray: with the
// Messages tab left open and the window hidden, every new text used to be marked seen the moment
// it arrived, so the unread cues never showed. Hidden, unfocused or covered by an overlay (search,
// the picker, the pairing prompt…) all count as not looking.

export interface WindowState {
  /** `document.visibilityState` */
  visibility: DocumentVisibilityState;
  /** `document.hasFocus()` */
  focused: boolean;
  /** Something covers the main view (the store's `overlayOpen`). */
  overlayOpen: boolean;
}

/** True only when the window is shown, focused and nothing covers the main view. */
export function canSeeWindow(w: WindowState): boolean {
  return w.visibility === "visible" && w.focused && !w.overlayOpen;
}
