import { describe, expect, it } from "vitest";
import { CHECKING_MAX_MS, phoneSwitches, switchesOff, switchesPending, type SwitchContext, type SwitchState } from "./phoneSwitches";
import type { DeviceStatus } from "../types/protocol";

// A fully connected, everything-on phone; tests override only what they exercise.
const CONNECTED: DeviceStatus = {
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

const ctx: SwitchContext = { now: 0, connectedSince: 0, messagesSince: 0, savedContacts: 0 };

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

  it("notifications wait (with the reason, not a pulse) until the phone is connected", () => {
    const down = { notifications: false, media: false, battery: false, messages: false };
    const away = phoneSwitches({ ...CONNECTED, connection: "disconnected", services: down })[0];
    expect(away.state).toBe("waiting");
    expect(away.note).toMatch(/once your iPhone is connected/);
    expect(phoneSwitches({ ...CONNECTED, connection: "disconnected", awaitingUnlock: true, services: down })[0].note).toMatch(/Unlock/);
    expect(phoneSwitches({ ...CONNECTED, connection: "connecting", radio: "off", services: down })[0].note).toMatch(/Bluetooth/);
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

  it("Show Notifications checks briefly, then says plainly there's no answer yet", () => {
    const s = { ...CONNECTED, services: { notifications: true, media: true, battery: true, messages: false }, contactsShared: false };
    const at = (ms: number) => phoneSwitches(s, { ...ctx, connectedSince: 0, now: ms })[1];
    expect(at(1_000).state).toBe("checking");
    expect(at(CHECKING_MAX_MS - 1).state).toBe("checking");
    expect(at(CHECKING_MAX_MS).state).toBe("waiting");
    expect(at(10 * 60_000).note).toMatch(/No answer/);
  });

  it("Show Notifications can't be read without a texts pairing: waiting with the fix, never Checking", () => {
    const base = { ...CONNECTED, services: { notifications: true, media: true, battery: true, messages: false }, contactsShared: false };
    expect(phoneSwitches({ ...base, textsPairing: "missing" }, ctx)[1].state).toBe("waiting");
    expect(phoneSwitches({ ...base, textsPairing: "missing" })[1].note).toMatch(/Set up texts/);
    expect(phoneSwitches({ ...base, textsPairing: "broken" })[1].state).toBe("waiting");
  });

  it("Sync Contacts: off on a refusal or repeated empty phonebooks, on when shared", () => {
    expect(states({ contactsError: "the iPhone refused contact access", contactsShared: false }).contacts).toBe("off");
    expect(states({ contactsShared: false, contactsOff: true }).contacts).toBe("off");
    expect(states({ contactsShared: true }).contacts).toBe("on");
  });

  it("Sync Contacts off explains names saved from before (re-pair), and offers Check again", () => {
    const off = { ...CONNECTED, contactsShared: false, contactsOff: true };
    const withNames = phoneSwitches(off, { ...ctx, savedContacts: 214 })[2];
    expect(withNames.note).toBe("Off on your iPhone. The names you see were saved earlier; turn it on to keep them up to date.");
    expect(withNames.recheck).toBe(true);
    expect(phoneSwitches(off, { ...ctx, savedContacts: 0 })[2].note).toMatch(/names instead of numbers/);
  });

  it("Sync Contacts 'Checking…' is bounded: never shows forever (the empty-pull bug)", () => {
    const s = { ...CONNECTED, contactsShared: false, contactsOff: false };
    const at = (ms: number) => phoneSwitches(s, { ...ctx, messagesSince: 0, now: ms })[2];
    expect(at(0).state).toBe("checking");
    expect(at(0).note).toBe("Checking…");
    expect(at(CHECKING_MAX_MS).state).toBe("waiting");
    expect(at(CHECKING_MAX_MS).note).not.toMatch(/Checking/);
    expect(at(CHECKING_MAX_MS).recheck).toBe(true);
    // Hours later it still isn't pulsing.
    expect(at(5 * 3_600_000).state).toBe("waiting");
  });

  it("Sync Contacts waits on the texts connection with a reason, never Checking", () => {
    const base = { ...CONNECTED, contactsShared: false, services: { notifications: true, media: true, battery: true, messages: false } };
    expect(phoneSwitches(base, ctx)[2]).toMatchObject({ state: "waiting", note: "Shows once texts are connected." });
    expect(phoneSwitches({ ...base, textsPairing: "missing" }, ctx)[2].note).toMatch(/texts are set up/);
    expect(phoneSwitches({ ...base, messagesError: "refused" }, ctx)[2].note).toMatch(/Show Notifications/);
  });

  it("turns green the moment the phone shares (one quick pull), from off", () => {
    const off = phoneSwitches({ ...CONNECTED, contactsShared: false, contactsOff: true }, ctx)[2];
    const on = phoneSwitches({ ...CONNECTED, contactsShared: true, contactsOff: false }, ctx)[2];
    expect([off.state, on.state]).toEqual(["off", "on"]);
    expect(on.note).toBe("");
  });

  it("never pulses for an unreachable phone: every switch waits with a reason", () => {
    const s = states({
      connection: "disconnected",
      services: { notifications: false, media: false, battery: false, messages: false },
      messagesError: null,
      contactsError: null,
      contactsShared: false,
      contactsOff: false,
    });
    expect(s).toEqual({ notifications: "waiting", messages: "waiting", contacts: "waiting" });
  });

  it("nothing is ever 'checking' past the bound, in any combination", () => {
    for (const connection of ["connected", "connecting", "disconnected"] as const)
      for (const messages of [true, false])
        for (const textsPairing of ["unknown", "ok", "missing", "broken"] as const) {
          const list = phoneSwitches(
            {
              ...CONNECTED,
              connection,
              textsPairing,
              contactsShared: false,
              services: { notifications: true, media: true, battery: true, messages },
            },
            { now: CHECKING_MAX_MS * 10, connectedSince: 0, messagesSince: 0, savedContacts: 3 },
          );
          for (const x of list) expect(x.state, `${connection}/${messages}/${textsPairing}/${x.key}`).not.toBe("checking");
        }
  });

  it("switchesOff lists exactly the switches that are off, with a fix", () => {
    const list = phoneSwitches({
      ...CONNECTED,
      services: { notifications: false, media: true, battery: true, messages: true },
      contactsError: "no",
      contactsShared: false,
    });
    const off = switchesOff(list);
    expect(off.map((x) => x.key)).toEqual(["notifications", "contacts"]);
    for (const x of off) expect(x.fix.length).toBeGreaterThan(0);
  });

  it("switchesPending counts Sync Contacts off even with names saved (contacts.length missed it)", () => {
    expect(switchesPending(phoneSwitches(CONNECTED))).toBe(false);
    expect(switchesPending(phoneSwitches({ ...CONNECTED, contactsShared: false, contactsOff: true }, { ...ctx, savedContacts: 214 }))).toBe(true);
  });
});
