// Quick text: a global shortcut brings up a small "text someone" composer from any app. Pick a
// person (lib/recipients, the same list as New message), type, and Enter sends through the iPhone
// exactly as the Messages view does; then tug gets out of the way and the app the person was in
// has the keyboard again (the backend, src-tauri/src/quick_text/, remembers which). Esc does the
// same without sending.
//
// Off until switched on in Settings › Messages: a global shortcut takes its keys from every other
// app. This module holds the decisions (which shortcuts, what happens after a send); the store and
// QuickText.vue do the I/O.
//
// Pure: no Tauri, no DOM.

import type { CodeHotkeyStatus } from "../types/protocol";
import { hotkeyAlternatives, hotkeyKeys, knownHotkey } from "./codeFill";
import hotkeyList from "./quickTextHotkeys.json";

/** The shortcuts on offer, in the order Settings lists them; the first is the default. */
export const QUICK_TEXT_HOTKEYS: readonly string[] = hotkeyList.hotkeys;
export const DEFAULT_QUICK_TEXT_HOTKEY = QUICK_TEXT_HOTKEYS[0];

/** A saved shortcut, or the default if it isn't one quick text offers. */
export function knownQuickTextHotkey(id: string | null | undefined): string {
  return knownHotkey(id, QUICK_TEXT_HOTKEYS);
}

/** The other shortcuts to offer when this one is taken by another app. */
export function quickTextAlternatives(current: string): string[] {
  return hotkeyAlternatives(current, QUICK_TEXT_HOTKEYS);
}

/**
 * The quick text shortcut's line in Settings › General › Keyboard: the keys actually registered
 * right now. None while it's off or another app holds the keys.
 */
export function quickTextShortcut(status: CodeHotkeyStatus): { what: string; keys: string[] } | null {
  if (status.state !== "on") return null;
  return { what: "Text someone (from any app)", keys: hotkeyKeys(status.keys) };
}

/** What `tug.sendMessage` said about a send. */
export type SendResult = "sent" | "unconfirmed" | "failed" | "error";

/**
 * What the composer does once a send is done:
 * - "giveBack": the phone took it; close, and the app the person was in gets the keyboard back.
 * - "showConversation": it's recorded but not known to be sent ("May have sent", or "Not sent"
 *   with Retry); close and open the conversation in tug, which says so, rather than hide it.
 * - "keep": nothing was recorded (tug has said why); stay open with the text, so nothing is lost.
 */
export function afterQuickSend(result: SendResult): "giveBack" | "showConversation" | "keep" {
  switch (result) {
    case "sent":
      return "giveBack";
    case "unconfirmed":
    case "failed":
      return "showConversation";
    case "error":
      return "keep";
  }
}

/** Whether Enter or Send may send now: someone picked, something typed, texts up, not already sending. */
export function canQuickSend(o: { to: string | null; text: string; unavailable: string | null; sending: boolean }): boolean {
  return o.to !== null && o.text.trim() !== "" && o.unavailable === null && !o.sending;
}
