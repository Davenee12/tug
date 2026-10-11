import { describe, expect, it } from "vitest";
import { HEARTBEAT_MS, MAX_IN_FLIGHT, MIN_GAP_MS, encodePad, nextSendDelay, padErrorAction, positionFromTouch, probe, samePad, tiltSupport } from "./pad";

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

describe("positionFromTouch", () => {
  it("maps straight across the pad, with easy full left and full right", () => {
    expect(positionFromTouch(200, 400)).toBe(0);
    expect(positionFromTouch(300, 400)).toBeCloseTo(0.5 / 0.88);
    expect(positionFromTouch(100, 400)).toBeCloseTo(-0.5 / 0.88);
    // The thin margins at the edges are full lock.
    expect(positionFromTouch(395, 400)).toBe(1);
    expect(positionFromTouch(4, 400)).toBe(-1);
    // Off the pad (a finger that slid past the edge) stays at full lock.
    expect(positionFromTouch(-30, 400)).toBe(-1);
    expect(positionFromTouch(480, 400)).toBe(1);
  });

  it("moves in proportion to the finger: no dead zone", () => {
    const a = positionFromTouch(210, 400);
    const b = positionFromTouch(220, 400);
    expect(a).toBeGreaterThan(0);
    expect(b - a).toBeCloseTo(a - positionFromTouch(200, 400));
  });

  it("is safe with odd sizes", () => {
    expect(positionFromTouch(10, 0)).toBe(0);
    expect(positionFromTouch(Number.NaN, 400)).toBe(0);
  });
});

describe("probe", () => {
  it("sends whole milliseconds the PC accepts", () => {
    expect(probe(3.4, 12.6)).toEqual({ age: 3, rtt: 13 });
    expect(probe(-5, 99_999)).toEqual({ age: 0, rtt: 10_000 });
    expect(probe(Number.NaN, Number.POSITIVE_INFINITY)).toEqual({ age: 0, rtt: 0 });
  });
});
describe("pacing", () => {
  it("sends a change at once, at most ~60 times a second, and a heartbeat when nothing changes", () => {
    expect(MIN_GAP_MS).toBeLessThanOrEqual(17);
    expect(MAX_IN_FLIGHT).toBe(2);
    // The first change after a pause goes straight out.
    expect(nextSendDelay(Number.POSITIVE_INFINITY, true)).toBe(0);
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
