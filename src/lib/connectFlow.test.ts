import { describe, expect, it } from "vitest";
import {
  canSkipSwitches,
  connectDone,
  connectStep,
  nextShowConnect,
  optionalNudge,
  rescanDue,
  RESCAN_INTERVAL_MS,
  shouldWatchSwitches,
} from "./connectFlow";
import type { DeviceStatus, PairingRequest } from "../types/protocol";

// A fully connected, everything-on phone; tests override only what they exercise.
const CONNECTED: DeviceStatus = {
  radio: "on",
  peripheralSupported: true,
  advertising: "on",
  device: { id: "x", name: "Dave's iPhone" },
  connection: "connected",
  battery: 76,
  services: { notifications: true, media: true, battery: true, messages: true },
  lastError: null,
  lastErrorAt: null,
  pairingStale: false,
  awaitingPhoneAllow: false,
  messagesError: null,
  contactsError: null,
  contactsShared: true,
  textsPairing: "ok",
  textsDevice: "Dave's iPhone",
  liveTexts: "off",
};

/** Nothing paired yet, the way the backend reports it on a fresh install. */
const FRESH: DeviceStatus = {
  ...CONNECTED,
  device: null,
  connection: "noDevice",
  battery: null,
  services: { notifications: false, media: false, battery: false, messages: false },
  contactsShared: false,
  textsPairing: "missing",
  textsDevice: null,
};

const REQUEST: PairingRequest = { deviceName: "Dave's iPhone", pin: "482 913", confirmOnPhone: false };

describe("connectStep", () => {
  it("looks for an iPhone when nothing is paired and no pairing is underway", () => {
    expect(connectStep(FRESH, null)).toBe("find");
  });

  it("shows the pairing phase while a pairing request is in flight", () => {
    expect(connectStep(FRESH, REQUEST)).toBe("pairing");
  });

  it("asks for Allow once paired while iOS holds the connection open", () => {
    const s = { ...FRESH, device: { id: "a", name: "Dave's iPhone" }, connection: "connecting" as const, awaitingPhoneAllow: true };
    expect(connectStep(s, null)).toBe("allow");
  });

  it("shows the switches while paired and connecting, before notifications are on", () => {
    const s = { ...FRESH, device: { id: "a", name: "Dave's iPhone" }, connection: "connected" as const };
    expect(connectStep(s, null)).toBe("switches");
  });

  it("is done once notifications are working, even mid-pairing-dialog", () => {
    expect(connectStep(CONNECTED, null)).toBe("done");
    expect(connectStep(CONNECTED, REQUEST)).toBe("done");
  });
});

describe("connectDone", () => {
  it("needs both a live connection and the notifications service", () => {
    expect(connectDone(CONNECTED)).toBe(true);
    expect(connectDone({ ...CONNECTED, services: { ...CONNECTED.services, notifications: false } })).toBe(false);
    expect(connectDone({ ...CONNECTED, connection: "connecting" })).toBe(false);
  });
});

describe("waiting for all three switches", () => {
  // Dave: the panel rushed to the Feed before he'd turned the switches on.
  const contactsOff = { ...CONNECTED, contactsShared: false };
  const textsOff = { ...CONNECTED, services: { ...CONNECTED.services, messages: false }, messagesError: "the iPhone refused message access" };

  it("stays on the switches until every one is on", () => {
    expect(connectDone(contactsOff)).toBe(false);
    expect(connectDone(textsOff)).toBe(false);
    expect(connectStep(contactsOff, null)).toBe("switches");
    expect(connectDone(CONNECTED)).toBe(true);
  });

  it("an open panel stays up while optional switches are off", () => {
    expect(nextShowConnect(true, true, contactsOff)).toBe(true);
    expect(nextShowConnect(true, true, CONNECTED)).toBe(false);
  });

  it("Skip for now lets it yield once notifications work", () => {
    expect(canSkipSwitches(contactsOff)).toBe(true);
    expect(canSkipSwitches(CONNECTED)).toBe(false);
    expect(canSkipSwitches({ ...contactsOff, services: { ...contactsOff.services, notifications: false } })).toBe(false);
    expect(connectDone(contactsOff, true)).toBe(true);
    expect(nextShowConnect(true, true, contactsOff, true)).toBe(false);
  });

  it("a returning user's reconnect never opens the panel over optional switches", () => {
    expect(nextShowConnect(false, true, contactsOff)).toBe(false);
  });
});

