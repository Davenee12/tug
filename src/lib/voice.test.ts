import { describe, expect, it } from "vitest";
import {
  DEFAULT_VOICE_HOTKEY,
  MAX_DICTIONARY_WORDS,
  SPOKEN_LANGUAGES,
  VOICE_HOTKEYS,
  dictionaryEntry,
  downloadPercent,
  engineLabel,
  engineModelId,
  engineOptions,
  isEnglish,
  knownVoiceHotkey,
  languageName,
  modelLines,
  modelStatus,
  modelsDiskUsed,
  phoneMicNote,
  readyCardState,
  readyPromptText,
  showReadyPrompt,
  sizeLabel,
  spokenLanguageName,
  spokenLanguageNote,
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
    mostAccurateModel: { state: "missing", received: 0, total: MOST, error: null },
    spokenLanguage: "auto",
    contactHints: true,
    accurateSupported: true,
    readyPrompt: false,
    phoneMic: false,
    accurateProblem: null,
    ...over,
  };
}

const MODEL = 81_781_811;
const MOST = 874_188_075;
const READY = { state: "ready", received: MODEL, total: MODEL, error: null } as const;
const MOST_READY = { state: "ready", received: MOST, total: MOST, error: null } as const;

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
    // Basic picked with the model downloaded: switching back is the fix, and it says so.
    const basic = voiceProblem(status({ state: "unavailable", language: null, engine: "basic", model: READY }));
    expect(basic?.title).toBe("Basic needs a Windows speech language");
    expect(basic?.body).toContain("Switch to Accurate");
    const most = voiceProblem(status({ state: "unavailable", language: null, engine: "basic", mostAccurateModel: MOST_READY }));
    expect(most?.body).toContain("Switch to Most accurate");
    expect(voiceProblem(status({ state: "unavailable", language: null, engine: "mostAccurate" }))?.title).toBe(
      "Voice typing needs its speech model",
    );
    // Listening again (the backend settled it on): no problem shown.
    expect(voiceProblem(status({ state: "on", language: null, model: READY, activeEngine: "accurate" }))).toBeNull();
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
  it("offers Basic, Accurate and Most accurate, cheapest first, with what each costs", () => {
    const [basic, accurate, most] = engineOptions(status());
    expect(basic).toEqual({ id: "basic", label: "Basic", detail: "Built into Windows, no download." });
    expect(accurate.id).toBe("accurate");
    expect(accurate.detail).toContain("Downloads a 78 MB speech model once");
    expect(accurate.detail).toContain("English only");
    expect(most.id).toBe("mostAccurate");
    expect(most.label).toBe("Most accurate");
    expect(most.detail).toContain("Downloads a 834 MB speech model once");
    expect(most.detail).toContain("other languages");
    // Downloaded: no more talk of downloads.
    const ready = engineOptions(status({ model: READY, mostAccurateModel: MOST_READY }));
    expect(ready[1].detail).not.toContain("Downloads");
    expect(ready[2].detail).not.toContain("Downloads");
  });

  it("knows which model each engine runs", () => {
    expect(engineModelId("accurate")).toBe("accurate");
    expect(engineModelId("mostAccurate")).toBe("mostAccurate");
    expect(engineModelId("basic")).toBeNull();
    expect(modelStatus(status({ mostAccurateModel: MOST_READY }), "mostAccurate")).toBe(MOST_READY);
    expect(engineLabel("mostAccurate")).toBe("Most accurate");
  });

  it("names Most accurate's languages, detecting by default", () => {
    expect(SPOKEN_LANGUAGES[0]).toEqual({ code: "auto", name: "Detect automatically" });
    expect(spokenLanguageName("de")).toBe("German");
    expect(spokenLanguageName("auto")).toBe("Detect automatically");
    expect(spokenLanguageName("xx")).toBe("Detect automatically");
    expect(new Set(SPOKEN_LANGUAGES.map((l) => l.code)).size).toBe(SPOKEN_LANGUAGES.length);
    expect(spokenLanguageNote("auto")).toContain("takes longer");
    expect(spokenLanguageNote("de")).toContain("English only");
  });
});

