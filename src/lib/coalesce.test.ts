import { describe, expect, it } from "vitest";
import { coalesce } from "./coalesce";

/** A scheduler the test runs by hand, like a frame that hasn't come yet. */
function manual() {
  let queued: (() => void) | null = null;
  let scheduled = 0;
  return {
    schedule: (fn: () => void) => {
      scheduled++;
      queued = fn;
      return () => (queued = null);
    },
    run: () => queued?.(),
    get scheduled() {
      return scheduled;
    },
    get pending() {
      return queued !== null;
    },
  };
}

describe("coalesce", () => {
  it("applies a burst once, with the newest value", () => {
    const seen: string[] = [];
    const frame = manual();
    const c = coalesce<string>((v) => seen.push(v), frame.schedule);
    c.push("title");
    c.push("title+artist");
    c.push("title+artist+album");
    expect(seen).toEqual([]);
    expect(frame.scheduled).toBe(1);
    frame.run();
    expect(seen).toEqual(["title+artist+album"]);
  });

  it("schedules again for the next burst", () => {
    const seen: number[] = [];
    const frame = manual();
    const c = coalesce<number>((v) => seen.push(v), frame.schedule);
    c.push(1);
    frame.run();
    c.push(2);
    frame.run();
    expect(seen).toEqual([1, 2]);
    expect(frame.scheduled).toBe(2);
  });

  it("flush applies now and calls off the scheduled run", () => {
    const seen: number[] = [];
    const frame = manual();
    const c = coalesce<number>((v) => seen.push(v), frame.schedule);
    c.push(1);
    c.flush();
    expect(seen).toEqual([1]);
    expect(frame.pending).toBe(false);
    c.flush(); // nothing pending: nothing applied
    expect(seen).toEqual([1]);
  });

  it("cancel drops what's pending", () => {
    const seen: number[] = [];
    const frame = manual();
    const c = coalesce<number>((v) => seen.push(v), frame.schedule);
    c.push(1);
    c.cancel();
    frame.run();
    expect(seen).toEqual([]);
  });
});
