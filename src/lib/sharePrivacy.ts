// Hide previews while you share your screen or present: what the window shows. The backend
// (src-tauri/src/share_privacy/) decides whether the screen is being shared or presented, and
// hides pop-up text itself; this decides the window's side (blurred texts and names, the banner,
// Show anyway) and the pop-up kind the backend words a hidden pop-up by.

import type { PhoneNotification, ShareReason, ShareState, ToastKind } from "../types/protocol";
import { isConversation } from "./format";

export const NOT_SHARING: ShareState = { active: false, reason: null };
/** What the window assumes when the backend couldn't be asked: hidden, saying tug couldn't check. */
export const SHARE_UNKNOWN: ShareState = { active: true, reason: "unknown" };

/**
 * Whether the window hides texts and names right now. "Show anyway" lasts until sharing stops.
 * `awaiting`: the window just came on screen and the backend hasn't answered yet, so an old
 * "not sharing" can't flash texts in front of a meeting ({@link awaitingShareAnswer}).
 */
export function previewsHidden(enabled: boolean, share: ShareState, revealed: boolean, awaiting = false): boolean {
  return enabled && !revealed && (share.active || awaiting);
}

/**
 * Each time the window asks the backend to start (or stop) watching is an episode; only the answer
 * to the newest one counts as fresh, so a late answer from an earlier time on screen can't end
 * the wait.
 */
export interface ShareWatch {
  episode: number;
  answered: boolean;
}

export const SHARE_WATCH_START: ShareWatch = { episode: 0, answered: false };

/** The window asked again (it came on screen, went away, or the setting changed). */
export function shareWatchAsked(w: ShareWatch): ShareWatch {
  return { episode: w.episode + 1, answered: false };
}

/** The backend answered the ask numbered `episode`. */
export function shareWatchAnswered(w: ShareWatch, episode: number): ShareWatch {
  return episode === w.episode ? { ...w, answered: true } : w;
}

/** Previews count as hidden while the window is on screen with the setting on and no fresh answer yet. */
export function awaitingShareAnswer(w: ShareWatch, visible: boolean, enabled: boolean): boolean {
  return visible && enabled && !w.answered;
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