describe("modelLines", () => {
  it("says Basic listens until the model is downloaded, with the download", () => {
    const [line] = modelLines(status());
    expect(line.text).toBe("tug uses Basic until the speech model is downloaded.");
    expect(line.action).toBe("download");
    expect(line.model).toBe("accurate");
    expect(line.actionLabel).toBe("Download (78 MB)");
  });

  it("shows progress with Cancel while downloading", () => {
    const lines = modelLines(status({ model: { ...READY, state: "downloading", received: MODEL / 4 } }));
    expect(lines).toHaveLength(1);
    expect(lines[0].text).toBe("Downloading the speech model… 25% (19 MB of 78 MB)");
    expect(lines[0]).toMatchObject({ progress: true, percent: 25, action: "cancel", model: "accurate" });
  });

  it("shows each download running, whichever engine is picked", () => {
    const lines = modelLines(
      status({
        engine: "mostAccurate",
        activeEngine: "basic",
        model: { ...READY, state: "downloading", received: MODEL / 2 },
        mostAccurateModel: { ...MOST_READY, state: "downloading", received: MOST / 10 },
      }),
    );
    expect(lines.map((l) => [l.model, l.action])).toEqual([
      ["accurate", "cancel"],
      ["mostAccurate", "cancel"],
    ]);
    expect(lines[1].text).toBe("Downloading Most accurate's speech model… 10% (83 MB of 834 MB)");
  });

  it("says what listens while Most accurate's model downloads", () => {
    const paused = modelLines(
      status({ engine: "mostAccurate", model: READY, activeEngine: "accurate", mostAccurateModel: { ...MOST_READY, state: "missing", received: MOST / 2 } }),
    );
    expect(paused[0].text).toBe("The download paused at 50%. tug uses Accurate until it's finished.");
    expect(paused[0].action).toBe("resume");
    const none = modelLines(status({ engine: "mostAccurate", activeEngine: "basic" }));
    expect(none[0].text).toBe("tug uses Basic until Most accurate's speech model is downloaded.");
    expect(none[0].actionLabel).toBe("Download (834 MB)");
  });

  it("offers to resume a paused download, and says what went wrong", () => {
    const [paused] = modelLines(status({ model: { ...READY, state: "missing", received: MODEL * 0.4 } }));
    expect(paused.text).toContain("paused at 40%");
    expect(paused.action).toBe("resume");
    const [failed] = modelLines(status({ model: { ...READY, state: "missing", received: MODEL * 0.6, error: "The download stopped." } }));
    expect(failed).toMatchObject({ text: "The download stopped.", error: true, action: "resume", actionLabel: "Try again" });
  });

  it("is quiet once the picked engine is listening, or when Basic was picked", () => {
    expect(modelLines(status({ model: READY, activeEngine: "accurate" }))).toEqual([]);
    expect(modelLines(status({ engine: "mostAccurate", mostAccurateModel: MOST_READY, activeEngine: "mostAccurate" }))).toEqual([]);
    expect(modelLines(status({ engine: "basic" }))).toEqual([]);
  });

  it("tells the truth about why something else is listening", () => {
    expect(modelLines(status({ accurateSupported: false }))[0].text).toBe(
      "This PC's processor can't run the accurate engine, so tug uses Basic.",
    );
    expect(modelLines(status({ engine: "mostAccurate", accurateSupported: false }))[0].text).toBe(
      "This PC's processor can't run the most accurate engine, so tug uses Basic.",
    );
    expect(modelLines(status({ model: READY, language: "407", activeEngine: "basic" }))[0].text).toBe(
      "Accurate understands English only, so tug uses Basic for German (Germany). Most accurate understands it.",
    );
  });

  it("says when the engine couldn't start, or memory was short, and what listened instead", () => {
    const [failed] = modelLines(status({ model: READY, accurateProblem: "failed", activeEngine: "basic" }));
    expect(failed.text).toContain("tug couldn't start the accurate engine on this PC, so it's using Basic");
    expect(failed.error).toBe(true);
    expect(failed.action).toBeNull();
    const [low] = modelLines(status({ model: READY, accurateProblem: "lowMemory", activeEngine: "accurate" }));
    expect(low.text).toBe("This PC was low on free memory, so tug used Basic for that one. Accurate tries again next time.");
    expect(low.error).toBe(false);
    const [fell] = modelLines(
      status({ engine: "mostAccurate", mostAccurateModel: MOST_READY, model: READY, accurateProblem: "lowMemoryUsedAccurate", activeEngine: "mostAccurate" }),
    );
    expect(fell.text).toBe("This PC was low on free memory, so tug used Accurate for that one. Most accurate tries again next time.");
  });

  it("still shows a running download after Basic is picked", () => {
    expect(modelLines(status({ engine: "basic", model: { ...READY, state: "downloading", received: 1 } }))[0].action).toBe("cancel");
  });
});

