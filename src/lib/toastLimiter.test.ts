import { describe, expect, it } from "vitest";
import { ToastLimiter } from "./toastLimiter";

describe("ToastLimiter", () => {
  it("lets a few through, then holds the rest for one summary", () => {
    const l = new ToastLimiter(3, 10_000);
    expect([0, 100, 200].map((t) => l.admit(t))).toEqual([true, true, true]);
    expect(l.admit(300)).toBe(false);
    expect(l.admit(400)).toBe(false);
    expect(l.takeHeld()).toBe(2);
    expect(l.takeHeld()).toBe(0); // counted once
  });

  it("frees up again once the window has passed", () => {
    const l = new ToastLimiter(2, 10_000);
    l.admit(0);
    l.admit(1_000);
    expect(l.admit(5_000)).toBe(false);
    expect(l.admit(10_001)).toBe(true); // the first one aged out
    expect(l.admit(10_500)).toBe(false); // the second is still inside the window
  });
});
