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

const { useGameStore, parseBest, SCORE_KEYS } = await import("./game");

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
