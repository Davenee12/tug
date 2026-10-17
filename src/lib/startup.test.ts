import { describe, expect, it } from "vitest";
import { retryDelay, settleAll, START_FAILED, STILL_LOADING, type StartupRead } from "./startup";

describe("settleAll", () => {
  it("applies every read that works even when another fails, and returns the failures", async () => {
    const applied: string[] = [];
    const ok = (name: string): StartupRead => async () => void applied.push(name);
    const bad: StartupRead = async () => {
      throw new Error("ipc down");
    };
    const failed = await settleAll([ok("status"), bad, ok("calls")]);
    expect(applied).toEqual(["status", "calls"]);
    expect(failed).toEqual([bad]);
  });

  it("counts a read that throws before awaiting as failed", async () => {
    const sync = (() => {
      throw new Error("boom");
    }) as unknown as StartupRead;
    expect(await settleAll([sync])).toEqual([sync]);
  });

  it("returns nothing when everything worked", async () => {
    expect(await settleAll([async () => undefined])).toEqual([]);
  });
});

describe("retryDelay", () => {
  it("retries quickly at first, then settles at a steady pace that never stops", () => {
    expect([0, 1, 2, 3, 10, 100].map(retryDelay)).toEqual([2_000, 5_000, 15_000, 30_000, 30_000, 30_000]);
  });
});

describe("startup copy", () => {
  it("speaks to the person, not the developer", () => {
    for (const text of [START_FAILED, STILL_LOADING]) {
      expect(text).not.toMatch(/error|exception|ipc|invoke|undefined|\$\{/i);
    }
  });
});
