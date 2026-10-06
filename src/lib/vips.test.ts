import { describe, expect, it } from "vitest";
import { isVip, vipIndex } from "./vips";
import type { Contact } from "../types/protocol";

const contacts: Contact[] = [
  { address: "+13025550123", name: "Tay" },
  { address: "+13025550111", name: "Sam" },
  { address: "+14155550000", name: "Not a VIP" },
];

describe("vips", () => {
  const index = vipIndex(["+1 (302) 555-0123"], contacts);

  it("matches a VIP by their resolved address, however it's formatted", () => {
    expect(isVip(index, { address: "+13025550123" })).toBe(true);
    expect(isVip(index, { address: "(302) 555-0123" })).toBe(true);
    expect(isVip(index, { address: "+14155550000" })).toBe(false);
  });

  it("matches a VIP contact by the name iOS titles their texts with", () => {
    expect(isVip(index, { name: "Tay" })).toBe(true);
    expect(isVip(index, { name: "tay " })).toBe(true);
    expect(isVip(index, { name: "Sam" })).toBe(false);
    expect(isVip(index, { name: "Not a VIP" })).toBe(false);
  });

  it("treats a number-only title as an address, not a name", () => {
    expect(isVip(index, { name: "+1 302-555-0123" })).toBe(true);
    expect(isVip(index, { name: "+1 415-555-0000" })).toBe(false);
  });

  it("is empty when there are no VIPs", () => {
    const none = vipIndex([], contacts);
    expect(isVip(none, { name: "Tay", address: "+13025550123" })).toBe(false);
  });

  it("matches a VIP by number even when it's not a saved contact", () => {
    const byNumber = vipIndex(["98765"], contacts);
    expect(isVip(byNumber, { address: "98765" })).toBe(true);
    expect(isVip(byNumber, { name: "98765" })).toBe(true);
  });
});
