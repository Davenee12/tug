// Play iPhone audio on this PC: mirrors the Rust status (src-tauri/src/pc_audio) and passes the
// person's clicks back. The words come from lib/pcAudio.

import { defineStore } from "pinia";
import { computed, ref } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, errorMessage, on } from "../lib/ipc";
import { newPcAudioProblem, pcAudioProblemText, pcAudioView } from "../lib/pcAudio";
import type { PcAudioStatus } from "../types/protocol";
import { useTugStore } from "./tug";

export const usePcAudioStore = defineStore("pcAudio", () => {
  const status = ref<PcAudioStatus | null>(null);
  const tug = useTugStore();
  const view = computed(() => pcAudioView(status.value, tug.connected));

  let teardown: UnlistenFn[] = [];
  let started = false;

  function apply(next: PcAudioStatus) {
    const problem = newPcAudioProblem(status.value, next);
    status.value = next;
    if (problem) {
      const label = problem === "dropped" ? "Reconnect" : "Try again";
      tug.notify("error", pcAudioProblemText(problem), tug.connected ? { label, run: () => void set(true) } : undefined);
    }
  }

  async function init() {
    if (started) return;
    started = true;
    teardown.push(await on("pc-audio", apply));
    try {
      apply(await api.pcAudioStatus());
    } catch {
      /* the next event will tell */
    }
  }

  function dispose() {
    for (const off of teardown) off();
    teardown = [];
    started = false;
  }

  async function set(on: boolean) {
    try {
      apply(await api.pcAudioSet(on));
    } catch (e) {
      tug.notify("error", errorMessage(e));
    }
  }

  /** What the button does now: turn on (or retry/reconnect), or stop. */
  function press() {
    const action = view.value.action;
    if (action) void set(action === "start");
  }

  async function setAuto(on: boolean) {
    try {
      apply(await api.pcAudioSetAuto(on));
    } catch (e) {
      tug.notify("error", errorMessage(e));
    }
  }

  return { status, view, init, dispose, set, press, setAuto };
});