describe("nextShowConnect", () => {
  it("engages on a fresh install once the real status is known", () => {
    expect(nextShowConnect(false, true, FRESH)).toBe(true);
  });

  it("never engages before the real status arrives (the placeholder has no device)", () => {
    expect(nextShowConnect(false, false, FRESH)).toBe(false);
  });

  it("leaves an upgrader's reconnecting phone on the Feed (device present, not done)", () => {
    const reconnecting = { ...CONNECTED, connection: "connecting" as const, services: { ...CONNECTED.services, notifications: false } };
    expect(nextShowConnect(false, true, reconnecting)).toBe(false);
  });

  it("holds the panel up through pairing and the switches until notifications work", () => {
    const paired = { ...FRESH, device: { id: "a", name: "Dave's iPhone" }, connection: "connecting" as const };
    expect(nextShowConnect(true, true, paired)).toBe(true);
  });

  it("yields to the Feed the moment notifications are working", () => {
    expect(nextShowConnect(true, true, CONNECTED)).toBe(false);
  });

  it("re-engages after Start over clears the device", () => {
    expect(nextShowConnect(false, true, FRESH)).toBe(true);
  });
});

describe("shouldWatchSwitches", () => {
  it("watches while the panel is on screen with a switch still off", () => {
    expect(shouldWatchSwitches({ panelVisible: true, fresh: false, switchesPending: true })).toBe(true);
  });

  it("stops once every switch is on, even with the panel open", () => {
    expect(shouldWatchSwitches({ panelVisible: true, fresh: true, switchesPending: false })).toBe(false);
  });

  it("watches during the fresh window even when the panel is hidden (tug in the tray)", () => {
    expect(shouldWatchSwitches({ panelVisible: false, fresh: true, switchesPending: true })).toBe(true);
  });

  it("does not watch when the panel is away and the fresh window has passed", () => {
    expect(shouldWatchSwitches({ panelVisible: false, fresh: false, switchesPending: true })).toBe(false);
  });
});

describe("rescanDue", () => {
  it("re-inquires once the interval has passed while the find step is up", () => {
    expect(rescanDue(true, RESCAN_INTERVAL_MS)).toBe(true);
    expect(rescanDue(true, RESCAN_INTERVAL_MS + 1000)).toBe(true);
  });

  it("waits until the interval has elapsed", () => {
    expect(rescanDue(true, RESCAN_INTERVAL_MS - 1)).toBe(false);
    expect(rescanDue(true, 0)).toBe(false);
  });

  it("never re-inquires away from the find step (a phone is chosen)", () => {
    expect(rescanDue(false, RESCAN_INTERVAL_MS * 10)).toBe(false);
  });
});

describe("optionalNudge", () => {
  it("is empty until notifications are working", () => {
    expect(optionalNudge(FRESH)).toEqual([]);
    const connecting = { ...CONNECTED, connection: "connecting" as const, services: { ...CONNECTED.services, notifications: false } };
    expect(optionalNudge(connecting)).toEqual([]);
  });

  it("names the optional switches that are off, never the required notifications one", () => {
    const s = {
      ...CONNECTED,
      services: { ...CONNECTED.services, messages: false },
      messagesError: "the iPhone refused message access; turn on Show Notifications for this PC",
      contactsError: "the iPhone refused contact access",
      contactsShared: false,
    };
    const nudge = optionalNudge(s);
    expect(nudge.map((x) => x.key)).toEqual(["messages", "contacts"]);
    expect(nudge.every((x) => !x.required)).toBe(true);
  });

  it("is empty when every optional switch is already on", () => {
    expect(optionalNudge(CONNECTED)).toEqual([]);
  });
});
