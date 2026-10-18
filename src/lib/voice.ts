// Voice typing: hold a shortcut, talk, let go, and tug types what you said wherever you're
// typing. This module holds the words and choices Settings and Ctrl+K show; the backend
// (src-tauri/src/voice/) owns the shortcut, the microphone, recognition and the keystrokes.
//
// Pure: no Tauri, no DOM.

import type { VoiceEngine, VoiceModel, VoiceStatus } from "../types/protocol";
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
  kind: "startFailed" | "micBlocked" | "unavailable" | "inUse";
  title: string;
  body: string;
  /** The Windows Settings page that fixes it, if one does. */
  page: "microphone" | "language" | null;
  button: string | null;
}

/** What's in the way, if anything. Only while it's switched on (off is off), except right after
 * turning it on failed: the switch went back off and this says why. */
export function voiceProblem(status: VoiceStatus | null): VoiceProblem | null {
  if (status && !status.enabled && status.startError) {
    return {
      kind: "startFailed",
      title: status.startError,
      body: "Voice typing is off. Try turning it on again.",
      page: null,
      button: null,
    };
  }
  if (!status?.enabled) return null;
  if (status.state === "unavailable") {
    const modelWouldDo = status.accurateSupported && status.engine === "accurate";
    return {
      kind: "unavailable",
      title: modelWouldDo ? "Voice typing needs its speech model" : "This PC has no speech language yet",
      body: modelWouldDo
        ? "Windows has no speech recognition on this PC, so voice typing needs the accurate engine's speech model. Download it above, or add a language with speech recognition in Windows Settings."
        : "Basic uses Windows' own speech recognition. Add a language with speech recognition in Windows Settings, then come back here.",
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
  if (!status.enabled && status.startError) return status.startError;
  if (!status.enabled || status.state === "on") return null;
  if (status.state === "unavailable") return "Voice typing needs its speech model or a Windows speech language. See Settings.";
  if (status.state === "inUse") return `${hotkeyLabel(status.keys)} is in use by another app. Pick another shortcut in Settings.`;
  return null;
}

// ---- The speech engine and its model ----

/** A size the way Windows shows it (1 MB = 1,048,576 bytes): "141 MB". */
export function sizeLabel(bytes: number): string {
  return `${Math.round(bytes / 1048576)} MB`;
}

/** How far a download is, 0 to 100 (whole percent, never 100 until it's all there). */
export function downloadPercent(model: VoiceModel): number {
  if (model.total <= 0) return 0;
  if (model.received >= model.total) return 100;
  return Math.min(99, Math.floor((model.received / model.total) * 100));
}

/** Whether a Windows language id ("409", "809") is English: the accurate engine's model is English-only. */
export function isEnglish(language: string | null): boolean {
  if (!language) return true;
  const id = Number.parseInt(language, 16);
  return Number.isFinite(id) && (id & 0x3ff) === 0x09;
}

/** One of the two engines, as Settings offers it. */
export interface EngineOption {
  id: VoiceEngine;
  label: string;
  detail: string;
}

/** The engine choices, with what each one costs. */
export function engineOptions(status: VoiceStatus): EngineOption[] {
  const ready = status.model.state === "ready";
  return [
    {
      id: "accurate",
      label: "Accurate",
      detail: ready
        ? "Best with code and short commands. English only."
        : `Downloads a ${sizeLabel(status.model.total)} speech model once. Best with code and short commands. English only.`,
    },
    { id: "basic", label: "Basic", detail: "Built into Windows, no download." },
  ];
}

/** Whether to offer the speech model by itself: the first time voice typing is on and Accurate could use it. */
export function showModelOffer(status: VoiceStatus | null): boolean {
  return Boolean(
    status?.enabled &&
      !status.modelOffered &&
      status.engine === "accurate" &&
      status.accurateSupported &&
      status.model.state === "missing" &&
      status.model.received === 0 &&
      isEnglish(status.language),
  );
}

/** The first-time offer's words. */
export function modelOfferText(status: VoiceStatus): { title: string; body: string; button: string } {
  return {
    title: "Make voice typing more accurate",
    body: `tug can download a ${sizeLabel(status.model.total)} speech model once. It understands code and short commands far better, and it runs on this PC: your voice never leaves it.`,
    button: `Download (${sizeLabel(status.model.total)})`,
  };
}

/** What Settings says under the engine choice, and the one thing to press, if any. */
export interface ModelLine {
  text: string;
  /** Show the download's progress bar. */
  progress: boolean;
  action: "download" | "resume" | "cancel" | null;
  actionLabel: string | null;
  /** It went wrong (shown as a problem, not a note). */
  error: boolean;
}

/** The engine's state in one line: downloading, paused, why Basic is listening, or nothing when Accurate is in use. */
export function modelLine(status: VoiceStatus): ModelLine | null {
  const m = status.model;
  const size = sizeLabel(m.total);
  if (m.state === "downloading") {
    const pct = downloadPercent(m);
    return {
      text: `Downloading the speech model… ${pct}% (${sizeLabel(m.received)} of ${size})`,
      progress: true,
      action: "cancel",
      actionLabel: "Cancel",
      error: false,
    };
  }
  if (status.engine === "basic") return null;
  if (!status.accurateSupported) {
    return {
      text: "This PC's processor can't run the accurate engine, so tug uses Basic.",
      progress: false,
      action: null,
      actionLabel: null,
      error: false,
    };
  }
  if (m.state === "ready") {
    if (!isEnglish(status.language) && status.activeEngine === "basic") {
      const name = languageName(status) ?? "this language";
      return {
        text: `Accurate understands English only, so tug uses Basic for ${name}.`,
        progress: false,
        action: null,
        actionLabel: null,
        error: false,
      };
    }
    return null;
  }
  if (m.error) {
    return { text: m.error, progress: false, action: m.received > 0 ? "resume" : "download", actionLabel: "Try again", error: true };
  }
  if (m.received > 0) {
    return {
      text: `The download paused at ${downloadPercent(m)}%. tug uses Basic until it's finished.`,
      progress: true,
      action: "resume",
      actionLabel: "Resume download",
      error: false,
    };
  }
  return {
    text: "tug uses Basic until the speech model is downloaded.",
    progress: false,
    action: "download",
    actionLabel: `Download (${size})`,
    error: false,
  };
}

/** The note when the microphone is a Bluetooth headset in its phone-call mode, or null. */
export function phoneMicNote(status: VoiceStatus | null): string | null {
  if (!status?.enabled || !status.phoneMic) return null;
  return "Your microphone is a phone-call headset, so accuracy will be lower. Your PC's own microphone or a USB one works better.";
}
