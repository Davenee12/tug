// Voice typing: mirrors the Rust status (src-tauri/src/voice) and passes the person's choices
// back. The words come from lib/voice.

import { defineStore } from "pinia";
import { computed, ref } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, errorMessage, on } from "../lib/ipc";
import { DEFAULT_VOICE_HOTKEY, knownVoiceHotkey, voiceProblem, voiceStartProblem } from "../lib/voice";
import type { VoiceEngine, VoiceStatus } from "../types/protocol";
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

  /** Pick the accurate or the basic engine. */
  const setEngine = (engine: VoiceEngine) => act(() => api.setVoiceEngine(engine));
  /** Download (or resume) the speech model; progress arrives as status events. */
  const downloadModel = () => act(() => api.voiceModelDownload());
  const cancelDownload = () => act(() => api.voiceModelCancel());

  /** The first-time offer was answered: Download starts it, Not now just stops offering. */
  async function answerOffer(download: boolean) {
    await act(() => api.voiceModelOffered());
    if (download) await downloadModel();
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
    answerOffer,
    openFix,
  };
});
