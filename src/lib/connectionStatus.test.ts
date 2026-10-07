import { describe, expect, it } from "vitest";
import { connectionBusy, connectionLabel } from "./connectionStatus";

const base = { connection: "connected" as const, awaitingUnlock: false, reconnecting: false };

describe("connectionLabel", () => {
  it("names each ordinary state", () => {
    expect(connectionLabel(base)).toBe("Connected");
    expect(connectionLabel({ ...base, connection: "connecting" })).toBe("Connecting…");
    expect(connectionLabel({ ...base, connection: "disconnected" })).toBe("Waiting for iPhone");
    expect(connectionLabel({ ...base, connection: "noDevice" })).toBe("Not set up");
  });

  it("says Reconnecting while tug rebuilds the link on its own, not that the phone is gone", () => {
    // 2026-10-06: Bluetooth stalled for ~3 minutes and the sidebar read as disconnected.
    expect(connectionLabel({ ...base, connection: "connecting", reconnecting: true })).toBe("Reconnecting…");
    expect(connectionLabel({ ...base, connection: "disconnected", reconnecting: true })).toBe("Reconnecting…");
  });

  it("asks for an unlock over saying it's reconnecting", () => {
    expect(connectionLabel({ ...base, connection: "disconnected", awaitingUnlock: true, reconnecting: true })).toBe(
      "Unlock your iPhone",
    );
  });

  it("never claims Reconnecting once connected", () => {
    expect(connectionLabel({ ...base, reconnecting: true })).toBe("Connected");
  });
});

describe("connectionBusy", () => {
  it("pulses while connecting or reconnecting, not while waiting idle", () => {
    expect(connectionBusy({ ...base, connection: "connecting" })).toBe(true);
    expect(connectionBusy({ ...base, connection: "disconnected", reconnecting: true })).toBe(true);
    expect(connectionBusy({ ...base, connection: "disconnected" })).toBe(false);
    expect(connectionBusy({ ...base, connection: "disconnected", reconnecting: true, awaitingUnlock: true })).toBe(false);
    expect(connectionBusy(base)).toBe(false);
  });
});
