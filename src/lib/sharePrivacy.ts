// Hide previews while you share your screen or present: what the window shows. The backend
// (src-tauri/src/share_privacy/) decides whether the screen is being shared or presented, and
// hides pop-up text itself; this decides the window's side (blurred texts and names, the banner,
// Show anyway) and the pop-up kind the backend words a hidden pop-up by.

import type { PhoneNotification, ShareReason, ShareState, ToastKind } from "../types/protocol";
import { isConversation } from "./format";

export const NOT_SHARING: ShareState = { active: false, reason: null };

/** Whether the window hides texts and names right now. "Show anyway" lasts until sharing stops. */
export function previewsHidden(enabled: boolean, share: ShareState, revealed: boolean): boolean {
  return enabled && share.active && !revealed;
}

/**
 * "Show anyway" is for this share only: once sharing stops it's forgotten, so the next share hides
 * previews again. Returns whether previews stay revealed after the state changed.
 */
export function stillRevealed(revealed: boolean, share: ShareState): boolean {
  return revealed && share.active;
}

const WHILE: Record<Exclude<ShareReason, "unknown">, string> = {
  sharing: "while you share your screen",
  presenting: "while you present",
  fullScreen: "while an app is full screen",
};

/** The banner: why previews are hidden (or showing, after Show anyway). Null when not sharing. */
export function shareBanner(enabled: boolean, share: ShareState, revealed: boolean): string | null {
  if (!enabled || !share.active) return null;
  // tug couldn't check the screen (Windows didn't answer): it hides to be safe, and says so.
  if (share.reason === "unknown") {
    return revealed ? "Previews showing, though tug can't check your screen right now" : "Previews hidden until tug can check your screen";
  }
  const why = WHILE[share.reason ?? "sharing"];
  return revealed ? `Previews showing ${why}` : `Previews hidden ${why}`;
}

/** What a pop-up for this notification is, for its wording while previews are hidden. */
export function toastKind(n: PhoneNotification): ToastKind {
  if (n.category === "incomingCall") return "call";
  if (n.category === "missedCall") return "missedCall";
  return isConversation(n) ? "message" : "notification";
}

/** The incoming-call card's caller line: their name, or nothing personal while previews are hidden. */
export function callerLine(name: string, hidden: boolean): string {
  return hidden ? "Incoming call" : name;
}
