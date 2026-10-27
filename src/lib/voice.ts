// Voice typing: hold a shortcut, talk, let go, and tug types what you said wherever you're
// typing. This module holds the words and choices Settings and Ctrl+K show; the backend
// (src-tauri/src/voice/) owns the shortcut, the microphone, recognition and the keystrokes.
//
// Pure: no Tauri, no DOM.

import type { VoiceEngine, VoiceModel, VoiceModelId, VoiceStatus } from "../types/protocol";
import { hotkeyLabel } from "./codeFill";
import hotkeyList from "./voiceHotkeys.json";
import spokenLanguageList from "./voiceSpokenLanguages.json";

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
    const words = (title: string, body: string): VoiceProblem => ({
      kind: "unavailable",
      title,
      body,
      page: "language",
      button: "Open language settings",
    });
    const downloaded = status.model.state === "ready" || status.mostAccurateModel.state === "ready";
    if (status.accurateSupported && status.engine === "basic" && downloaded) {
      const which = status.mostAccurateModel.state === "ready" ? "Most accurate" : "Accurate";
      return words(
        "Basic needs a Windows speech language",
        `Windows has no speech recognition on this PC. Switch to ${which} above, which uses the speech model you downloaded, or add a language with speech recognition in Windows Settings.`,
      );
    }
    if (status.accurateSupported && status.engine !== "basic") {
      return words(
        "Voice typing needs its speech model",
        "Windows has no speech recognition on this PC, so voice typing needs a speech model. Download it above, or add a language with speech recognition in Windows Settings.",
      );
    }
    return words(
      "This PC has no speech language yet",
      "Basic uses Windows' own speech recognition. Add a language with speech recognition in Windows Settings, then come back here.",
    );
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

/** The speech model a whisper.cpp engine runs, or null for Basic. */
export function engineModelId(engine: VoiceEngine): VoiceModelId | null {
  return engine === "basic" ? null : engine;
}

/** A model's status. */
export function modelStatus(status: VoiceStatus, id: VoiceModelId): VoiceModel {
  return id === "accurate" ? status.model : status.mostAccurateModel;
}

/** An engine's name as Settings shows it. */
export function engineLabel(engine: VoiceEngine): string {
  return engine === "mostAccurate" ? "Most accurate" : engine === "accurate" ? "Accurate" : "Basic";
}

/** One of the engines, as Settings offers it. */
export interface EngineOption {
  id: VoiceEngine;
  label: string;
  detail: string;
}

/** The engine choices, with what each one costs, cheapest first. */
export function engineOptions(status: VoiceStatus): EngineOption[] {
  const download = (m: VoiceModel) => (m.state === "ready" ? "" : `Downloads a ${sizeLabel(m.total)} speech model once. `);
  return [
    { id: "basic", label: "Basic", detail: "Built into Windows, no download." },
    {
      id: "accurate",
      label: "Accurate",
      detail: `${download(status.model)}Best with code and short commands. English only.`,
    },
    {
      id: "mostAccurate",
      label: "Most accurate",
      detail: `${download(status.mostAccurateModel)}Understands other languages too. Much slower (several seconds a phrase) and needs more memory.`,
    },
  ];
}

/** What Settings says under the engine choice, and the one thing to press, if any. */
export interface ModelLine {
  /** The model it's about, for its button. */
  model: VoiceModelId;
  text: string;
  /** Show the download's progress bar. */
  progress: boolean;
  /** How far, 0 to 100, for the progress bar. */
  percent: number;
  action: "download" | "resume" | "cancel" | null;
  actionLabel: string | null;
  /** It went wrong (shown as a problem, not a note). */
  error: boolean;
}

function line(model: VoiceModelId, text: string, extra: Partial<ModelLine> = {}): ModelLine {
  return { model, text, progress: false, percent: 0, action: null, actionLabel: null, error: false, ...extra };
}

