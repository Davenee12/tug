import { describe, expect, it } from "vitest";
import { phoneSwitches, switchesOff, type SwitchState } from "./phoneSwitches";
import type { DeviceStatus } from "../types/protocol";

// A fully connected, everything-on phone; tests override only what they exercise.
const CONNECTED: DeviceStatus = {
  radio: "on",
  peripheralSupported: true,
  advertising: "on",
  device: { id: "x", name: "iPhone" },
  connection: "connected",
  battery: 76,
  services: { notifications: true, media: true, battery: true, messages: true },
  lastError: null,
  lastErrorAt: null,
  pairingStale: false,
  awaitingPhoneAllow: false,
  awaitingUnlock: false,
  messagesError: null,
  contactsError: null,
  contactsShared: true,
  textsPairing: "ok",
  textsDevice: "iPhone",
  liveTexts: "off",
};

function states(overrides: Partial<DeviceStatus>): Record<string, SwitchState> {
  const list = phoneSwitches({ ...CONNECTED, ...overrides });
  return Object.fromEntries(list.map((x) => [x.key, x.state]));
}

describe("phoneSwitches", () => {
  it("names the three switches in order with exact labels", () => {
    const list = phoneSwitches(CONNECTED);
    expect(list.map((x) => x.key)).toEqual(["notifications", "messages", "contacts"]);
    expect(list.map((x) => x.label)).toEqual(["Share System Notifications", "Show Notifications", "Sync Contacts"]);
    // Every switch points at where it lives on the phone.
    for (const x of list) expect(x.where).toContain("Settings › Bluetooth");
  });

  it("is all on when everything is healthy", () => {
    const s = states({});
    expect(s).toEqual({ notifications: "on", messages: "on", contacts: "on" });
  });

  it("marks only required switches required", () => {
    const list = phoneSwitches(CONNECTED);
    expect(list.find((x) => x.key === "notifications")!.required).toBe(true);
    expect(list.find((x) => x.key === "messages")!.required).toBe(false);
    expect(list.find((x) => x.key === "contacts")!.required).toBe(false);
  });

  it("notifications are unknown until the phone is connected", () => {
    expect(states({ connection: "connecting", services: { notifications: false, media: false, battery: false, messages: false } }).notifications).toBe("unknown");
    expect(states({ connection: "disconnected", services: { notifications: false, media: false, battery: false, messages: false } }).notifications).toBe("unknown");
  });

  it("notifications off once connected but the probe says not sharing", () => {
    expect(states({ services: { notifications: false, media: true, battery: true, messages: true } }).notifications).toBe("off");
  });

  it("reads a 0xC3 message refusal as the Show Notifications switch being off", () => {
    const s = states({
      services: { notifications: true, media: true, battery: true, messages: false },
      messagesError: "the iPhone refused message access; turn on Show Notifications for this PC",
    });
    expect(s.messages).toBe("off");
  });

  it("leaves Show Notifications unknown before any message signal", () => {
    const s = states({
      services: { notifications: true, media: true, battery: true, messages: false },
      messagesError: null,
    });
    expect(s.messages).toBe("unknown");
  });

  it("marks Sync Contacts off on a refusal, unknown until the phone shares, on when it does", () => {
    expect(states({ contactsError: "the iPhone refused contact access", contactsShared: false }).contacts).toBe("off");
    expect(states({ contactsShared: false }).contacts).toBe("unknown");
    expect(states({ contactsShared: true }).contacts).toBe("on");
  });

  it("never guesses: a disconnected phone leaves every switch unknown", () => {
    const s = states(
      {
        connection: "disconnected",
        services: { notifications: false, media: false, battery: false, messages: false },
        messagesError: null,
        contactsError: null,
        contactsShared: false,
      },
    );
    expect(s).toEqual({ notifications: "unknown", messages: "unknown", contacts: "unknown" });
  });

  it("switchesOff lists exactly the switches that are off, with a fix", () => {
    const list = phoneSwitches(
      { ...CONNECTED, services: { notifications: false, media: true, battery: true, messages: true }, contactsError: "no", contactsShared: false },
    );
    const off = switchesOff(list);
    expect(off.map((x) => x.key)).toEqual(["notifications", "contacts"]);
    for (const x of off) expect(x.fix.length).toBeGreaterThan(0);
  });
});
