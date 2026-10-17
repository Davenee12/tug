import { describe, expect, it } from "vitest";
import { droppedPlayer, playerFor, raceKey, seatFor, winner, type DropState } from "./tugboatRace";

describe("who steers what", () => {
  it("the first phone is Player 1 unless swapped, the second Player 2", () => {
    expect([seatFor(1, false), seatFor(2, false)]).toEqual([1, 2]);
    expect([seatFor(1, true), seatFor(2, true)]).toEqual([2, 1]);
    for (const swapped of [false, true]) {
      for (const p of [1, 2] as const) expect(playerFor(seatFor(p, swapped), swapped)).toBe(p);
    }
  });

  it("each player has keys of their own", () => {
    expect(raceKey("ArrowLeft")).toEqual({ player: 1, action: "left" });
    expect(raceKey("ArrowRight")).toEqual({ player: 1, action: "right" });
    expect(raceKey("ArrowUp")).toEqual({ player: 1, action: "hop" });
    expect(raceKey("a")).toEqual({ player: 2, action: "left" });
    expect(raceKey("D")).toEqual({ player: 2, action: "right" });
    expect(raceKey("w")).toEqual({ player: 2, action: "hop" });
    expect(raceKey(" ")).toBeNull();
    expect(raceKey("p")).toBeNull();
  });
});

describe("the result", () => {
  it("the higher score wins; level scores tie", () => {
    expect(winner([410, 380])).toBe(1);
    expect(winner([120, 530])).toBe(2);
    expect(winner([300, 300])).toBeNull();
  });
});

describe("a phone dropping out", () => {
  const racing: DropState = { playing: true, swapped: false, out: [false, false], phoneDrove: [true, true] };

  it("pauses the race for the player whose phone was steering", () => {
    expect(droppedPlayer(2, racing)).toBe(2);
    expect(droppedPlayer(1, racing)).toBe(1);
    // Swapped, the first phone is Player 2's.
    expect(droppedPlayer(1, { ...racing, swapped: true })).toBe(2);
  });

  it("not when nothing is at stake", () => {
    // The race isn't on (menu or results).
    expect(droppedPlayer(2, { ...racing, playing: false })).toBeNull();
    // That player's boat already sank or left.
    expect(droppedPlayer(2, { ...racing, out: [false, true] })).toBeNull();
    // That player was on the keyboard: their phone never steered.
    expect(droppedPlayer(2, { ...racing, phoneDrove: [true, false] })).toBeNull();
  });
});
