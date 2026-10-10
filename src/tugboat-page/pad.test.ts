import { describe, expect, it } from "vitest";
import { HEARTBEAT_MS, MIN_GAP_MS, encodePad, nextSendDelay, padErrorAction, samePad, steerFromTouch, tiltSupport } from "./pad";

describe("encodePad", () => {
  it("sends whole-number steering from -100 to 100 and a real boolean", () => {
    expect(encodePad(0.5, true)).toEqual({ steer: 50, boost: true });
    expect(encodePad(-1, false)).toEqual({ steer: -100, boost: false });
    expect(encodePad(0.333, false)).toEqual({ steer: 33, boost: false });
  });

  it("never sends anything the PC would refuse", () => {
    expect(encodePad(7, false).steer).toBe(100);
    expect(encodePad(-7, false).steer).toBe(-100);
    expect(encodePad(Number.NaN, false).steer).toBe(0);
    expect(encodePad(Number.POSITIVE_INFINITY, false).steer).toBe(0);
    expect(Object.is(encodePad(-0.001, false).steer, 0)).toBe(true);
    // Only the two fields, so the JSON is exactly what the PC expects.
    expect(Object.keys(encodePad(0.2, true))).toEqual(["steer", "boost"]);
    expect(JSON.stringify(encodePad(-0.004, false))).toBe('{"steer":0,"boost":false}');
  });

  it("compares states", () => {
    expect(samePad(null, encodePad(0, false))).toBe(false);
    expect(samePad(encodePad(0.5, true), encodePad(0.5, true))).toBe(true);
    expect(samePad(encodePad(0.5, true), encodePad(0.5, false))).toBe(false);
  });
});

describe("steerFromTouch", () => {
  it("goes straight in the middle and reaches full lock before the edge", () => {
    expect(steerFromTouch(200, 400)).toBe(0);
    expect(steerFromTouch(210, 400)).toBe(0); // inside the dead zone
    expect(steerFromTouch(400, 400)).toBe(1);
    expect(steerFromTouch(0, 400)).toBe(-1);
    expect(steerFromTouch(380, 400)).toBe(1);
    const half = steerFromTouch(300, 400);
    expect(half).toBeGreaterThan(0.4);
    expect(half).toBeLessThan(0.7);
    expect(steerFromTouch(100, 400)).toBeCloseTo(-half);
  });

  it("is safe with odd sizes", () => {
    expect(steerFromTouch(10, 0)).toBe(0);
    expect(steerFromTouch(Number.NaN, 400)).toBe(0);
  });
});

describe("pacing", () => {
  it("sends a change at most ~30 times a second, and a heartbeat when nothing changes", () => {
    expect(nextSendDelay(0, true)).toBe(MIN_GAP_MS);
    expect(nextSendDelay(MIN_GAP_MS + 5, true)).toBe(0);
    expect(nextSendDelay(50, false)).toBe(HEARTBEAT_MS - 50);
    expect(nextSendDelay(HEARTBEAT_MS * 2, false)).toBe(0);
  });
});

describe("padErrorAction", () => {
  it("goes back to the Tugboat page when the game or session ended", () => {
    for (const code of ["no-game", "closed", "unauthorized", "in-use"]) expect(padErrorAction(code)).toBe("leave");
    expect(padErrorAction("busy")).toBe("slow");
    for (const code of ["network", "stale", "bad-request", "error"]) expect(padErrorAction(code)).toBe("retry");
  });
});

describe("tiltSupport", () => {
  it("says tilt needs a secure connection on Tugboat's plain-HTTP page", () => {
    expect(tiltSupport({ secure: false, hasOrientation: true })).toBe("needs-secure");
    expect(tiltSupport({ secure: false, hasOrientation: false })).toBe("needs-secure");
    expect(tiltSupport({ secure: true, hasOrientation: true })).toBe("available");
    expect(tiltSupport({ secure: true, hasOrientation: false })).toBe("unsupported");
  });
});
