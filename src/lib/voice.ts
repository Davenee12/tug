// Voice typing: hold a shortcut, talk, let go, and tug types what you said wherever you're
// typing. This module holds the words and choices Settings and Ctrl+K show; the backend
// (src-tauri/src/voice/) owns the shortcut, the microphone, recognition and the keystrokes.
//
// Pure: no Tauri, no DOM.

import type { VoiceStatus } from "../types/protocol";
import { hotkeyLabel } from "./codeFill";
import hotkeyList from "./voiceHotkeys.json";

/** The hold-to-talk shortcuts on offer, in Settings' order; the first is the default. */
export const VOICE_HOTKEYS: readonly string[] = hotkeyList.hotkeys;
export const DEFAULT_VOICE_HOTKEY = VOICE_HOTKEYS[0];

/** A saved shortcut, or the default if it isn't one tug offers. */
export function knownVoiceHotkey(id: string | null | undefined): string {
  return id && VOICE_HOTKEYS.includes(id) ? id : DEFAULT_VOICE_HOTKEY;
}

/** The other shortcuts to offer when this one is taken by another app. */
export function voiceHotkeyAlternatives(current: string): string[] {
  return VOICE_HOTKEYS.filter((h) => h !== current);
}

/** The line under the switch: what it does, with the shortcut in it. */
export function voiceDescription(keys: string): string {
  return `Hold ${hotkeyLabel(keys)}, say what you want to type, and let go. tug types it wherever you're typing: a code editor, a terminal, a browser. Your voice is recognized on this PC and never sent anywhere.`;
}

/** Something stopping voice typing that the person can fix, with where to fix it. */
export interface VoiceProblem {
  kind: "micBlocked" | "unavailable" | "inUse";
  title: string;
  body: string;
  /** The Windows Settings page that fixes it, if one does. */
  page: "microphone" | "language" | null;
  button: string | null;
}

/** What's in the way, if anything. Only while it's switched on: off is off. */
export function voiceProblem(status: VoiceStatus | null): VoiceProblem | null {
  if (!status?.enabled) return null;
  if (status.state === "unavailable") {
    return {
      kind: "unavailable",
      title: "This PC has no speech language yet",
      body: "Voice typing uses Windows' own speech recognition. Add a language with speech recognition in Windows Settings, then come back here.",
      page: "language",
      button: "Open language settings",
    };
  }
  if (status.micBlocked) {
    return {
      kind: "micBlocked",
      title: "Windows is blocking the microphone",
      body: "In Windows' privacy settings, turn on Microphone access and Let desktop apps access your microphone.",
      page: "microphone",
      button: "Open microphone settings",
    };
  }
  if (status.state === "inUse") {
    return {
      kind: "inUse",
      title: "That shortcut is in use by another app",
      body: "Pick another one:",
      page: null,
      button: null,
    };
  }
  return null;
}

/** The language picker's name for the one in use, or null when there's nothing to pick from. */
export function languageName(status: VoiceStatus | null): string | null {
  if (!status?.language) return null;
  return status.languages.find((l) => l.id === status.language)?.name ?? null;
}

/** The voice commands Settings lists, so people know what they can say. */
export const VOICE_COMMANDS: ReadonlyArray<{ say: string; types: string }> = [
  { say: "open paren · close paren", types: "( )" },
  { say: "dot · comma · semicolon", types: ". , ;" },
  { say: "equals · plus · arrow", types: "= + ->" },
  { say: "quote … quote", types: '"…"' },
  { say: "camel case user id", types: "userId" },
  { say: "snake case user id", types: "user_id" },
  { say: "new line", types: "line break" },
];

/** The message after turning it on from Ctrl+K when it couldn't start, or null when it's on. */
export function voiceStartProblem(status: VoiceStatus): string | null {
  if (!status.enabled || status.state === "on") return null;
  if (status.state === "unavailable") return "Voice typing needs a speech language in Windows. See Settings.";
  if (status.state === "inUse") return `${hotkeyLabel(status.keys)} is in use by another app. Pick another shortcut in Settings.`;
  return null;
}
