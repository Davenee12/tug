// Code fill: a newly arrived verification code goes straight onto the clipboard, and a global
// shortcut types the latest one into whatever box has focus. This module holds the decisions
// (which code, how fresh, never twice, what the pop-ups say, which shortcuts); the store does the
// I/O, and the backend (src-tauri/src/code_fill/) owns the private clipboard write, the
// 2-minute clear and the keystrokes.
//
// Pure: no Tauri, no DOM, no clock of its own (callers pass `now`).

import type { CodeFilled, ToastSpec } from "../types/protocol";
import hotkeyList from "./codeHotkeys.json";

/** A code older than this is never copied or typed (mirrors `MAX_AGE_MS` in code_fill/mod.rs). */
export const CODE_FILL_MAX_AGE_MS = 10 * 60 * 1000;
/** A text counts as a live arrival only this soon after it was received (not catch-up backlog). */
export const CODE_LIVE_MS = 2 * 60 * 1000;
/** How long an auto-copied code stays on the clipboard (the backend clears it). */
export const CODE_CLEAR_AFTER_MS = 2 * 60 * 1000;

/** The shortcuts on offer, in the order Settings lists them; the first is the default. */
export const CODE_HOTKEYS: readonly string[] = hotkeyList.hotkeys;
export const DEFAULT_CODE_HOTKEY = CODE_HOTKEYS[0];

/** A saved shortcut, or the default if it isn't one tug offers (an old or hand-edited value). */
export function knownHotkey(id: string | null | undefined): string {
  return id && CODE_HOTKEYS.includes(id) ? id : DEFAULT_CODE_HOTKEY;
}

/** "ctrl+shift+v" → ["Ctrl", "Shift", "V"], for <kbd> chips. */
export function hotkeyKeys(id: string): string[] {
  return id.split("+").map((k) => (k.length === 1 ? k.toUpperCase() : k.charAt(0).toUpperCase() + k.slice(1)));
}

/** "ctrl+shift+v" → "Ctrl+Shift+V", for sentences. */
export function hotkeyLabel(id: string): string {
  return hotkeyKeys(id).join("+");
}

/** The other shortcuts to offer when this one is taken by another app. */
export function hotkeyAlternatives(current: string): string[] {
  return CODE_HOTKEYS.filter((h) => h !== current);
}

export interface AutoCopyCandidate {
  /** The code `findCode` found, or null. Nothing else is ever copied. */
  code: string | null;
  /** When tug received the notification or text (unix ms). */
  receivedAt: number;
  now: number;
  /** A notification replayed after a reconnect, or one the phone flagged as already there. */
  replayed: boolean;
  /** Codes already copied, with when (see `rememberCopied`). */
  copied: ReadonlyMap<string, number>;
  /** How recent it must be to count as live (default: the 10-minute cap). */
  liveMs?: number;
}

/**
 * Whether a freshly arrived code goes on the clipboard: a code, live (not a replay), no older
 * than the live window or the 10-minute cap, and not already copied in the last 10 minutes (the
 * same code often arrives twice, as a notification and as a text, and copying it again would
 * clobber whatever the person copied since).
 */
export function shouldAutoCopy(c: AutoCopyCandidate): boolean {
  if (c.code === null || c.replayed) return false;
  const age = c.now - c.receivedAt;
  if (age > Math.min(c.liveMs ?? CODE_FILL_MAX_AGE_MS, CODE_FILL_MAX_AGE_MS)) return false;
  const last = c.copied.get(c.code);
  return last === undefined || c.now - last >= CODE_FILL_MAX_AGE_MS;
}

/** Record a copy, forgetting ones past the window so the map can't grow. */
export function rememberCopied(copied: Map<string, number>, code: string, now: number) {
  copied.set(code, now);
  for (const [c, at] of copied) if (now - at >= CODE_FILL_MAX_AGE_MS) copied.delete(c);
}

/** "482193 copied, from Chase". */
export function copiedTitle(code: string, sender: string): string {
  const from = sender.trim();
  return from ? `${code} copied, from ${from}` : `${code} copied`;
}

/** The usual pop-up for a code, reworded for a code that's already on the clipboard: no Copy button. */
export function copiedToastSpec(spec: ToastSpec, code: string, sender: string): ToastSpec {
  return { ...spec, title: copiedTitle(code, sender), code: null };
}

/** What the small pop-up after a press of the shortcut says. */
export function fillFeedback(e: CodeFilled, hotkey: string): { title: string; body: string } {
  switch (e.outcome) {
    case "typed":
      return { title: "Code typed", body: e.from ? `From ${e.from}` : "" };
    case "noCode":
      return { title: "No recent code", body: "Codes from the last 10 minutes can be typed." };
    case "noTarget":
      return { title: "Click where the code goes", body: `Then press ${hotkeyLabel(hotkey)} again.` };
    case "failed":
      return { title: "Couldn't type the code", body: "Copy it from tug instead." };
  }
}
