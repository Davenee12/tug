import { describe, expect, it } from "vitest";
import { bondHint, hasDuplicateIphones, pairedPhones, pairingProblem, setupDeviceLists, startedOutsideTug } from "./pairings";
import type { DeviceStatus, DiscoveredDevice } from "../types/protocol";

const CONNECTED: DeviceStatus = {
  radio: "on",
  peripheralSupported: true,
  advertising: "on",
  device: { id: "x", name: "Jordan's iPhone" },
  connection: "connected",
  battery: 76,
  services: { notifications: true, media: true, battery: true, messages: true },
  lastError: null,
  lastErrorAt: null,
  pairingStale: false,
  awaitingPhoneAllow: false,
  messagesError: null,
  contactsError: null,
  textsPairing: "ok",
  textsDevice: "Jordan's iPhone",
  liveTexts: "off",
};
const status = (o: Partial<DeviceStatus>): DeviceStatus => ({ ...CONNECTED, ...o });

function dev(overrides: Partial<DiscoveredDevice>): DiscoveredDevice {
  return {
    id: "id",
    name: "Jordan's iPhone",
    transport: "le",
    paired: true,
    connected: false,
    canPair: false,
    kind: "phone",
    ...overrides,
  };
}

describe("pairedPhones", () => {
  it("keeps only paired, phone-kind devices", () => {
    const list = [
      dev({ id: "a" }),
      dev({ id: "b", paired: false }),
      dev({ id: "c", kind: "accessory", name: "Keychron" }),
      dev({ id: "d", kind: "unknown", name: "Unnamed device" }),
    ];
    expect(pairedPhones(list).map((d) => d.id)).toEqual(["a"]);
  });
});

describe("hasDuplicateIphones", () => {
  it("counts the LE and Classic sides of one phone as one", () => {
    const list = [
      dev({ id: "le", transport: "le", name: "Jordan's iPhone" }),
      dev({ id: "classic", transport: "classic", name: "Jordan's iPhone" }),
    ];
    expect(hasDuplicateIphones(list)).toBe(false);
  });

  it("flags two differently-named iPhones", () => {
    const list = [
      dev({ id: "old", name: "Old iPhone" }),
      dev({ id: "new", name: "Jordan's iPhone" }),
    ];
    expect(hasDuplicateIphones(list)).toBe(true);
  });

  it("ignores unnamed entries so a nameless just-connected phone doesn't count", () => {
    const list = [dev({ id: "a", name: "Jordan's iPhone" }), dev({ id: "b", name: "Unnamed device" })];
    expect(hasDuplicateIphones(list)).toBe(false);
  });
});

describe("pairingProblem", () => {
  it("is null when nothing is paired", () => {
    expect(pairingProblem([], "id")).toBeNull();
    expect(pairingProblem([dev({ paired: false })], "id")).toBeNull();
  });

  it("is null for one healthy remembered phone (both transports)", () => {
    const list = [dev({ id: "le" }), dev({ id: "classic", transport: "classic" })];
    expect(pairingProblem(list, "le")).toBeNull();
  });

  it("reports the remembered phone missing when a different one is paired", () => {
    const list = [dev({ id: "new", name: "Jordan's iPhone" })];
    expect(pairingProblem(list, "old-gone")).toBe("remembered-missing");
  });

  it("reports duplicates when two iPhones are paired and none is remembered", () => {
    const list = [dev({ id: "old", name: "Old iPhone" }), dev({ id: "new", name: "Jordan's iPhone" })];
    expect(pairingProblem(list, null)).toBe("duplicates");
  });

  it("prefers remembered-missing over duplicates", () => {
    const list = [dev({ id: "old", name: "Old iPhone" }), dev({ id: "new", name: "New iPhone" })];
    expect(pairingProblem(list, "gone")).toBe("remembered-missing");
  });

  it("does not trip when the remembered phone is simply away (nothing paired visible)", () => {
    expect(pairingProblem([dev({ paired: false, id: "x" })], "remembered")).toBeNull();
  });
});

