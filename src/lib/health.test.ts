import { describe, expect, it } from "vitest";
import { connectionHealth, errorAge, type HealthLink } from "./health";
import type { DeviceStatus } from "../types/protocol";

// A fully connected, healthy phone; tests override only what they exercise.
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
  messagesError: null,
  contactsError: null,
  textsPairing: "ok",
  textsDevice: "iPhone",
  liveTexts: "off",
};

function health(overrides: Partial<DeviceStatus>, counts = { contacts: 3, calls: 4 }) {
  const links = connectionHealth({ ...CONNECTED, ...overrides }, counts);
  return new Map(links.map((l) => [l.key, l] as const));
}

function link(overrides: Partial<DeviceStatus>, key: string): HealthLink {
  const l = health(overrides).get(key);
  if (!l) throw new Error(`no link ${key}`);
  return l;
}

describe("connectionHealth", () => {
  it("lists every link in order", () => {
    const keys = connectionHealth(CONNECTED, { contacts: 3, calls: 4 }).map((l) => l.key);
    expect(keys).toEqual(["radio", "advertising", "notifications", "media", "battery", "texts", "contacts", "calls"]);
  });

  it("is all green when everything is healthy", () => {
    for (const l of connectionHealth(CONNECTED, { contacts: 3, calls: 4 })) {
      expect(l.state, l.key).toBe("ok");
    }
  });

  it("flags Bluetooth off as an error with a fix", () => {
    const l = link({ radio: "off" }, "radio");
    expect(l.state).toBe("error");
    expect(l.detail).toMatch(/Turn it on/);
  });

  it("flags an adapter that can't be a peripheral", () => {
    const l = link({ peripheralSupported: false }, "advertising");
    expect(l.state).toBe("error");
    expect(l.detail).toMatch(/can't act as a peripheral/);
  });

  it("asks the owner to turn visibility on", () => {
    expect(link({ advertising: "off" }, "advertising").state).toBe("off");
  });

  it("downstream links wait until connected", () => {
    const m = health({ connection: "disconnected", device: { id: "x", name: "iPhone" } });
    for (const key of ["notifications", "media", "battery", "texts", "contacts", "calls"]) {
      expect(m.get(key)!.state, key).toBe("waiting");
      expect(m.get(key)!.detail).toMatch(/reconnect/);
    }
    // The radio and visibility don't need a connection.
    expect(m.get("radio")!.state).toBe("ok");
  });

  it("tells a never-paired owner to pair", () => {
    const m = health({ connection: "noDevice", device: null });
    expect(m.get("texts")!.detail).toMatch(/Pair your iPhone/);
  });

  it("surfaces a stale pairing on the notifications link", () => {
    const l = link({ pairingStale: true, services: { notifications: false, media: true, battery: true, messages: true } }, "notifications");
    expect(l.state).toBe("error");
    expect(l.detail).toMatch(/forgot this PC/);
  });

  it("asks to share notifications when the switch is off", () => {
    const l = link({ services: { notifications: false, media: true, battery: true, messages: true } }, "notifications");
    expect(l.state).toBe("off");
    expect(l.detail).toMatch(/Share System Notifications/);
  });

  it("explains a broken texts pairing naming the device", () => {
    const l = link({ textsPairing: "broken", textsDevice: "Dave's iPhone" }, "texts");
    expect(l.state).toBe("error");
    expect(l.detail).toContain("Dave's iPhone");
    expect(l.detail).toMatch(/remove .* and add it again/);
  });

  it("treats a missing texts pairing as a switch to flip", () => {
    expect(link({ textsPairing: "missing" }, "texts").state).toBe("off");
  });

  it("passes the phone's own message error through, capitalised", () => {
    const l = link({ messagesError: "the iPhone refused message access; turn on Show Notifications" }, "texts");
    expect(l.state).toBe("off");
    expect(l.detail).toBe("The iPhone refused message access; turn on Show Notifications");
  });

  it("shows the battery level when it's known", () => {
    expect(link({ battery: 42 }, "battery").detail).toContain("42%");
  });

  it("counts contacts and calls", () => {
    const m = health({}, { contacts: 1, calls: 1 });
    expect(m.get("contacts")!.detail).toContain("1 contact synced");
    expect(m.get("calls")!.detail).toContain("1 recent call loaded");
  });

  it("asks to sync contacts when none have arrived", () => {
    const l = health({}, { contacts: 0, calls: 0 }).get("contacts")!;
    expect(l.state).toBe("off");
    expect(l.detail).toMatch(/Sync Contacts/);
  });
});

describe("errorAge", () => {
  it("returns null without a timestamp", () => {
    expect(errorAge(null, 1000)).toBeNull();
  });

  it("reads recent, minutes, hours and days", () => {
    const now = 10_000_000;
    expect(errorAge(now - 5_000, now)).toBe("just now");
    expect(errorAge(now - 3 * 60_000, now)).toBe("3m ago");
    expect(errorAge(now - 2 * 3_600_000, now)).toBe("2h ago");
    expect(errorAge(now - 3 * 86_400_000, now)).toBe("3d ago");
  });
});
