import { describe, expect, it } from "vitest";
import { TUGBOAT_OFF, HELP_AFTER_MS, percent, showConnectHelp, skippedMessage, transferring } from "./tugboat";

describe("Tugboat panel decisions", () => {
  it("offers connection help only after 30 s with no phone", () => {
    const waiting = { phase: "waiting" as const };
    expect(showConnectHelp(waiting, 1000, 1000 + HELP_AFTER_MS - 1)).toBe(false);
    expect(showConnectHelp(waiting, 1000, 1000 + HELP_AFTER_MS)).toBe(true);
    expect(showConnectHelp({ phase: "connected" }, 1000, 1000 + 10 * HELP_AFTER_MS)).toBe(false);
    expect(showConnectHelp(waiting, null, 1e12)).toBe(false);
  });

  it("knows when files are still arriving", () => {
    expect(transferring(TUGBOAT_OFF)).toBe(false);
    const file = { id: "a", name: "a.jpg", size: 10, received: 4, done: false, path: null, at: 0 };
    expect(transferring({ incoming: [file] })).toBe(true);
    expect(transferring({ incoming: [{ ...file, received: 10, done: true }] })).toBe(false);
  });

  it("explains skipped files in one sentence", () => {
    expect(skippedMessage([])).toBeNull();
    expect(skippedMessage([{ name: "film.mkv", reason: "tooBig" }])).toBe("“film.mkv” is over 1 GB, too big to send to a phone's browser.");
    expect(skippedMessage([{ name: "Photos", reason: "folder" }])).toMatch(/is a folder/);
    expect(skippedMessage([{ name: "x", reason: "unreadable" }])).toBe("Couldn't read “x”.");
    expect(
      skippedMessage([
        { name: "a", reason: "tooBig" },
        { name: "b", reason: "tooBig" },
      ]),
    ).toBe("2 items weren't added: they're over 1 GB.");
    expect(
      skippedMessage([
        { name: "a", reason: "tooBig" },
        { name: "b", reason: "folder" },
      ]),
    ).toMatch(/^2 items weren't added: folders/);
  });

  it("computes progress", () => {
    expect(percent(0, 0)).toBe(100);
    expect(percent(5, 10)).toBe(50);
    expect(percent(11, 10)).toBe(100);
  });
});
