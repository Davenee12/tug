import { describe, expect, it } from "vitest";
import { olderWindowStart, startIncluding, THREAD_STEP, THREAD_WINDOW, windowStart } from "./threadWindow";

const items = (n: number) => Array.from({ length: n }, (_, i) => ({ id: `m${i}` }));

describe("windowStart", () => {
  it("shows a short conversation whole", () => {
    expect(windowStart(items(12), null)).toBe(0);
  });

  it("shows the newest items of a long one", () => {
    expect(windowStart(items(300), null)).toBe(300 - THREAD_WINDOW);
  });

  it("stays pinned to the oldest item on screen as new texts arrive", () => {
    const list = items(300);
    const anchor = list[windowStart(list, null)].id;
    // Three new texts at the bottom: nothing drops off the top.
    expect(windowStart(items(303), anchor)).toBe(300 - THREAD_WINDOW);
  });

  it("falls back to the newest items when the anchor is gone", () => {
    expect(windowStart(items(100), "nope")).toBe(100 - THREAD_WINDOW);
  });
});

describe("revealing older items", () => {
  it("steps back a page, never past the start", () => {
    expect(olderWindowStart(240)).toBe(240 - THREAD_STEP);
    expect(olderWindowStart(20)).toBe(0);
  });

  it("opens down to an item from search, with a little above it", () => {
    expect(startIncluding(240, 15)).toBe(5);
    expect(startIncluding(240, 3)).toBe(0);
    // Already on screen, or not in this conversation: unchanged.
    expect(startIncluding(240, 260)).toBe(240);
    expect(startIncluding(240, -1)).toBe(240);
  });
});