describe("disk use", () => {
  it("counts whole models and paused downloads", () => {
    expect(modelsDiskUsed(status())).toBe(0);
    expect(modelsDiskUsed(status({ model: READY }))).toBe(MODEL);
    expect(modelsDiskUsed(status({ model: READY, mostAccurateModel: { ...MOST_READY, state: "missing", received: 1000 } }))).toBe(MODEL + 1000);
    expect(sizeLabel(modelsDiskUsed(status({ model: READY, mostAccurateModel: MOST_READY })))).toBe("912 MB");
  });
});

describe("the one-time ready card", () => {
  const due = (over: Partial<VoiceStatus> = {}) => status({ enabled: false, state: "off", readyPrompt: true, ...over });

  it("shows only when the backend says it's due and there's something to download", () => {
    expect(showReadyPrompt(due())).toBe(true);
    expect(showReadyPrompt(due({ readyPrompt: false }))).toBe(false);
    expect(showReadyPrompt(due({ enabled: true, state: "on" }))).toBe(false);
    expect(showReadyPrompt(due({ accurateSupported: false }))).toBe(false);
    expect(showReadyPrompt(due({ engine: "basic" }))).toBe(false);
    expect(showReadyPrompt(due({ model: READY }))).toBe(false);
    expect(showReadyPrompt(due({ model: { ...READY, state: "downloading", received: 5 } }))).toBe(false);
    expect(showReadyPrompt(due({ model: { ...READY, state: "missing", received: 5 } }))).toBe(false);
    expect(showReadyPrompt(null)).toBe(false);
  });

  it("asks once, with the size, and follows the download through", () => {
    expect(readyPromptText(due())).toEqual({
      title: "Get voice typing ready now?",
      body: "Downloads the speech model once (78 MB). It runs on this PC: your voice never leaves it.",
    });
    expect(readyPromptText(due({ engine: "mostAccurate" })).body).toContain("(834 MB)");
    expect(readyCardState(due(), false).phase).toBe("offer");
    const going = readyCardState(due({ model: { ...READY, state: "downloading", received: MODEL / 2 } }), true);
    expect(going).toMatchObject({ phase: "downloading", percent: 50 });
    expect(readyCardState(due({ model: READY }), true).phase).toBe("ready");
    const failed = readyCardState(due({ model: { ...READY, state: "missing", received: 10, error: "The download stopped." } }), true);
    expect(failed).toEqual({ phase: "failed", text: "The download stopped.", percent: 0 });
  });
});

describe("personal dictionary", () => {
  it("adds a tidy word, and says why it can't", () => {
    expect(dictionaryEntry("  Zephyrine  ", [])).toEqual({ word: "Zephyrine" });
    expect(dictionaryEntry("two   words", [])).toEqual({ word: "two words" });
    expect(dictionaryEntry("", [])).toEqual({ problem: "Type a word or a name first." });
    expect(dictionaryEntry("x".repeat(41), [])).toEqual({ problem: "Use 40 characters or fewer." });
    expect(dictionaryEntry("1234", [])).toEqual({ problem: "Use at least one letter." });
    expect(dictionaryEntry("zephyrine", ["Zephyrine"])).toEqual({ problem: "That's already in your dictionary." });
    const full = Array.from({ length: MAX_DICTIONARY_WORDS }, (_, i) => `w${i}`);
    expect(dictionaryEntry("one more", full)).toEqual({ problem: "Your dictionary is full (100 words)." });
  });

  it("lets a word be edited, even to a new spelling of itself", () => {
    expect(dictionaryEntry("ZEPHYRINE", ["Zephyrine"], "Zephyrine")).toEqual({ word: "ZEPHYRINE" });
    expect(dictionaryEntry("Kubernetes", ["Zephyrine", "Kubernetes"], "Zephyrine")).toEqual({
      problem: "That's already in your dictionary.",
    });
    const full = Array.from({ length: MAX_DICTIONARY_WORDS }, (_, i) => `w${i}`);
    expect(dictionaryEntry("renamed", full, "w3")).toEqual({ word: "renamed" });
  });
});

describe("phoneMicNote", () => {
  it("notes a phone-call headset while voice typing is on", () => {
    expect(phoneMicNote(status({ phoneMic: true }))).toContain("phone-call headset");
    expect(phoneMicNote(status({ phoneMic: true, enabled: false, state: "off" }))).toBeNull();
    expect(phoneMicNote(status())).toBeNull();
  });
});
