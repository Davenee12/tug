import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";

// The games' scores live in tug's own settings on this PC, nowhere else.
const saved: Record<string, string> = {};
const setSetting = vi.fn(async (key: string, value: string) => {
  saved[key] = value;
});
vi.mock("../lib/ipc", () => ({
  api: {
    getSettings: vi.fn(async () => ({ ...saved })),
    setSetting: (key: string, value: string) => setSetting(key, value),
  },
}));

const { useGameStore, parseBest, parseTrackBests, beats, SCORE_KEYS, DERBY_TRACKS_KEY, DERBY_DUO_TRACKS_KEY, GAMES } =
  await import("./game");

/** Let the saves finish (a save waits for the saved scores to load first). */
const settle = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  for (const k of Object.keys(saved)) delete saved[k];
  setSetting.mockClear();
  setActivePinia(createPinia());
});

describe("the games' screens", () => {
  it("opens the picker by default, or a game by name, and closes", async () => {
    const game = useGameStore();
    expect(game.open).toBe(false);
    await game.show();
    expect(game.screen).toBe("picker");
    expect(game.open).toBe(true);
    await game.show("harbor-stack");
    expect(game.screen).toBe("harbor-stack");
    game.close();
    expect(game.open).toBe(false);
  });

  it("Play a game while a game is open keeps the game", async () => {
    const game = useGameStore();
    await game.show("tugboat-run");
    // The taskbar's Play a game (or the Play button) mid-race.
    await game.show("picker");
    expect(game.screen).toBe("tugboat-run");
    await game.show();
    expect(game.screen).toBe("tugboat-run");
    // Choosing a game by name still switches, and the picker opens once nothing is.
    await game.show("harbor-stack");
    expect(game.screen).toBe("harbor-stack");
    game.close();
    await game.show();
    expect(game.screen).toBe("picker");
  });
});

describe("scores", () => {
  it("keeps each game's best apart, saved under its own key", async () => {
    const game = useGameStore();
    await game.show("harbor-stack");
    expect(game.finish(14, "harbor-stack")).toBe(true);
    expect(game.finish(9, "harbor-stack")).toBe(false);
    expect(game.finish(300)).toBe(true); // Tugboat Run, by default
    await settle();
    expect(saved[SCORE_KEYS["harbor-stack"].best]).toBe("14");
    expect(saved["ui.tugboatRunBest"]).toBe("300");
    expect(game.scores["harbor-stack"].best).toBe(14);
    expect(game.best).toBe(300);
  });

  it("records the daily stack's best and streak per day", async () => {
    const game = useGameStore();
    await game.show("harbor-stack");
    expect(game.finishDay("2026-10-16", 8, "harbor-stack")).toBe(true);
    expect(game.finishDay("2026-10-17", 12, "harbor-stack")).toBe(true);
    expect(game.finishDay("2026-10-17", 10, "harbor-stack")).toBe(false);
    await settle();
    const rec = JSON.parse(saved["ui.harborStackDaily"]);
    expect(rec).toEqual({ streak: 2, last: "2026-10-17", bests: { "2026-10-16": 8, "2026-10-17": 12 } });
    // Tugboat Run's daily is untouched.
    expect(saved["ui.tugboatRunDaily"]).toBeUndefined();
    expect(game.daily.streak).toBe(0);
  });

  it("keeps the two-player best apart from the solo ones", async () => {
    const game = useGameStore();
    expect(game.finishDuo([7, 11], "harbor-stack")).toBe(true);
    expect(game.finishDuo([10, 3], "harbor-stack")).toBe(false);
    await settle();
    expect(saved["ui.harborStackDuoBest"]).toBe("11");
    expect(game.scores["harbor-stack"].best).toBe(0);
  });

  it("reads what was saved, and a damaged value as nothing", async () => {
    Object.assign(saved, {
      "ui.harborStackBest": "23",
      "ui.harborStackDuoBest": "-4",
      "ui.harborStackDaily": "{not json",
      "ui.tugboatRunBest": "512",
    });
    const game = useGameStore();
    await game.show();
    expect(game.scores["harbor-stack"].best).toBe(23);
    expect(game.scores["harbor-stack"].duoBest).toBe(0);
    expect(game.scores["harbor-stack"].daily.streak).toBe(0);
    expect(game.best).toBe(512);
    expect(parseBest("7.9")).toBe(7);
    expect(parseBest(undefined)).toBe(0);
    expect(parseBest("NaN")).toBe(0);
  });

  it("keeps a higher score set while the saved ones were loading", async () => {
    saved["ui.harborStackBest"] = "5";
    const game = useGameStore();
    const loading = game.show("harbor-stack");
    game.finish(9, "harbor-stack");
    await loading;
    expect(game.scores["harbor-stack"].best).toBe(9);
  });
});

