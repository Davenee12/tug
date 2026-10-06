import { describe, expect, it } from "vitest";
import { knownIdentifiers, phoneModel } from "./phoneModel";

describe("phoneModel", () => {
  it.each([
    ["iPhone10,1", "iPhone 8", "homeButton", "regular"],
    ["iPhone10,5", "iPhone 8 Plus", "homeButton", "large"],
    ["iPhone10,3", "iPhone X", "notch", "regular"],
    ["iPhone10,6", "iPhone X", "notch", "regular"],
    ["iPhone11,8", "iPhone XR", "notch", "regular"],
    ["iPhone12,8", "iPhone SE", "homeButton", "regular"],
    ["iPhone13,1", "iPhone 12 mini", "notch", "mini"],
    ["iPhone14,4", "iPhone 13 mini", "notch", "mini"],
    ["iPhone14,6", "iPhone SE", "homeButton", "regular"],
    ["iPhone14,8", "iPhone 14 Plus", "notch", "large"],
    ["iPhone15,2", "iPhone 14 Pro", "island", "regular"],
    ["iPhone16,2", "iPhone 15 Pro Max", "island", "large"],
    ["iPhone17,3", "iPhone 16", "island", "regular"],
    ["iPhone17,5", "iPhone 16e", "notch", "regular"],
    ["iPhone18,1", "iPhone 17 Pro", "island", "regular"],
    ["iPhone18,2", "iPhone 17 Pro Max", "island", "large"],
    ["iPhone18,3", "iPhone 17", "island", "regular"],
    ["iPhone18,4", "iPhone Air", "island", "large"],
    ["iPhone18,5", "iPhone 17e", "notch", "regular"],
  ])("%s is the %s", (id, name, face, size) => {
    expect(phoneModel(id)).toEqual({ name, face, size, known: true });
  });

  it("falls back to a generic iPhone with the newest design for unknown or missing models", () => {
    const generic = { name: "iPhone", face: "island", size: "regular", known: false };
    expect(phoneModel("iPhone19,1")).toEqual(generic);
    expect(phoneModel("iPhone9,1")).toEqual(generic);
    expect(phoneModel("iPad13,1")).toEqual(generic);
    expect(phoneModel("")).toEqual(generic);
    expect(phoneModel(null)).toEqual(generic);
    expect(phoneModel(undefined)).toEqual(generic);
  });

  it("ignores stray whitespace around the identifier", () => {
    expect(phoneModel(" iPhone16,1 ").name).toBe("iPhone 15 Pro");
  });

  it("only uses home buttons on the 8 family and SE, and the island from the 14 Pro on", () => {
    for (const id of knownIdentifiers()) {
      const m = phoneModel(id);
      if (m.face === "homeButton") expect(m.name).toMatch(/^iPhone (8|SE)/);
      expect(m.name).toMatch(/^iPhone /);
    }
    expect(phoneModel("iPhone14,7").face).toBe("notch"); // iPhone 14 kept the notch
    expect(phoneModel("iPhone15,4").face).toBe("island"); // iPhone 15 brought the island to all
  });

  it("keys every entry by a well-formed identifier (the exact string the phone reports)", () => {
    for (const id of knownIdentifiers()) expect(id).toMatch(/^iPhone\d+,\d+$/);
  });
});
