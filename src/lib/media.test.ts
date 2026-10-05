import { describe, expect, it } from "vitest";
import { canRestart, repeatIgnoredMessage, repeatLabel, supportsRepeat } from "./media";

describe("supportsRepeat", () => {
  it("needs AdvanceRepeatMode in the player's command list", () => {
    expect(supportsRepeat({ available: ["play", "pause", "advanceRepeatMode"] })).toBe(true);
    expect(supportsRepeat({ available: ["play", "pause", "nextTrack"] })).toBe(false);
  });

  it("is off before the list arrives, unlike the other buttons", () => {
    expect(supportsRepeat({ available: [] })).toBe(false);
  });
});

describe("repeatLabel", () => {
  it("names the phone-reported mode", () => {
    expect(repeatLabel("off")).toBe("Repeat is off");
    expect(repeatLabel("all")).toBe("Repeating all");
    expect(repeatLabel("one")).toBe("Repeating this song");
    expect(repeatLabel(null)).toBe("Repeat");
  });
});

describe("repeatIgnoredMessage", () => {
  it("names the player", () => {
    expect(repeatIgnoredMessage("Spotify")).toBe("Spotify didn't change repeat from your PC");
    expect(repeatIgnoredMessage(null)).toBe("The player didn't change repeat from your PC");
  });
});

describe("canRestart", () => {
  it("only sends Back well past the start", () => {
    expect(canRestart(42)).toBe(true);
    expect(canRestart(5.5)).toBe(true);
    expect(canRestart(4)).toBe(false);
    expect(canRestart(0)).toBe(false);
    expect(canRestart(null)).toBe(true);
  });
});