/** "the speech model", or for the second model, "Most accurate's speech model". */
function modelName(id: VoiceModelId): string {
  return id === "mostAccurate" ? "Most accurate's speech model" : "the speech model";
}

/** What listens instead while the picked engine can't: Accurate or Basic (the backend's decision). */
function standIn(status: VoiceStatus): string {
  return status.activeEngine === "accurate" ? "Accurate" : "Basic";
}

/** The engines' state, a line each: every download running (with Cancel), then the picked engine's problem if it has one (paused, failed, not downloaded, why something else is listening). Empty when the picked engine is simply listening. */
export function modelLines(status: VoiceStatus): ModelLine[] {
  const lines: ModelLine[] = [];
  for (const id of ["accurate", "mostAccurate"] as const) {
    const m = modelStatus(status, id);
    if (m.state !== "downloading") continue;
    const pct = downloadPercent(m);
    const name = modelName(id);
    lines.push(
      line(id, `Downloading ${name}… ${pct}% (${sizeLabel(m.received)} of ${sizeLabel(m.total)})`, {
        progress: true,
        percent: pct,
        action: "cancel",
        actionLabel: "Cancel",
      }),
    );
  }
  const picked = pickedLine(status);
  if (picked) lines.push(picked);
  return lines;
}

/** The picked engine's own line, or null when it's listening (or downloading: that line is above). */
function pickedLine(status: VoiceStatus): ModelLine | null {
  const id = engineModelId(status.engine);
  if (!id) return null;
  const m = modelStatus(status, id);
  const engine = engineLabel(status.engine).toLowerCase();
  if (m.state === "downloading") return null;
  if (!status.accurateSupported) {
    return line(id, `This PC's processor can't run the ${engine} engine, so tug uses Basic.`);
  }
  if (m.state === "ready") {
    if (status.accurateProblem === "failed") {
      return line(
        id,
        `tug couldn't start the ${engine} engine on this PC, so it's using Basic. Turning voice typing off and on tries again.`,
        { error: true },
      );
    }
    const again = `${engineLabel(status.engine)} tries again next time.`;
    if (status.accurateProblem === "lowMemory") {
      return line(id, `This PC was low on free memory, so tug used Basic for that one. ${again}`);
    }
    if (status.accurateProblem === "lowMemoryUsedAccurate") {
      return line(id, `This PC was low on free memory, so tug used Accurate for that one. ${again}`);
    }
    if (id === "accurate" && !isEnglish(status.language) && status.activeEngine === "basic") {
      const name = languageName(status) ?? "this language";
      return line(id, `Accurate understands English only, so tug uses Basic for ${name}. Most accurate understands it.`);
    }
    return null;
  }
  const until = standIn(status);
  if (m.error) {
    return line(id, m.error, { action: m.received > 0 ? "resume" : "download", actionLabel: "Try again", error: true });
  }
  if (m.received > 0) {
    const pct = downloadPercent(m);
    return line(id, `The download paused at ${pct}%. tug uses ${until} until it's finished.`, {
      progress: true,
      percent: pct,
      action: "resume",
      actionLabel: "Resume download",
    });
  }
  return line(id, `tug uses ${until} until ${modelName(id)} is downloaded.`, {
    action: "download",
    actionLabel: `Download (${sizeLabel(m.total)})`,
  });
}

/** The bytes the speech models take on this PC: whole models and paused downloads. */
export function modelsDiskUsed(status: VoiceStatus): number {
  return [status.model, status.mostAccurateModel].reduce((sum, m) => sum + (m.state === "ready" ? m.total : m.received), 0);
}

/** The one-time "Get voice typing ready now?" card: only when the backend says it's due (first launch after an install or update, voice typing never set up, never shown before), this PC can run the model, and it isn't already here or on its way. */
export function showReadyPrompt(status: VoiceStatus | null): boolean {
  if (!status?.readyPrompt || status.enabled || !status.accurateSupported) return false;
  const id = engineModelId(status.engine);
  if (!id) return false;
  const m = modelStatus(status, id);
  return m.state === "missing" && m.received === 0;
}

