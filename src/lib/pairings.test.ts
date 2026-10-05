import { describe, expect, it } from "vitest";
import { hasDuplicateIphones, pairedPhones, pairingProblem } from "./pairings";
import type { DiscoveredDevice } from "../types/protocol";

function dev(overrides: Partial<DiscoveredDevice>): DiscoveredDevice {
  return {
    id: "id",
    name: "Dave's iPhone",
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
      dev({ id: "le", transport: "le", name: "Dave's iPhone" }),
      dev({ id: "classic", transport: "classic", name: "Dave's iPhone" }),
    ];
    expect(hasDuplicateIphones(list)).toBe(false);
  });

  it("flags two differently-named iPhones", () => {
    const list = [
      dev({ id: "old", name: "DTD iPhone Max 15 Pro" }),
      dev({ id: "new", name: "Dave's iPhone" }),
    ];
    expect(hasDuplicateIphones(list)).toBe(true);
  });

  it("ignores unnamed entries so a nameless just-connected phone doesn't count", () => {
    const list = [dev({ id: "a", name: "Dave's iPhone" }), dev({ id: "b", name: "Unnamed device" })];
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
    const list = [dev({ id: "new", name: "Dave's iPhone" })];
    expect(pairingProblem(list, "old-gone")).toBe("remembered-missing");
  });

  it("reports duplicates when two iPhones are paired and none is remembered", () => {
    const list = [dev({ id: "old", name: "DTD iPhone Max 15 Pro" }), dev({ id: "new", name: "Dave's iPhone" })];
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
