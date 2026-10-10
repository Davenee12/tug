// Tugboat Run's tiny store: whether the game is open, and the best score (kept on this PC only,
// in tug's settings). Everything that ticks lives in TugboatRun.vue and exists only while it's
// open; this store holds no timers and listens to nothing.

import { defineStore } from "pinia";
import { ref } from "vue";
import { api } from "../lib/ipc";

const BEST_KEY = "ui.tugboatRunBest";

export const useGameStore = defineStore("game", () => {
  const open = ref(false);
  const best = ref(0);
  let bestLoaded = false;

  async function show() {
    open.value = true;
    if (bestLoaded) return;
    try {
      const raw = await api.getSettings();
      const n = Number(raw[BEST_KEY]);
      // A higher score set while this was loading wins.
      if (Number.isFinite(n) && n > best.value) best.value = Math.floor(n);
      bestLoaded = true;
    } catch {
      /* no best yet */
    }
  }

  function close() {
    open.value = false;
  }

  /** Record a finished run; true if it's a new best. */
  function finish(score: number): boolean {
    if (!(score > best.value)) return false;
    best.value = Math.floor(score);
    void api.setSetting(BEST_KEY, String(best.value)).catch(() => undefined);
    return true;
  }

  return { open, best, show, close, finish };
});