/** The card's words, with the size of the model it downloads (the picked engine's). */
export function readyPromptText(status: VoiceStatus): { title: string; body: string } {
  const id = engineModelId(status.engine) ?? "accurate";
  return {
    title: "Get voice typing ready now?",
    body: `Downloads the speech model once (${sizeLabel(modelStatus(status, id).total)}). It runs on this PC: your voice never leaves it.`,
  };
}

/** Where the card is once it's open: still asking, downloading (after Download), done, or stopped with a problem. */
export interface ReadyCardState {
  phase: "offer" | "downloading" | "ready" | "failed";
  text: string;
  percent: number;
}

/** The card's state from the status, once it's open. `started`: the person pressed Download on it. */
export function readyCardState(status: VoiceStatus, started: boolean): ReadyCardState {
  const id = engineModelId(status.engine) ?? "accurate";
  const m = modelStatus(status, id);
  if (m.state === "ready") {
    return { phase: "ready", text: "The speech model is ready. Turn voice typing on in Settings whenever you like.", percent: 100 };
  }
  if (m.state === "downloading") {
    const pct = downloadPercent(m);
    return { phase: "downloading", text: `Downloading… ${pct}% (${sizeLabel(m.received)} of ${sizeLabel(m.total)})`, percent: pct };
  }
  if (started && m.error) return { phase: "failed", text: m.error, percent: downloadPercent(m) };
  return { phase: "offer", text: readyPromptText(status).body, percent: 0 };
}

/** The languages Most accurate can listen in, "Detect automatically" first. */
export const SPOKEN_LANGUAGES: ReadonlyArray<{ code: string; name: string }> = spokenLanguageList.languages;

/** A spoken language's name ("auto" is "Detect automatically"). */
export function spokenLanguageName(code: string): string {
  return SPOKEN_LANGUAGES.find((l) => l.code === code)?.name ?? SPOKEN_LANGUAGES[0].name;
}

/** The line under Most accurate's language: detecting takes longer; spoken commands are English. */
export function spokenLanguageNote(code: string): string {
  if (code === "auto") return "Detecting the language takes longer for each phrase than picking one. Spoken commands work in English.";
  if (code === "en") return "Picking your language makes each phrase quicker than detecting it.";
  return "Spoken commands like “comma” and “camel case” work in English only.";
}

/** The longest personal dictionary entry, in characters (the backend's `vocab::MAX_ENTRY_CHARS`). */
export const MAX_DICTIONARY_ENTRY = 40;
/** The most entries (the backend's `vocab::MAX_WORDS`). */
export const MAX_DICTIONARY_WORDS = 100;

/** A word typed into the personal dictionary box, trimmed, or why it can't be added. */
export function dictionaryEntry(raw: string, current: readonly string[], replacing: string | null = null): { word: string } | { problem: string } {
  const word = raw.trim().replace(/\s+/g, " ");
  if (!word) return { problem: "Type a word or a name first." };
  if ([...word].length > MAX_DICTIONARY_ENTRY) return { problem: `Use ${MAX_DICTIONARY_ENTRY} characters or fewer.` };
  if (!/\p{L}/u.test(word)) return { problem: "Use at least one letter." };
  const key = word.toLowerCase();
  if (current.some((w) => w.toLowerCase() === key && w !== replacing)) return { problem: "That's already in your dictionary." };
  if (!replacing && current.length >= MAX_DICTIONARY_WORDS) return { problem: `Your dictionary is full (${MAX_DICTIONARY_WORDS} words).` };
  return { word };
}

/** The note when the microphone is a Bluetooth headset in its phone-call mode, or null. */
export function phoneMicNote(status: VoiceStatus | null): string | null {
  if (!status?.enabled || !status.phoneMic) return null;
  return "Your microphone is a phone-call headset, so accuracy will be lower. Your PC's own microphone or a USB one works better.";
}