describe("setupDeviceLists", () => {
  it("offers a discoverable iPhone even when it isn't connected to the PC yet", () => {
    // The old LightBlue assumption required `connected`; Dave's phone was discoverable but not
    // connected, so the wizard's list was empty and he tapped the PC from the phone instead.
    const list = [dev({ id: "p", name: "Jordan's iPhone", paired: false, connected: false, kind: "phone" })];
    const { phones } = setupDeviceLists(list);
    expect(phones.map((d) => d.id)).toEqual(["p"]);
  });

  it("keeps an Echo Dot out of the phone offer", () => {
    const list = [
      dev({ id: "phone", name: "Jordan's iPhone", kind: "phone" }),
      dev({ id: "echo", name: "Echo Dot-5TF", kind: "unknown", paired: true }),
    ];
    const lists = setupDeviceLists(list);
    expect(lists.phones.map((d) => d.id)).toEqual(["phone"]);
    expect(lists.others.map((d) => d.id)).toEqual(["echo"]);
  });

  it("hides accessories and counts them", () => {
    const list = [
      dev({ id: "phone", kind: "phone" }),
      dev({ id: "kbd", name: "Keychron", kind: "accessory" }),
      dev({ id: "mouse", name: "MX Master", kind: "accessory" }),
    ];
    const lists = setupDeviceLists(list);
    expect(lists.phones.map((d) => d.id)).toEqual(["phone"]);
    expect(lists.hiddenAccessories).toBe(2);
    expect(lists.others).toEqual([]);
  });

  it("only considers the LE transport", () => {
    const list = [
      dev({ id: "le", transport: "le", kind: "phone" }),
      dev({ id: "classic", transport: "classic", kind: "phone" }),
    ];
    expect(setupDeviceLists(list).phones.map((d) => d.id)).toEqual(["le"]);
  });

  it("puts a connected phone first", () => {
    const list = [
      dev({ id: "away", name: "B iPhone", connected: false, kind: "phone" }),
      dev({ id: "here", name: "A iPhone", connected: true, kind: "phone" }),
    ];
    expect(setupDeviceLists(list).phones.map((d) => d.id)).toEqual(["here", "away"]);
  });
});

describe("startedOutsideTug", () => {
  it("flags an unpaired→paired flip that tug didn't start", () => {
    expect(startedOutsideTug(false, true, false)).toBe(true);
  });

  it("ignores a flip tug started itself", () => {
    expect(startedOutsideTug(false, true, true)).toBe(false);
  });

  it("ignores devices that were already paired or are still unpaired", () => {
    expect(startedOutsideTug(true, true, false)).toBe(false);
    expect(startedOutsideTug(false, false, false)).toBe(false);
    expect(startedOutsideTug(undefined, true, false)).toBe(false);
  });
});

describe("bondHint", () => {
  it("is null for a healthy connected phone", () => {
    expect(bondHint(CONNECTED)).toBeNull();
  });

  it("reports forgotten on a stale-bond signal", () => {
    expect(bondHint(status({ pairingStale: true, connection: "disconnected" }))).toBe("forgotten");
  });

  it("reports maybe when the bond exists but the phone rejects it (0xC3, no notifications)", () => {
    const s = status({
      connection: "disconnected",
      services: { notifications: false, media: false, battery: false, messages: false },
      messagesError: "the iPhone refused message access",
    });
    expect(bondHint(s)).toBe("maybe");
  });

  it("does not cry forgotten when it's just Show Notifications off (LE fine)", () => {
    // Connected, notifications flowing, only the texts switch off: that's not a forgotten bond.
    const s = status({
      services: { notifications: true, media: true, battery: true, messages: false },
      messagesError: "turn on Show Notifications",
    });
    expect(bondHint(s)).toBeNull();
  });

  it("is null before a phone is adopted", () => {
    expect(bondHint(status({ device: null, connection: "noDevice" }))).toBeNull();
  });
});
