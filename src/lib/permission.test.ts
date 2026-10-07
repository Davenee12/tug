import { describe, expect, it, vi } from "vitest";
import { grantedOnceCache } from "./permission";

describe("grantedOnceCache", () => {
  it("asks once when granted, then remembers it", async () => {
    const check = vi.fn(async () => true);
    const has = grantedOnceCache(check);
    expect(await has()).toBe(true);
    expect(await has()).toBe(true);
    expect(check).toHaveBeenCalledTimes(1);
  });

  it("asks again after a refusal, so turning notifications on in Windows works without a restart", async () => {
    const answers = [false, true];
    const check = vi.fn(async () => answers.shift() ?? true);
    const has = grantedOnceCache(check);
    expect(await has()).toBe(false);
    expect(await has()).toBe(true);
    expect(await has()).toBe(true);
    expect(check).toHaveBeenCalledTimes(2);
  });
});
