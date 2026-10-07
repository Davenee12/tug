import { describe, expect, it } from "vitest";
import { canSeeWindow } from "./attention";

describe("canSeeWindow", () => {
  const shown = { visibility: "visible" as const, focused: true, overlayOpen: false };

  it("is true only when the window is shown, focused and uncovered", () => {
    expect(canSeeWindow(shown)).toBe(true);
  });

  it("is false while tug sits hidden in the tray (texts there stay unread)", () => {
    expect(canSeeWindow({ ...shown, visibility: "hidden" })).toBe(false);
    // Hidden wins even if the webview still reports focus.
    expect(canSeeWindow({ visibility: "hidden", focused: true, overlayOpen: false })).toBe(false);
  });

  it("is false when another window has focus", () => {
    expect(canSeeWindow({ ...shown, focused: false })).toBe(false);
  });

  it("is false while search, the picker or a prompt covers the main view", () => {
    expect(canSeeWindow({ ...shown, overlayOpen: true })).toBe(false);
  });
});
