import { describe, expect, it } from "vitest";
import {
  bondHint,
  hasDuplicateIphones,
  leftoverPhone,
  pairedPhones,
  pairingProblem,
  setupDeviceLists,
  startedOutsideTug,
} from "./pairings";
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
  awaitingUnlock: false,
  messagesError: null,
  contactsError: null,
  contactsShared: false,
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

  it("offers a discoverable Classic iPhone (the freshly-forgotten case)", () => {
    // A phone that forgot this PC is discoverable over Classic inquiry with its real name, but
    // unpaired; its LE adverts are anonymous. The wizard must still offer it.
    const list = [
      dev({ id: "classic", transport: "classic", name: "Jordan's iPhone", kind: "phone", paired: false, canPair: true }),
    ];
    expect(setupDeviceLists(list).phones.map((d) => d.id)).toEqual(["classic"]);
  });

  it("shows one row when a phone appears on both transports (LE wins)", () => {
    const list = [
      dev({ id: "le", transport: "le", name: "Jordan's iPhone", kind: "phone" }),
      dev({ id: "classic", transport: "classic", name: "Jordan's iPhone", kind: "phone" }),
    ];
    expect(setupDeviceLists(list).phones.map((d) => d.id)).toEqual(["le"]);
  });

  it("offers the Classic row for an unpaired iPhone seen on both transports", () => {
    // Dave's fresh setup: two "Jordan's iPhone" rows; the LE one failed to pair (status 19), the
    // Classic one paired and brought the LE bond with it.
    const list = [
      dev({ id: "le", transport: "le", name: "Jordan’s iPhone", kind: "phone", paired: false }),
      dev({ id: "classic", transport: "classic", name: "Jordan’s iPhone", kind: "phone", paired: false }),
    ];
    expect(setupDeviceLists(list).phones.map((d) => d.id)).toEqual(["classic"]);
  });

  it("hides an unrecognised LE entry that is the Classic phone's other side", () => {
    const list = [
      dev({ id: "le", transport: "le", name: "Jordan’s iPhone", kind: "unknown", paired: false }),
      dev({ id: "classic", transport: "classic", name: "Jordan’s iPhone", kind: "phone", paired: false }),
    ];
    const lists = setupDeviceLists(list);
    expect(lists.phones.map((d) => d.id)).toEqual(["classic"]);
    expect(lists.others).toEqual([]);
  });

  it("keeps the LE row when the phone connected over LE (LightBlue)", () => {
    const list = [
      dev({ id: "le", transport: "le", name: "Jordan’s iPhone", kind: "phone", paired: false, connected: true }),
      dev({ id: "classic", transport: "classic", name: "Jordan’s iPhone", kind: "phone", paired: false }),
    ];
    expect(setupDeviceLists(list).phones.map((d) => d.id)).toEqual(["le"]);
  });

  it("keeps a Classic phone with a different name alongside the LE one", () => {
    const list = [
      dev({ id: "le", transport: "le", name: "A iPhone", kind: "phone", connected: true }),
      dev({ id: "classic", transport: "classic", name: "B iPhone", kind: "phone" }),
    ];
    // Connected LE first, then the distinct Classic phone.
    expect(setupDeviceLists(list).phones.map((d) => d.id)).toEqual(["le", "classic"]);
  });

  it("hides Classic accessories and counts them", () => {
    const list = [
      dev({ id: "phone", transport: "classic", name: "Jordan's iPhone", kind: "phone", paired: false, canPair: true }),
      dev({ id: "airpods", transport: "classic", name: "AirPods", kind: "accessory", paired: true }),
    ];
    const lists = setupDeviceLists(list);
    expect(lists.phones.map((d) => d.id)).toEqual(["phone"]);
    expect(lists.hiddenAccessories).toBe(1);
  });

  it("puts a connected phone first", () => {
    const list = [
      dev({ id: "away", name: "B iPhone", connected: false, kind: "phone" }),
      dev({ id: "here", name: "A iPhone", connected: true, kind: "phone" }),
    ];
    expect(setupDeviceLists(list).phones.map((d) => d.id)).toEqual(["here", "away"]);
  });
});

describe("leftoverPhone", () => {
  it("surfaces an iPhone still paired from a previous install when nothing is remembered", () => {
    const list = [
      dev({ id: "le", transport: "le", name: "Jordan's iPhone" }),
      dev({ id: "classic", transport: "classic", name: "Jordan's iPhone" }),
    ];
    // The LE side is offered (notifications adopt the LE bond).
    expect(leftoverPhone(list, null)?.id).toBe("le");
  });

  it("returns the Classic side when that's the only paired bond", () => {
    const list = [dev({ id: "classic", transport: "classic", name: "Jordan's iPhone" })];
    expect(leftoverPhone(list, null)?.id).toBe("classic");
  });

  it("stays quiet once tug remembers a phone of its own", () => {
    const list = [dev({ id: "le", name: "Jordan's iPhone" })];
    expect(leftoverPhone(list, "le")).toBeNull();
  });

  it("is null when nothing is paired (only a fresh, unpaired phone to pair)", () => {
    const list = [dev({ id: "p", paired: false, name: "Jordan's iPhone" })];
    expect(leftoverPhone(list, null)).toBeNull();
  });

  it("ignores paired accessories", () => {
    const list = [dev({ id: "kbd", kind: "accessory", name: "Keychron" })];
    expect(leftoverPhone(list, null)).toBeNull();
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
