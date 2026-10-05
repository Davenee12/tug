import { describe, expect, it } from "vitest";
import { LINES, TUG_LINES, friendlyLocateError, locatingLines } from "./locating";

// A fixed sequence of "random" numbers, so the order is repeatable.
const seeded = (seed = 7) => () => {
  seed = (seed * 9301 + 49297) % 233280;
  return seed / 233280;
};

describe("locatingLines", () => {
  it("opens with a tug line and uses every line exactly once", () => {
    const lines = locatingLines(seeded());
    expect((TUG_LINES as readonly string[]).includes(lines[0])).toBe(true);
    expect(new Set(lines).size).toBe(lines.length);
    expect(lines.length).toBe(LINES.length + TUG_LINES.length);
  });

  it("brings a tug line back about every third line, without crowding", () => {
    const lines = locatingLines(seeded(42));
    const tugAt = lines.flatMap((l, i) => ((TUG_LINES as readonly string[]).includes(l) ? [i] : []));
    expect(tugAt.slice(0, 4)).toEqual([0, 3, 6, 9]);
    for (let i = 1; i < tugAt.length; i++) expect(tugAt[i] - tugAt[i - 1]).toBeGreaterThan(1);
  });

  it("varies from one search to the next", () => {
    expect(locatingLines(seeded(1))).not.toEqual(locatingLines(seeded(2)));
  });
});

describe("friendlyLocateError", () => {
  it("speaks plainly about Windows not sharing the location", () => {
    expect(
      friendlyLocateError("Windows isn't sharing your location with apps. Turn it on in Settings › Privacy & security › Location, or type a city."),
    ).toBe("Your PC is keeping your location to itself. Turn on Location in Windows settings, or just type a city.");
  });

  it("treats a slow fix as a retry, and anything else gently", () => {
    expect(friendlyLocateError("Finding your location took too long. Try again, or type a city.")).toContain("Try again");
    expect(friendlyLocateError("HRESULT 0x80004005")).toBe("The map wouldn't budge. Try again, or type a city.");
  });
});
