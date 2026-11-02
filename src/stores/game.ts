// tug's games' tiny store: which game screen is open (the games picker, Tugboat Run, Harbor Stack
// or Desk Derby), and the scores kept on this PC only, in tug's settings. For each game: the Free
// play best, the daily's streak and best per day, and the best in a two-player game (kept apart
// from the solo ones). Desk Derby's score is a race time in milliseconds, where lower is better and
// 0 means no time yet (`LOWER_IS_BETTER`), and it also keeps a Free play best per track. Nothing
// leaves the PC. Everything that ticks lives in the game's component and exists
// only while it's open; this store holds no timers and listens to nothing.

import { defineStore } from "pinia";
import { computed, reactive, ref } from "vue";
import { api } from "../lib/ipc";
import { EMPTY_DAILY, finishDaily, parseDaily, type DailyRecord } from "../lib/tugboatDaily";
import type { GameId } from "../types/protocol";

/** Where each game's scores are saved (Tugboat Run's keys are the ones it always had). */
export const SCORE_KEYS: Record<GameId, { best: string; daily: string; duo: string }> = {
  "tugboat-run": { best: "ui.tugboatRunBest", daily: "ui.tugboatRunDaily", duo: "ui.tugboatRunDuoBest" },
  "harbor-stack": { best: "ui.harborStackBest", daily: "ui.harborStackDaily", duo: "ui.harborStackDuoBest" },
  "desk-derby": { best: "ui.deskDerbyBest", daily: "ui.deskDerbyDaily", duo: "ui.deskDerbyDuoBest" },
};
/** Desk Derby's Free play best on each track (`{"inbox": 92310, …}`, milliseconds). */
export const DERBY_TRACKS_KEY = "ui.deskDerbyTracks";

export const GAMES: GameId[] = ["tugboat-run", "harbor-stack", "desk-derby"];

/** Games scored by time: the lowest wins, and 0 is "no time yet", never a best. */
export const LOWER_IS_BETTER: ReadonlySet<GameId> = new Set<GameId>(["desk-derby"]);

/** Whether `score` beats `best` in `game` (a higher score, or for a time a lower one). */
export function beats(game: GameId, score: number, best: number): boolean {
  if (LOWER_IS_BETTER.has(game)) return score > 0 && (best <= 0 || score < best);
  return score > best;
}

/** The better of two scores in `game`. */
export function better(game: GameId, a: number, b: number): number {
  return beats(game, a, b) ? a : b;
}

/** Desk Derby's saved per-track bests, whatever shape they're in (anything damaged is dropped). */
export function parseTrackBests(raw: string | undefined): Record<string, number> {
  try {
    const j = JSON.parse(raw ?? "null") as unknown;
    if (!j || typeof j !== "object" || Array.isArray(j)) return {};
    const out: Record<string, number> = {};
    for (const [k, v] of Object.entries(j)) {
      const n = parseBest(String(v));
      if (/^[a-z-]{1,24}$/.test(k) && n > 0) out[k] = n;
    }
    return out;
  } catch {
    return {};
  }
}

export interface Scores {
  best: number;
  daily: DailyRecord;
  duoBest: number;
}

/** A saved best, whatever shape it's in: a whole number of at least 0, or 0. */
export function parseBest(raw: string | undefined): number {
  const n = Number(raw);
  return Number.isFinite(n) && n > 0 ? Math.floor(n) : 0;
}

export const useGameStore = defineStore("game", () => {
  /** What's open over the window: nothing, the games picker, or a game. */
  const screen = ref<null | "picker" | GameId>(null);
  const open = computed(() => screen.value !== null);
  const scores = reactive<Record<GameId, Scores>>({
    "tugboat-run": { best: 0, daily: EMPTY_DAILY, duoBest: 0 },
    "harbor-stack": { best: 0, daily: EMPTY_DAILY, duoBest: 0 },
    "desk-derby": { best: 0, daily: EMPTY_DAILY, duoBest: 0 },
  });
  /** Desk Derby's Free play best on each track (milliseconds). */
  const trackBests = reactive<Record<string, number>>({});
  let loaded = false;
  /** A daily finished before the saved record arrived: keep what this session knows. */
  const dailyTouched: Record<GameId, boolean> = { "tugboat-run": false, "harbor-stack": false, "desk-derby": false };

  async function load() {
    if (loaded) return;
    try {
      const raw = await api.getSettings();
      for (const g of GAMES) {
        const keys = SCORE_KEYS[g];
        const s = scores[g];
        // A better score set while this was loading wins.
        s.best = better(g, s.best, parseBest(raw[keys.best]));
        s.duoBest = better(g, s.duoBest, parseBest(raw[keys.duo]));
        if (!dailyTouched[g]) s.daily = parseDaily(raw[keys.daily]);
      }
      for (const [track, ms] of Object.entries(parseTrackBests(raw[DERBY_TRACKS_KEY]))) {
        trackBests[track] = better("desk-derby", trackBests[track] ?? 0, ms);
      }
      loaded = true;
    } catch {
      /* nothing saved yet */
    }
  }

  /**
   * Open a game, or the picker to choose one (the Play button, the taskbar's Play a game). The
   * picker never replaces a game that's open: a taskbar "Play a game" mid-game would otherwise
   * throw the game away.
   */
  async function show(which: GameId | "picker" = "picker") {
    if (which === "picker" && screen.value !== null) return;
    screen.value = which;
    await load();
  }

  function close() {
    screen.value = null;
  }

  /**
   * Record a finished Free play game; true if it's a new best. For Desk Derby, `track` also keeps
   * the best on that track, and then "new best" means the track's.
   */
  function finish(score: number, game: GameId = "tugboat-run", track?: string): boolean {
    const s = scores[game];
    let newBest = false;
    if (beats(game, score, s.best)) {
      s.best = Math.floor(score);
      newBest = true;
      void api.setSetting(SCORE_KEYS[game].best, String(s.best)).catch(() => undefined);
    }
    if (track === undefined) return newBest;
    if (!beats(game, score, trackBests[track] ?? 0)) return false;
    trackBests[track] = Math.floor(score);
    void api.setSetting(DERBY_TRACKS_KEY, JSON.stringify(trackBests)).catch(() => undefined);
    return true;
  }

  /** Record a finished daily for the day it was; true if it's that day's new best. */
  function finishDay(date: string, score: number, game: GameId = "tugboat-run"): boolean {
    const { record, newBest } = finishDaily(scores[game].daily, date, score, LOWER_IS_BETTER.has(game));
    scores[game].daily = record;
    dailyTouched[game] = true;
    void api.setSetting(SCORE_KEYS[game].daily, JSON.stringify(record)).catch(() => undefined);
    return newBest;
  }

  /** Record a finished two-player game (its better score); true if it's a new best. */
  function finishDuo(pair: readonly [number, number], game: GameId = "tugboat-run"): boolean {
    const s = scores[game];
    const top = Math.floor(better(game, pair[0], pair[1]));
    if (!beats(game, top, s.duoBest)) return false;
    s.duoBest = top;
    void api.setSetting(SCORE_KEYS[game].duo, String(top)).catch(() => undefined);
    return true;
  }

  // Tugboat Run's scores by their old names.
  const best = computed(() => scores["tugboat-run"].best);
  const daily = computed(() => scores["tugboat-run"].daily);
  const duoBest = computed(() => scores["tugboat-run"].duoBest);

  return { screen, open, scores, trackBests, best, daily, duoBest, show, close, finish, finishDay, finishDuo };
});
