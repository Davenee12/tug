import { describe, expect, it } from "vitest";
import { replyUnavailable, TEXTS_RECONNECTING } from "./availability";
import type { DeviceStatus } from "../types/protocol";

const UP: DeviceStatus = {
  radio: "on",
  peripheralSupported: true,
  advertising: "on",
  device: { id: "x", name: "iPhone", model: null },
  connection: "connected",
  battery: 76,
  services: { notifications: true, media: true, battery: true, messages: true },
  lastError: null,
  lastErrorAt: null,
  pairingStale: false,
  awaitingPhoneAllow: false,
  awaitingUnlock: false,
  reconnecting: false,
  messagesError: null,
  contactsError: null,
  contactsShared: true,
  contactsOff: false,
  textsPairing: "ok",
  textsDevice: "iPhone",
  liveTexts: "off",
};
const textsDown = { ...UP, services: { ...UP.services, messages: false } };
// Long enough after connecting that the texts switch isn't still "checking".
const ctx = { now: 10 * 60_000, connectedSince: 0, messagesSince: null, savedContacts: 0 };

describe("replyUnavailable", () => {
  it("says nothing while texts work", () => {
    expect(replyUnavailable({ ...UP, textsWereConnected: true }, ctx)).toBeNull();
  });

  it("calls a drop after texts worked a reconnect (the iPhone closes message access now and then)", () => {
    expect(replyUnavailable({ ...textsDown, textsWereConnected: true }, ctx)).toBe(TEXTS_RECONNECTING);
  });

  it("keeps the setup wording only before texts have ever connected with this phone", () => {
    expect(replyUnavailable(textsDown, ctx)).toMatch(/hasn't opened message access yet/);
  });

  it("puts the phone being away, and the phone's own reason, first", () => {
    expect(replyUnavailable({ ...textsDown, textsWereConnected: true, connection: "disconnected" }, ctx)).toMatch(
      /once your iPhone reconnects/,
    );
    expect(replyUnavailable({ ...textsDown, textsWereConnected: true, messagesError: "Turn on Show Notifications." }, ctx)).toBe(
      "Turn on Show Notifications.",
    );
  });
});
