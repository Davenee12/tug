import { describe, expect, it } from "vitest";
import { escClosesSettings } from "./escape";

describe("escClosesSettings", () => {
  it("closes Settings on a plain Esc", () => {
    expect(escClosesSettings({ key: "Escape", defaultPrevented: false }, false)).toBe(true);
  });

  it("leaves Settings open when a pop-up's focus trap already handled the Esc", () => {
    // The trap closed the pop-up first, so nothing covers Settings any more; the event says it was handled.
    expect(escClosesSettings({ key: "Escape", defaultPrevented: true }, false)).toBe(false);
  });

  it("leaves Settings open while something covers it (setup run again, a dialog)", () => {
    expect(escClosesSettings({ key: "Escape", defaultPrevented: false }, true)).toBe(false);
  });

  it("ignores other keys", () => {
    expect(escClosesSettings({ key: ",", defaultPrevented: false }, false)).toBe(false);
  });
});
