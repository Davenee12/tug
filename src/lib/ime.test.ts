import { describe, expect, it } from "vitest";
import { imeComposing } from "./ime";

describe("imeComposing", () => {
  it("is the input method's key while a word is being composed", () => {
    expect(imeComposing({ isComposing: true, keyCode: 13 })).toBe(true);
    // Some browsers report the composing Enter only as keyCode 229.
    expect(imeComposing({ isComposing: false, keyCode: 229 })).toBe(true);
  });

  it("is tug's key otherwise", () => {
    expect(imeComposing({ isComposing: false, keyCode: 13 })).toBe(false);
    expect(imeComposing({})).toBe(false);
  });
});