describe("Desk Derby: a race time, where lower is better", () => {
  it("is one of the games, with its own keys", () => {
    expect(GAMES).toContain("desk-derby");
    expect(SCORE_KEYS["desk-derby"]).toEqual({ best: "ui.deskDerbyBest", daily: "ui.deskDerbyDaily", duo: "ui.deskDerbyDuoBest" });
    expect(new Set(GAMES.flatMap((g) => Object.values(SCORE_KEYS[g]))).size).toBe(GAMES.length * 3);
  });

  it("a faster time beats a slower one; no time (0) never beats anything; any time beats none", () => {
    expect(beats("desk-derby", 90_000, 95_000)).toBe(true);
    expect(beats("desk-derby", 96_000, 95_000)).toBe(false);
    expect(beats("desk-derby", 95_000, 0)).toBe(true);
    expect(beats("desk-derby", 0, 95_000)).toBe(false);
    expect(beats("desk-derby", 0, 0)).toBe(false);
    // The other games are unchanged: higher wins.
    expect(beats("harbor-stack", 12, 9)).toBe(true);
    expect(beats("tugboat-run", 9, 12)).toBe(false);
  });

  it("keeps the fastest Free play race, overall and on each track", async () => {
    const game = useGameStore();
    await game.show("desk-derby");
    expect(game.finish(95_120, "desk-derby", "inbox")).toBe(true);
    expect(game.finish(97_000, "desk-derby", "inbox")).toBe(false);
    expect(game.finish(91_500, "desk-derby", "inbox")).toBe(true);
    // A slower time on another track is still that track's first best (but not the overall best).
    expect(game.finish(104_000, "desk-derby", "coffee")).toBe(true);
    await settle();
    expect(game.scores["desk-derby"].best).toBe(91_500);
    expect(saved["ui.deskDerbyBest"]).toBe("91500");
    expect(JSON.parse(saved[DERBY_TRACKS_KEY])).toEqual({ inbox: 91_500, coffee: 104_000 });
    expect(game.trackBests).toEqual({ inbox: 91_500, coffee: 104_000 });
  });

  it("today's fastest race is the day's best, and the streak counts it", async () => {
    const game = useGameStore();
    expect(game.finishDay("2026-11-02", 93_000, "desk-derby")).toBe(true);
    expect(game.finishDay("2026-11-02", 96_000, "desk-derby")).toBe(false);
    expect(game.finishDay("2026-11-02", 90_010, "desk-derby")).toBe(true);
    await settle();
    expect(JSON.parse(saved["ui.deskDerbyDaily"])).toEqual({ streak: 1, last: "2026-11-02", bests: { "2026-11-02": 90_010 } });
  });

  it("head to head keeps the winner's time; a race nobody finished keeps nothing", async () => {
    const game = useGameStore();
    expect(game.finishDuo([0, 0], "desk-derby")).toBe(false);
    expect(game.finishDuo([99_000, 0], "desk-derby")).toBe(true);
    expect(game.finishDuo([101_000, 98_000], "desk-derby")).toBe(true);
    expect(game.finishDuo([99_500, 102_000], "desk-derby")).toBe(false);
    await settle();
    expect(saved["ui.deskDerbyDuoBest"]).toBe("98000");
  });

  it("keeps a head to head best per track, since tracks differ in length", async () => {
    const game = useGameStore();
    await game.show("desk-derby");
    expect(game.finishDuo([62_000, 70_000], "desk-derby", "inbox")).toBe(true);
    // A longer track's race is its own best, not beaten by the short track's.
    expect(game.finishDuo([81_000, 85_000], "desk-derby", "esses")).toBe(true);
    expect(game.finishDuo([82_000, 0], "desk-derby", "esses")).toBe(false);
    await settle();
    expect(game.duoTrackBests).toEqual({ inbox: 62_000, esses: 81_000 });
    expect(JSON.parse(saved[DERBY_DUO_TRACKS_KEY])).toEqual({ inbox: 62_000, esses: 81_000 });
  });

  it("never saves over the saved scores before they've been read", async () => {
    Object.assign(saved, { [DERBY_TRACKS_KEY]: JSON.stringify({ inbox: 60_000, coffee: 70_000 }) });
    const game = useGameStore();
    // Finished before anything loaded: the save waits for the saved ones and keeps them all.
    expect(game.finish(65_000, "desk-derby", "edge")).toBe(true);
    await settle();
    expect(JSON.parse(saved[DERBY_TRACKS_KEY])).toEqual({ inbox: 60_000, coffee: 70_000, edge: 65_000 });
  });

  it("writes nothing when the saved scores can't be read", async () => {
    const { api } = await import("../lib/ipc");
    vi.mocked(api.getSettings).mockRejectedValue(new Error("no settings"));
    try {
      const game = useGameStore();
      game.finish(65_000, "desk-derby", "edge");
      await settle();
      expect(setSetting).not.toHaveBeenCalled();
    } finally {
      vi.mocked(api.getSettings).mockImplementation(async () => ({ ...saved }));
    }
  });

  it("reads saved times back, keeping a faster one set while loading, and drops damaged track bests", async () => {
    Object.assign(saved, {
      "ui.deskDerbyBest": "93000",
      [DERBY_TRACKS_KEY]: JSON.stringify({ inbox: 93_000, edge: "88000", "<script>": 5, coffee: -3 }),
    });
    const game = useGameStore();
    const loading = game.show("desk-derby");
    game.finish(90_000, "desk-derby", "inbox");
    await loading;
    expect(game.scores["desk-derby"].best).toBe(90_000);
    expect(game.trackBests).toEqual({ inbox: 90_000, edge: 88_000 });
    expect(parseTrackBests("{oops")).toEqual({});
    expect(parseTrackBests("[1,2]")).toEqual({});
    expect(parseTrackBests(undefined)).toEqual({});
  });
});
