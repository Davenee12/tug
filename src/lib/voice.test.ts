import { describe, expect, it } from "vitest";
import {
  DEFAULT_VOICE_HOTKEY,
  VOICE_HOTKEYS,
  downloadPercent,
  engineOptions,
  isEnglish,
  knownVoiceHotkey,
  languageName,
  modelLine,
  modelOfferText,
  phoneMicNote,
  showModelOffer,
  sizeLabel,
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
    engine: "accurate",
    activeEngine: "basic",
    model: { state: "missing", received: 0, total: MODEL, error: null },
    accurateSupported: true,
    modelOffered: false,
    phoneMic: false,
    ...over,
  };
}

const MODEL = 81_781_811;
const READY = { state: "ready", received: MODEL, total: MODEL, error: null } as const;

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
    const p = voiceProblem(status({ state: "unavailable", language: null, languages: [], engine: "basic" }));
    expect(p?.kind).toBe("unavailable");
    expect(p?.page).toBe("language");
    expect(p?.title).toBe("This PC has no speech language yet");
  });

  it("points at the speech model when Accurate could listen instead", () => {
    const p = voiceProblem(status({ state: "unavailable", language: null, languages: [] }));
    expect(p?.title).toBe("Voice typing needs its speech model");
    expect(p?.body).toContain("Download it above");
    // A processor that can't run it: only Windows' speech can help.
    expect(voiceProblem(status({ state: "unavailable", language: null, accurateSupported: false }))?.title).toBe(
      "This PC has no speech language yet",
    );
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

describe("sizes and progress", () => {
  it("shows sizes the way Windows does", () => {
    expect(sizeLabel(MODEL)).toBe("78 MB");
    expect(sizeLabel(147_964_211)).toBe("141 MB");
    expect(sizeLabel(0)).toBe("0 MB");
  });

  it("never says 100% before it's all there", () => {
    expect(downloadPercent({ ...READY, state: "downloading", received: 0 })).toBe(0);
    expect(downloadPercent({ ...READY, state: "downloading", received: MODEL / 2 })).toBe(50);
    expect(downloadPercent({ ...READY, state: "downloading", received: MODEL - 1 })).toBe(99);
    expect(downloadPercent(READY)).toBe(100);
    expect(downloadPercent({ ...READY, total: 0 })).toBe(0);
  });

  it("knows English from its language id", () => {
    expect(isEnglish("409")).toBe(true);
    expect(isEnglish("809")).toBe(true);
    expect(isEnglish("407")).toBe(false);
    expect(isEnglish(null)).toBe(true);
  });
});

describe("engine choice", () => {
  it("offers Accurate with what it costs, and Basic", () => {
    const [accurate, basic] = engineOptions(status());
    expect(accurate.id).toBe("accurate");
    expect(accurate.detail).toContain("Downloads a 78 MB speech model once");
    expect(basic).toEqual({ id: "basic", label: "Basic", detail: "Built into Windows, no download." });
    expect(engineOptions(status({ model: READY }))[0].detail).not.toContain("Downloads");
  });

  it("offers the model once, the first time voice typing is on", () => {
    expect(showModelOffer(status())).toBe(true);
    expect(modelOfferText(status()).body).toContain("your voice never leaves it");
    expect(modelOfferText(status()).button).toBe("Download (78 MB)");
    expect(showModelOffer(status({ modelOffered: true }))).toBe(false);
    expect(showModelOffer(status({ enabled: false, state: "off" }))).toBe(false);
    expect(showModelOffer(status({ engine: "basic" }))).toBe(false);
    expect(showModelOffer(status({ accurateSupported: false }))).toBe(false);
    expect(showModelOffer(status({ language: "407" }))).toBe(false);
    expect(showModelOffer(status({ model: READY }))).toBe(false);
    expect(showModelOffer(status({ model: { ...READY, state: "downloading", received: 10 } }))).toBe(false);
    expect(showModelOffer(status({ model: { ...READY, state: "missing", received: 10 } }))).toBe(false);
    expect(showModelOffer(null)).toBe(false);
  });
});

describe("modelLine", () => {
  it("says Basic listens until the model is downloaded, with the download", () => {
    const line = modelLine(status());
    expect(line?.text).toBe("tug uses Basic until the speech model is downloaded.");
    expect(line?.action).toBe("download");
    expect(line?.actionLabel).toBe("Download (78 MB)");
  });

  it("shows progress with Cancel while downloading", () => {
    const line = modelLine(status({ model: { ...READY, state: "downloading", received: MODEL / 4 } }));
    expect(line?.text).toBe("Downloading the speech model… 25% (19 MB of 78 MB)");
    expect(line?.progress).toBe(true);
    expect(line?.action).toBe("cancel");
  });

  it("offers to resume a paused download, and says what went wrong", () => {
    const paused = modelLine(status({ model: { ...READY, state: "missing", received: MODEL * 0.4 } }));
    expect(paused?.text).toContain("paused at 40%");
    expect(paused?.action).toBe("resume");
    const failed = modelLine(
      status({ model: { ...READY, state: "missing", received: MODEL * 0.6, error: "The download stopped." } }),
    );
    expect(failed).toMatchObject({ text: "The download stopped.", error: true, action: "resume", actionLabel: "Try again" });
  });

  it("is quiet once Accurate is listening, or when Basic was picked", () => {
    expect(modelLine(status({ model: READY, activeEngine: "accurate" }))).toBeNull();
    expect(modelLine(status({ engine: "basic" }))).toBeNull();
  });

  it("tells the truth about why Basic is listening", () => {
    expect(modelLine(status({ accurateSupported: false }))?.text).toBe(
      "This PC's processor can't run the accurate engine, so tug uses Basic.",
    );
    expect(modelLine(status({ model: READY, language: "407", activeEngine: "basic" }))?.text).toBe(
      "Accurate understands English only, so tug uses Basic for German (Germany).",
    );
  });

  it("still shows a running download after Basic is picked", () => {
    expect(modelLine(status({ engine: "basic", model: { ...READY, state: "downloading", received: 1 } }))?.action).toBe(
      "cancel",
    );
  });
});

describe("phoneMicNote", () => {
  it("notes a phone-call headset while voice typing is on", () => {
    expect(phoneMicNote(status({ phoneMic: true }))).toContain("phone-call headset");
    expect(phoneMicNote(status({ phoneMic: true, enabled: false, state: "off" }))).toBeNull();
    expect(phoneMicNote(status())).toBeNull();
  });
});
