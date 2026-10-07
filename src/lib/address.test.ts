import { describe, expect, it } from "vitest";
import { isNanpRegion, normalizeAddress, normalizeAddressIn, regionFromLocale } from "./address";

// Kept in step with the tests in src-tauri/src/map/address.rs. Fictional ranges where they exist:
// UK mobile (Ofcom 07700 900xxx), AU mobile (ACMA 0491 570 xxx), FR mobile (ARCEP 06 39 98 xx xx);
// IN has no fiction range, so a placeholder.
describe("normalizeAddressIn", () => {
  it("reads North American numbers as +1 on a PC in a +1 region", () => {
    for (const region of ["US", "CA", "pr"]) {
      for (const raw of ["+1 (302) 555-0173", "13025550173", "3025550173", "302.555.0173", "+13025550173"]) {
        expect(normalizeAddressIn(raw, region), `${raw} in ${region}`).toBe("+13025550173");
      }
    }
  });

  it("keeps local numbers elsewhere as typed, for the iPhone to resolve", () => {
    expect(normalizeAddressIn("07700 900123", "GB")).toBe("07700900123");
    expect(normalizeAddressIn("0491 570 006", "AU")).toBe("0491570006");
    expect(normalizeAddressIn("06 39 98 12 34", "FR")).toBe("0639981234");
    expect(normalizeAddressIn("98765 43210", "IN")).toBe("9876543210");
    // A leading 0 is never North American, even on a US PC or with no region known.
    expect(normalizeAddressIn("0491 570 006", "US")).toBe("0491570006");
    expect(normalizeAddressIn("07700 900123", null)).toBe("07700900123");
    // A bare 10-digit number off a +1 PC (or with no region) stays as typed.
    expect(normalizeAddressIn("3025550173", null)).toBe("3025550173");
    expect(normalizeAddressIn("3025550173", "GB")).toBe("3025550173");
    expect(normalizeAddressIn("13025550173", "IN")).toBe("13025550173");
  });

  it("keeps the country code of a number typed with +", () => {
    for (const region of ["US", "GB", "AU", null]) {
      expect(normalizeAddressIn("+44 7700 900123", region)).toBe("+447700900123");
      expect(normalizeAddressIn("+61 491 570 006", region)).toBe("+61491570006");
      expect(normalizeAddressIn("+33 6 39 98 12 34", region)).toBe("+33639981234");
      expect(normalizeAddressIn("+91 98765 43210", region)).toBe("+919876543210");
      expect(normalizeAddressIn("+1 302 555 0173", region)).toBe("+13025550173");
    }
  });

  it("keeps short codes, emails and names", () => {
    expect(normalizeAddressIn("72975", "US")).toBe("72975");
    expect(normalizeAddressIn(" Zoe@Example.com ", "GB")).toBe("zoe@example.com");
    expect(normalizeAddressIn("Unknown", "US")).toBe("Unknown");
  });

  it("uses the PC's region by default (tests run as a US PC, like the Rust side)", () => {
    expect(normalizeAddress("3025550173")).toBe("+13025550173");
    expect(["07700 900123"].map(normalizeAddress)).toEqual(["07700900123"]);
  });
});

describe("regions", () => {
  it("knows the +1 regions", () => {
    expect(isNanpRegion("US") && isNanpRegion("ca") && isNanpRegion("JM")).toBe(true);
    expect(isNanpRegion("GB") || isNanpRegion("AU") || isNanpRegion(null)).toBe(false);
  });

  it("reads the region from a language tag", () => {
    expect(regionFromLocale("en-US")).toBe("US");
    expect(regionFromLocale("en-AU")).toBe("AU");
    expect(regionFromLocale("fr")).toBeNull();
    expect(regionFromLocale("")).toBeNull();
  });
});
