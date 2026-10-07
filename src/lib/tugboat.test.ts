import { describe, expect, it } from "vitest";
import { TUGBOAT_OFF, HELP_AFTER_MS, capitalised, closeWarning, percent, showConnectHelp, skippedMessage, transferring } from "./tugboat";
import type { TugboatOffer } from "../types/protocol";

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
    expect(transferring({ incoming: [file], sending: false })).toBe(true);
    expect(transferring({ incoming: [{ ...file, received: 10, done: true }], sending: false })).toBe(false);
    // The phone downloading from the PC counts too.
    expect(transferring({ incoming: [], sending: true })).toBe(true);
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

describe("closing Tugboat", () => {
  const offer = (downloads: number) => ({ id: "o1", name: "a.jpg", size: 10, downloads }) as unknown as TugboatOffer;

  it("just closes with nothing pending", () => {
    expect(closeWarning(TUGBOAT_OFF)).toBeNull();
    expect(closeWarning({ ...TUGBOAT_OFF, outgoing: [offer(1)] })).toBeNull();
  });

  it("asks while files are moving", () => {
    expect(closeWarning({ ...TUGBOAT_OFF, sending: true, outgoing: [offer(0)] })).toBe("moving");
  });

  it("asks when an offered file hasn't been saved on the phone yet", () => {
    expect(closeWarning({ ...TUGBOAT_OFF, outgoing: [offer(1), offer(0)] })).toBe("unsaved");
  });

  it("capitalises a phone name for a headline", () => {
    expect(capitalised("phone")).toBe("Phone");
    expect(capitalised("Android phone")).toBe("Android phone");
    expect(capitalised("iPhone")).toBe("iPhone");
  });
});
