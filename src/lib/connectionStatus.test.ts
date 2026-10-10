import { describe, expect, it } from "vitest";
import { connectionBusy, connectionLabel, connectionSentence, NOT_AN_IPHONE } from "./connectionStatus";
import winrtRs from "../../src-tauri/src/ble/winrt.rs?raw";

const base = { connection: "connected" as const, awaitingUnlock: false, reconnecting: false, radio: "on" as const };

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

  it("says the iPhone is away while it's out of range, but asks for an unlock first", () => {
    expect(connectionLabel({ ...base, connection: "disconnected", away: true })).toBe("iPhone away");
    expect(connectionLabel({ ...base, connection: "disconnected", away: true, awaitingUnlock: true })).toBe(
      "Unlock your iPhone",
    );
  });

  it("asks for an unlock over saying it's reconnecting", () => {
    expect(connectionLabel({ ...base, connection: "disconnected", awaitingUnlock: true, reconnecting: true })).toBe(
      "Unlock your iPhone",
    );
  });

  it("never claims Reconnecting or Connecting with Bluetooth off or missing", () => {
    for (const radio of ["off", "unavailable"] as const) {
      for (const connection of ["disconnected", "connecting"] as const) {
        const label = connectionLabel({ ...base, connection, reconnecting: true, radio });
        expect(label).not.toMatch(/onnecting/);
        expect(label).toMatch(/Bluetooth/);
        // A relink caught by the radio going off must not pulse until it comes back.
        expect(connectionBusy({ ...base, connection, reconnecting: true, radio })).toBe(false);
      }
    }
    expect(connectionLabel({ ...base, connection: "connecting", radio: "off" })).toBe("Bluetooth is off");
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

describe("connectionSentence agrees with the sidebar label", () => {
  const device = { id: "x", name: "iPhone", model: null };
  const cases = [
    { connection: "connected" as const, label: "Connected", sentence: /^Connected/ },
    { connection: "connecting" as const, label: "Connecting…", sentence: /^Connecting/ },
    { connection: "disconnected" as const, label: "Waiting for iPhone", sentence: /^Waiting/ },
    { connection: "disconnected" as const, awaitingUnlock: true, label: "Unlock your iPhone", sentence: /Unlock it/ },
    { connection: "disconnected" as const, reconnecting: true, label: "Reconnecting…", sentence: /^Reconnecting/ },
    { connection: "connecting" as const, reconnecting: true, label: "Reconnecting…", sentence: /^Reconnecting/ },
    { connection: "connecting" as const, radio: "off" as const, label: "Bluetooth is off", sentence: /Bluetooth is off/ },
    { connection: "disconnected" as const, radio: "off" as const, label: "Bluetooth is off", sentence: /Bluetooth is off/ },
    { connection: "disconnected" as const, away: true, label: "iPhone away", sentence: /out of range/ },
    // Away wins over reconnecting in both, so the label and the sentence never disagree.
    { connection: "disconnected" as const, away: true, reconnecting: true, label: "iPhone away", sentence: /out of range/ },
    { connection: "disconnected" as const, awaitingUnlock: true, reconnecting: true, label: "Unlock your iPhone", sentence: /Unlock it/ },
    { connection: "connecting" as const, awaitingUnlock: true, label: "Connecting…", sentence: /^Connecting/ },
    // The phone restarted after working this run: say so, and that unlocking is all it takes.
    {
      connection: "disconnected" as const,
      awaitingUnlock: true,
      phoneRestarted: true,
      label: "iPhone restarted: unlock it",
      sentence: /^Your iPhone restarted\. Unlock it and tug reconnects by itself\.$/,
    },
    // An Android phone paired by mistake: never "Unlock your iPhone".
    { connection: "disconnected" as const, notIphone: true, label: "Not an iPhone", sentence: /doesn't look like an iPhone/ },
    { connection: "disconnected" as const, notIphone: true, awaitingUnlock: true, label: "Not an iPhone", sentence: /works with iPhone/ },
  ];
  it.each(cases)("$label", (c) => {
    const s = { ...base, ...c, device };
    expect(connectionLabel(s)).toBe(c.label);
    expect(connectionSentence(s)).toMatch(c.sentence);
  });

  it("says what the Rust side logs for a phone that isn't an iPhone", () => {
    expect(winrtRs).toContain(`NOT_AN_IPHONE: &str = "${NOT_AN_IPHONE}"`);
  });

  it("asks to pair when nothing is set up", () => {
    expect(connectionSentence({ ...base, connection: "noDevice", device: null })).toMatch(/Pair your iPhone/);
  });
});
