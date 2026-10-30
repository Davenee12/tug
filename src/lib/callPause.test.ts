import { describe, expect, it } from "vitest";
import {
  IDLE,
  MISSED_WAIT_MS,
  OFFER_MS,
  missedCall,
  nextTick,
  resumePressed,
  ringStarted,
  ringStopped,
  tick,
  type CallPause,
} from "./callPause";

const T = 1_000_000;
const ringing = (call = 7): CallPause => ({ phase: "ringing", call });

describe("pausing while the iPhone rings", () => {
  it("pauses when a call starts ringing, once per call", () => {
    const r = ringStarted(IDLE, 7, true);
    expect(r).toEqual({ state: ringing(7), step: "pause" });
    expect(ringStarted(r.state, 7, true).step).toBeNull();
  });

  it("does nothing with the setting off", () => {
    expect(ringStarted(IDLE, 7, false)).toEqual({ state: IDLE, step: null });
  });

  it("pauses again for a second call, even after an answered one", () => {
    const answered: CallPause = { phase: "answered", until: T + OFFER_MS };
    expect(ringStarted(answered, 8, true)).toEqual({ state: ringing(8), step: "pause" });
    expect(ringStarted(ringing(7), 8, true).step).toBe("pause");
  });
});

describe("after the ringing stops", () => {
  it("plays again at once when the call was declined in tug", () => {
    expect(ringStopped(ringing(), 7, "declined", T)).toEqual({ state: IDLE, step: "resume" });
  });

  it("offers Resume (and never plays by itself) when it was answered in tug", () => {
    const r = ringStopped(ringing(), 7, "answered", T);
    expect(r).toEqual({ state: { phase: "answered", until: T + OFFER_MS }, step: "offer" });
    // The hang-up can't be seen: no amount of time plays it again; it's let go at the end.
    expect(tick(r.state, T + OFFER_MS - 1).step).toBeNull();
    expect(tick(r.state, T + OFFER_MS)).toEqual({ state: IDLE, step: "release" });
  });

  it("plays again when a missed call follows the ringing", () => {
    const stopped = ringStopped(ringing(), 7, "unknown", T);
    expect(stopped).toEqual({ state: { phase: "settling", until: T + MISSED_WAIT_MS }, step: null });
    expect(missedCall(stopped.state)).toEqual({ state: IDLE, step: "resume" });
  });

  it("plays again when the missed call comes before the ringing call's removal", () => {
    expect(missedCall(ringing())).toEqual({ state: IDLE, step: "resume" });
  });

  it("treats the call as answered when no missed call comes in time", () => {
    const stopped = ringStopped(ringing(), 7, "unknown", T).state;
    expect(tick(stopped, T + MISSED_WAIT_MS - 1).step).toBeNull();
    const answered = tick(stopped, T + MISSED_WAIT_MS);
    expect(answered).toEqual({ state: { phase: "answered", until: T + MISSED_WAIT_MS + OFFER_MS }, step: "offer" });
    // A missed call that turns up much later is some other call: it doesn't start anything.
    expect(missedCall(answered.state).step).toBeNull();
  });

  it("only follows the call it paused for", () => {
    expect(ringStopped(ringing(7), 8, "declined", T)).toEqual({ state: ringing(7), step: null });
    expect(ringStopped(IDLE, 7, "declined", T)).toEqual({ state: IDLE, step: null });
    expect(missedCall(IDLE)).toEqual({ state: IDLE, step: null });
  });

  it("plays again when Resume is pressed, once", () => {
    const answered = ringStopped(ringing(), 7, "answered", T).state;
    expect(resumePressed(answered)).toEqual({ state: IDLE, step: "resume" });
    expect(resumePressed(IDLE).step).toBeNull();
    expect(resumePressed(ringing()).step).toBeNull();
  });

  it("says when to look again", () => {
    expect(nextTick(IDLE)).toBeNull();
    expect(nextTick(ringing())).toBeNull();
    expect(nextTick({ phase: "settling", until: T })).toBe(T);
    expect(nextTick({ phase: "answered", until: T })).toBe(T);
  });
});
