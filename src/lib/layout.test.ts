import { describe, expect, it } from "vitest";
import { isWide, railDensity } from "./layout";

describe("layout thresholds (in tug's own, zoomed, pixels)", () => {
  it("a 1366×768 laptop at 100% gets the compact sidebar", () => {
    expect(railDensity(768)).toEqual({ compact: true, tight: false });
    expect(railDensity(900)).toEqual({ compact: false, tight: false });
  });

  it("the same window zoomed to 125% / 150% gets less room, and the layout follows", () => {
    // What the sidebar measures itself at: 768 / 1.25 and 768 / 1.5.
    expect(railDensity(614)).toEqual({ compact: true, tight: true });
    expect(railDensity(512)).toEqual({ compact: true, tight: true });
    // A tall window zoomed in drops out of the roomy layout too.
    expect(railDensity(1000 / 1.5).compact).toBe(true);
  });

  it("the Connection panel goes beside the Feed only with room for it", () => {
    expect(isWide(1366)).toBe(true);
    expect(isWide(1366 / 1.5)).toBe(false);
    expect(isWide(1239)).toBe(false);
  });
});
