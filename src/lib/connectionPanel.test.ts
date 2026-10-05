import { describe, expect, it } from "vitest";
import { nextDownSince, RECONNECT_GRACE_MS, showConnectionPanel, type PanelInputs } from "./connectionPanel";

const T0 = 1_000_000;
const base: PanelInputs = {
  wide: true,
  inSettings: false,
  statusKnown: true,
  connection: "connecting",
  hasDevice: true,
  pairingStale: false,
  downSince: T0,
  now: T0,
};

describe("showConnectionPanel", () => {
  it("stays hidden through a quick relink (the settings flicker)", () => {
    // Real log, 2026-10-05: relinks took 1-17 s, every few minutes.
    for (const secs of [1, 3, 12, 17]) {
      expect(showConnectionPanel({ ...base, now: T0 + secs * 1000 })).toBe(false);
    }
  });

  it("slides in once the phone has been away past the grace period", () => {
    expect(showConnectionPanel({ ...base, connection: "disconnected", now: T0 + RECONNECT_GRACE_MS })).toBe(true);
  });

  it("shows at once when nothing is paired, or the pairing is dead", () => {
    expect(showConnectionPanel({ ...base, connection: "noDevice", hasDevice: false, downSince: T0, now: T0 })).toBe(true);
    expect(showConnectionPanel({ ...base, connection: "disconnected", pairingStale: true })).toBe(true);
  });

  it("never shows before the real status arrives (the placeholder says noDevice)", () => {
    expect(showConnectionPanel({ ...base, statusKnown: false, connection: "noDevice", hasDevice: false })).toBe(false);
  });

  it("never shows while connected, in Settings, or on a narrow window", () => {
    const late = { ...base, now: T0 + RECONNECT_GRACE_MS * 2 };
    expect(showConnectionPanel({ ...late, connection: "connected" })).toBe(false);
    expect(showConnectionPanel({ ...late, inSettings: true })).toBe(false);
    expect(showConnectionPanel({ ...late, wide: false })).toBe(false);
  });
});

describe("nextDownSince", () => {
  it("keeps the first moment the phone went away across connecting/disconnected flips", () => {
    let since: number | null = null;
    since = nextDownSince(since, false, T0); // disconnected
    since = nextDownSince(since, false, T0 + 2000); // connecting
    since = nextDownSince(since, false, T0 + 5000); // disconnected again (a failed attempt)
    expect(since).toBe(T0);
  });

  it("clears once connected, so the next drop starts a fresh grace period", () => {
    expect(nextDownSince(T0, true, T0 + 3000)).toBeNull();
    expect(nextDownSince(null, false, T0 + 60_000)).toBe(T0 + 60_000);
  });
});
