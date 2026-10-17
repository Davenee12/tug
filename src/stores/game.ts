// Tugboat Run's tiny store: whether the game is open, and the scores kept on this PC only, in
// tug's settings: the Free play best, the daily challenge's streak and best per day, and the best
// score in a two-player race (kept apart from the solo ones). Everything that ticks lives in
// TugboatRun.vue and exists only while it's open; this store holds no timers and listens to nothing.

import { defineStore } from "pinia";
import { ref } from "vue";
import { api } from "../lib/ipc";
import { EMPTY_DAILY, finishDaily, parseDaily, type DailyRecord } from "../lib/tugboatDaily";

const BEST_KEY = "ui.tugboatRunBest";
const DAILY_KEY = "ui.tugboatRunDaily";
const DUO_KEY = "ui.tugboatRunDuoBest";

export const useGameStore = defineStore("game", () => {
  const open = ref(false);
  const best = ref(0);
  const daily = ref<DailyRecord>(EMPTY_DAILY);
  const duoBest = ref(0);
  let loaded = false;
  /** A daily run finished before the saved record arrived: keep what this session knows. */
  let dailyTouched = false;

  async function show() {
    open.value = true;
    if (loaded) return;
    try {
      const raw = await api.getSettings();
      const n = Number(raw[BEST_KEY]);
      // A higher score set while this was loading wins.
      if (Number.isFinite(n) && n > best.value) best.value = Math.floor(n);
      const duo = Number(raw[DUO_KEY]);
      if (Number.isFinite(duo) && duo > duoBest.value) duoBest.value = Math.floor(duo);
      if (!dailyTouched) daily.value = parseDaily(raw[DAILY_KEY]);
      loaded = true;
    } catch {
      /* nothing saved yet */
    }
  }

  function close() {
    open.value = false;
  }

  /** Record a finished Free play run; true if it's a new best. */
  function finish(score: number): boolean {
    if (!(score > best.value)) return false;
    best.value = Math.floor(score);
    void api.setSetting(BEST_KEY, String(best.value)).catch(() => undefined);
    return true;
  }

  /** Record a finished daily run for the day its course was; true if it's that day's new best. */
  function finishDay(date: string, score: number): boolean {
    const { record, newBest } = finishDaily(daily.value, date, score);
    daily.value = record;
    dailyTouched = true;
    void api.setSetting(DAILY_KEY, JSON.stringify(record)).catch(() => undefined);
    return newBest;
  }

  /** Record a finished two-player race (its higher score); true if it's a new race best. */
  function finishDuo(scores: readonly [number, number]): boolean {
    const top = Math.floor(Math.max(scores[0], scores[1]));
    if (!(top > duoBest.value)) return false;
    duoBest.value = top;
    void api.setSetting(DUO_KEY, String(top)).catch(() => undefined);
    return true;
  }

  return { open, best, daily, duoBest, show, close, finish, finishDay, finishDuo };
});
