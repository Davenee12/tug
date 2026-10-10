import { describe, expect, it } from "vitest";
import { newPcAudioProblem, pcAudioProblemText, pcAudioView } from "./pcAudio";
import type { PcAudioProblem, PcAudioStatus } from "../types/protocol";

const status = (s: Partial<PcAudioStatus> = {}): PcAudioStatus => ({
  supported: true,
  state: "off",
  problem: null,
  auto: false,
  ...s,
});

describe("pcAudioView", () => {
  it("offers Play on this PC once the iPhone is connected", () => {
    const v = pcAudioView(status(), true);
    expect(v).toMatchObject({ show: true, action: "start", label: "Play on this PC", active: false });
  });

  it("hides the button until the status loads, and on Windows that can't do it", () => {
    expect(pcAudioView(null, true).show).toBe(false);
    const old = pcAudioView(status({ supported: false }), true);
    expect(old.show).toBe(false);
    expect(old.line).toBe("Needs Windows 10 version 2004 or later.");
  });

  it("asks for the iPhone first when it isn't connected", () => {
    const v = pcAudioView(status(), false);
    expect(v.show).toBe(false);
    expect(v.line).toBe("Connect your iPhone first.");
  });

  it("makes playing on this PC obvious and one click to stop", () => {
    const v = pcAudioView(status({ state: "on" }), true);
    expect(v).toMatchObject({ show: true, action: "stop", label: "Stop", active: true });
    expect(v.title).toBe("Stop playing on this PC");
    // Still on (and stoppable) if the phone's link blips.
    expect(pcAudioView(status({ state: "on" }), false).action).toBe("stop");
  });

  it("can cancel while connecting", () => {
    const v = pcAudioView(status({ state: "connecting" }), true);
    expect(v).toMatchObject({ action: "stop", busy: true, label: "Cancel" });
  });

  it("offers Reconnect after a drop and Try again after a failure", () => {
    expect(pcAudioView(status({ problem: "dropped" }), true)).toMatchObject({
      action: "start",
      label: "Reconnect",
      problem: "Your iPhone stopped playing on this PC.",
    });
    const timedOut = pcAudioView(status({ problem: "timedOut" }), true);
    expect(timedOut.label).toBe("Try again");
    expect(timedOut.line).toBe("Your iPhone didn't connect audio. Make sure it's unlocked and try again.");
  });
});

describe("pcAudioProblemText", () => {
  it("has a calm, plain sentence for every problem", () => {
    const all: PcAudioProblem[] = ["notFound", "denied", "timedOut", "failed", "dropped"];
    for (const p of all) {
      const text = pcAudioProblemText(p);
      expect(text).toMatch(/^[A-Z].*\.$/);
      expect(text).not.toMatch(/A2DP|AudioPlayback|error|0x/i);
    }
  });
});

describe("newPcAudioProblem", () => {
  it("speaks up when a problem appears, once", () => {
    const connecting = status({ state: "connecting" });
    const failed = status({ problem: "timedOut" });
    expect(newPcAudioProblem(connecting, failed)).toBe("timedOut");
    expect(newPcAudioProblem(failed, failed)).toBeNull();
    expect(newPcAudioProblem(status({ state: "on" }), status({ problem: "dropped" }))).toBe("dropped");
  });

  it("stays quiet on first load and when things go well", () => {
    expect(newPcAudioProblem(null, status({ problem: "dropped" }))).toBeNull();
    expect(newPcAudioProblem(status({ state: "connecting" }), status({ state: "on" }))).toBeNull();
  });
});
