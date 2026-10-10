import { describe, expect, it } from "vitest";
import { canWiggle, isTugChant, shouldTug, WIGGLE_GAP_MS } from "./logoTricks";
import type { ConnectionState } from "../types/protocol";

describe("shouldTug", () => {
  it.each<[ConnectionState | null, ConnectionState | null, boolean]>([
    ["connecting", "connected", true], // first connect after launch
    ["disconnected", "connected", true], // every reconnect
    ["noDevice", "connected", true], // fresh pairing
    [null, "connected", false], // already connected when the window loaded
    ["connected", "connected", false], // a status refresh
    ["connected", "disconnected", false],
    ["disconnected", "connecting", false],
    [null, null, false],
  ])("%s → %s: %s", (prev, next, want) => {
    expect(shouldTug(prev, next)).toBe(want);
  });
});

describe("canWiggle", () => {
  it("wiggles the first time and then at most every gap", () => {
    expect(canWiggle(null, 1000)).toBe(true);
    expect(canWiggle(1000, 1000 + WIGGLE_GAP_MS - 1)).toBe(false);
    expect(canWiggle(1000, 1000 + WIGGLE_GAP_MS)).toBe(true);
  });
});

describe("isTugChant", () => {
  it.each([
    ["tug tug tug tug tug", true],
    ["TUG Tug tUg tug tug", true],
    ["tugtugtugtugtug", true],
    ["  tug tugtug  tug tug ", true],
    ["tug tug tug tug", false],
    ["tug tug tug tug tug tug", false],
    ["tug tug tug tug tugboat", false],
    ["tugboat", false],
    ["", false],
  ])("%j: %s", (q, want) => {
    expect(isTugChant(q)).toBe(want);
  });
});
