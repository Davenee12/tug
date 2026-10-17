import { describe, expect, it } from "vitest";
import {
  DEFAULT_VOICE_HOTKEY,
  VOICE_HOTKEYS,
  knownVoiceHotkey,
  languageName,
  voiceDescription,
  voiceHotkeyAlternatives,
  voiceProblem,
  voiceStartProblem,
} from "./voice";
import { CODE_HOTKEYS, hotkeyLabel } from "./codeFill";
import shared from "./voiceHotkeys.json";
import type { VoiceStatus } from "../types/protocol";

function status(over: Partial<VoiceStatus> = {}): VoiceStatus {
  return {
    enabled: true,
    state: "on",
    keys: "ctrl+alt+space",
    language: "409",
    languages: [
      { id: "409", name: "English (US)" },
      { id: "407", name: "German (Germany)" },
    ],
    micBlocked: false,
    startError: null,
    ...over,
  };
}

const START_ERROR = "Voice typing couldn't start: Windows couldn't show the listening indicator.";

describe("voice shortcuts", () => {
  it("reads the list the backend registers from", () => {
    expect(VOICE_HOTKEYS).toEqual(shared.hotkeys);
    expect(DEFAULT_VOICE_HOTKEY).toBe("ctrl+alt+space");
  });

  it("never offers one of tug's own shortcuts or code fill's", () => {
    for (const h of VOICE_HOTKEYS) {
      expect(CODE_HOTKEYS).not.toContain(h);
      expect(["ctrl+k", "ctrl+n", "ctrl+shift+c", "ctrl+shift+v", "ctrl+,"]).not.toContain(h);
    }
  });

  it("falls back to the default for a shortcut it doesn't offer", () => {
    expect(knownVoiceHotkey("ctrl+alt+f12")).toBe("ctrl+alt+f12");
    expect(knownVoiceHotkey("ctrl+k")).toBe(DEFAULT_VOICE_HOTKEY);
    expect(knownVoiceHotkey(null)).toBe(DEFAULT_VOICE_HOTKEY);
  });

  it("offers the others when one is taken", () => {
    expect(voiceHotkeyAlternatives("ctrl+alt+space")).not.toContain("ctrl+alt+space");
    expect(voiceHotkeyAlternatives("ctrl+alt+space")).toHaveLength(VOICE_HOTKEYS.length - 1);
  });

  it("labels them for people", () => {
    expect(hotkeyLabel("ctrl+alt+space")).toBe("Ctrl+Alt+Space");
    expect(hotkeyLabel("ctrl+alt+f12")).toBe("Ctrl+Alt+F12");
    expect(voiceDescription("ctrl+alt+space")).toContain("Hold Ctrl+Alt+Space");
    expect(voiceDescription("ctrl+alt+space")).toContain("never sent anywhere");
  });
});

describe("voiceProblem", () => {
  it("is quiet while off, and while everything works", () => {
    expect(voiceProblem(null)).toBeNull();
    expect(voiceProblem(status({ enabled: false, state: "off", micBlocked: true }))).toBeNull();
    expect(voiceProblem(status())).toBeNull();
  });

  it("points a blocked microphone at Windows' privacy settings", () => {
    const p = voiceProblem(status({ micBlocked: true }));
    expect(p?.kind).toBe("micBlocked");
    expect(p?.title).toBe("Windows is blocking the microphone");
    expect(p?.page).toBe("microphone");
  });

  it("says when this PC has no speech language", () => {
    const p = voiceProblem(status({ state: "unavailable", language: null, languages: [] }));
    expect(p?.kind).toBe("unavailable");
    expect(p?.page).toBe("language");
  });

  it("says when another app holds the shortcut", () => {
    const p = voiceProblem(status({ state: "inUse" }));
    expect(p?.kind).toBe("inUse");
    expect(p?.page).toBeNull();
  });

  it("no language beats a blocked microphone (there's nothing to listen with)", () => {
    expect(voiceProblem(status({ state: "unavailable", micBlocked: true }))?.kind).toBe("unavailable");
  });

  it("says why it couldn't start, with the switch back off", () => {
    const p = voiceProblem(status({ enabled: false, state: "off", startError: START_ERROR }));
    expect(p?.kind).toBe("startFailed");
    expect(p?.title).toBe(START_ERROR);
    expect(p?.body).toContain("Try turning it on again");
    expect(p?.page).toBeNull();
    expect(p?.button).toBeNull();
  });

  it("forgets a failed start once it's on, and stays quiet when switched off on purpose", () => {
    expect(voiceProblem(status({ startError: START_ERROR }))).toBeNull();
    expect(voiceProblem(status({ enabled: false, state: "off" }))).toBeNull();
  });
});

describe("languageName", () => {
  it("names the language in use", () => {
    expect(languageName(status())).toBe("English (US)");
    expect(languageName(status({ language: "407" }))).toBe("German (Germany)");
    expect(languageName(status({ language: null }))).toBeNull();
    expect(languageName(null)).toBeNull();
  });
});

describe("voiceStartProblem", () => {
  it("is null once it's on (or off on purpose)", () => {
    expect(voiceStartProblem(status())).toBeNull();
    expect(voiceStartProblem(status({ enabled: false, state: "off" }))).toBeNull();
  });

  it("says what stopped it", () => {
    expect(voiceStartProblem(status({ state: "inUse" }))).toBe(
      "Ctrl+Alt+Space is in use by another app. Pick another shortcut in Settings.",
    );
    expect(voiceStartProblem(status({ state: "unavailable" }))).toContain("speech language");
  });

  it("says it couldn't start rather than 'Voice typing is off'", () => {
    expect(voiceStartProblem(status({ enabled: false, state: "off", startError: START_ERROR }))).toBe(START_ERROR);
  });
});
