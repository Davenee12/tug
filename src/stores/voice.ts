// Voice typing: mirrors the Rust status (src-tauri/src/voice) and passes the person's choices
// back. The words come from lib/voice.

import { defineStore } from "pinia";
import { computed, ref } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, errorMessage, on } from "../lib/ipc";
import { DEFAULT_VOICE_HOTKEY, knownVoiceHotkey, showReadyPrompt, voiceProblem, voiceStartProblem } from "../lib/voice";
import type { VoiceEngine, VoiceModelId, VoiceStatus } from "../types/protocol";
import { useTugStore } from "./tug";

export const useVoiceStore = defineStore("voice", () => {
  const status = ref<VoiceStatus | null>(null);
  const tug = useTugStore();
  const enabled = computed(() => status.value?.enabled ?? false);
  const keys = computed(() => status.value?.keys ?? DEFAULT_VOICE_HOTKEY);
  const problem = computed(() => voiceProblem(status.value));

  let teardown: UnlistenFn[] = [];
  let started = false;

  async function init() {
    if (started) return;
    started = true;
    teardown.push(await on("voice-status", (s) => (status.value = s)));
    try {
      status.value = await api.voiceStatus(false);
    } catch {
      /* the next event will tell */
    }
  }

  function dispose() {
    for (const off of teardown) off();
    teardown = [];
    started = false;
  }

  /** Settings opened: list the installed speech languages (and look at the microphone again). */
  async function refresh() {
    try {
      status.value = await api.voiceStatus(true);
    } catch {
      /* keep what we have */
    }
  }

  async function set(next: { enabled?: boolean; keys?: string; language?: string | null }) {
    const cur = status.value;
    try {
      status.value = await api.setVoiceTyping(
        next.enabled ?? cur?.enabled ?? false,
        knownVoiceHotkey(next.keys ?? cur?.keys),
        next.language !== undefined ? next.language : (cur?.language ?? null),
      );
    } catch (e) {
      tug.notify("error", errorMessage(e));
      // A fresh copy, so a switch or picker the person just changed shows the real setting again.
      if (cur) status.value = { ...cur };
      return null;
    }
    return status.value;
  }

  /** Ctrl+K: turn it on or off, and say so if it couldn't start. */
  async function toggleFromPalette(on: boolean) {
    const s = await set({ enabled: on });
    if (!s) return;
    const trouble = voiceStartProblem(s);
    if (trouble) {
      tug.notify("error", trouble, { label: "Open Settings", run: () => tug.openSettings("general") });
    } else {
      tug.notify("info", s.enabled ? "Voice typing is on" : "Voice typing is off");
    }
  }

  /** Run one of the engine/model commands, keeping the status it returns; errors become a notice. */
  async function act(call: () => Promise<VoiceStatus>) {
    try {
      status.value = await call();
    } catch (e) {
      tug.notify("error", errorMessage(e));
      if (status.value) status.value = { ...status.value };
    }
  }

  /** Pick an engine; a whisper.cpp engine's model starts downloading in the background if it isn't here. */
  const setEngine = (engine: VoiceEngine) => act(() => api.setVoiceEngine(engine));
  /** Download (or resume) a speech model; progress arrives as status events. */
  const downloadModel = (model: VoiceModelId) => act(() => api.voiceModelDownload(model));
  const cancelDownload = (model: VoiceModelId) => act(() => api.voiceModelCancel(model));
  /** Delete every downloaded speech model. */
  const deleteModels = () => act(() => api.voiceModelsDelete());
  const setSpokenLanguage = (language: string) => act(() => api.setVoiceSpokenLanguage(language));
  const setContactHints = (on: boolean) => act(() => api.setVoiceContactHints(on));

  /** The personal dictionary, as saved (loaded when Settings opens). */
  const dictionary = ref<string[]>([]);
  async function loadDictionary() {
    try {
      dictionary.value = await api.voiceDictionary();
    } catch {
      /* keep what we have */
    }
  }
  /** Save the dictionary; it comes back as saved (tidied, without duplicates). */
  async function saveDictionary(words: string[]) {
    try {
      dictionary.value = await api.setVoiceDictionary(words);
    } catch (e) {
      tug.notify("error", errorMessage(e));
    }
    return dictionary.value;
  }

  /** The "Get voice typing ready now?" card is on screen (until it's answered or closed). */
  const readyCardOpen = ref(false);
  /** Open the card if it's due (App.vue says when nothing else needs the window), and tell the
   * backend it was shown, so it never shows by itself again, whatever the answer. */
  function offerReadyCard() {
    if (readyCardOpen.value || !showReadyPrompt(status.value)) return;
    readyCardOpen.value = true;
    void act(() => api.voiceReadyPromptShown());
  }

  async function openFix() {
    const page = problem.value?.page;
    if (!page) return;
    try {
      await api.openWindowsSettings(page);
    } catch (e) {
      tug.notify("error", errorMessage(e));
    }
  }

  return {
    status,
    enabled,
    keys,
    problem,
    init,
    dispose,
    refresh,
    set,
    toggleFromPalette,
    setEngine,
    downloadModel,
    cancelDownload,
    deleteModels,
    setSpokenLanguage,
    setContactHints,
    dictionary,
    loadDictionary,
    saveDictionary,
    readyCardOpen,
    offerReadyCard,
    openFix,
  };
});
