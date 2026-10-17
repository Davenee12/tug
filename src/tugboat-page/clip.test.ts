import { describe, expect, it, vi } from "vitest";
import { ackOutcome, copyWith, isNewClip, needsAck, showClip, type CopyEnv } from "./clip";
import type { PageOffer } from "./client";

const picture: PageOffer = { id: "p", name: "Clipboard picture.png", size: 10, chunkSize: 2 << 20, chunks: 1, type: "image/png" };

describe("a clipboard from the PC", () => {
  it("is new only when its id changes (the PC repeats it until the page says it has it)", () => {
    const incoming = { id: 3, text: "hi", image: null };
    expect(isNewClip(null, incoming)).toBe(true);
    expect(isNewClip({ id: 3 }, incoming)).toBe(false);
    expect(isNewClip({ id: 2 }, incoming)).toBe(true);
    // The PC let go of it after the page said it has it: the page keeps showing what it has.
    expect(isNewClip({ id: 3 }, null)).toBe(false);
  });

  it("tells the PC once text is on screen, and a picture only once it's fetched", () => {
    const text = showClip({ id: 1, text: "221B Example Street", image: null });
    expect(text.phase).toBe("ready");
    expect(needsAck(text)).toBe(true);
    expect(needsAck({ ...text, acked: true })).toBe(false);

    const pic = showClip({ id: 2, text: null, image: picture });
    expect(pic.phase).toBe("loading");
    expect(needsAck(pic)).toBe(false);
    expect(needsAck({ ...pic, phase: "failed" })).toBe(false);
    expect(needsAck({ ...pic, phase: "ready" })).toBe(true);
    expect(needsAck(null)).toBe(false);
  });

  it("knows when to stop telling the PC", () => {
    expect(ackOutcome(null)).toBe("done");
    // Replaced by a newer clipboard, or dropped: nothing more to say about this one.
    expect(ackOutcome("not-found")).toBe("done");
    // A picture the PC hasn't seen fetched yet, or a dropped connection: say it again later.
    expect(ackOutcome("incomplete")).toBe("retry");
    expect(ackOutcome("network")).toBe("retry");
    expect(ackOutcome("stale")).toBe("retry");
    expect(ackOutcome("closed")).toBe("fatal");
    expect(ackOutcome("in-use")).toBe("fatal");
  });
});

describe("copying on a page that isn't a secure context", () => {
  // Tugboat's page over plain HTTP on the LAN: no navigator.clipboard.
  const plainHttp = (execOk: boolean): CopyEnv => ({ secure: false, writeText: undefined, execCopy: vi.fn(() => execOk) });

  it("copies with execCommand inside the tap", async () => {
    const env = plainHttp(true);
    expect(await copyWith("hello", env)).toBe("copied");
    expect(env.execCopy).toHaveBeenCalledWith("hello");
  });

  it("falls back to selecting the text when the browser refuses", async () => {
    expect(await copyWith("hello", plainHttp(false))).toBe("select");
  });

  it("never uses the Clipboard API on a page that isn't secure, even if one is offered", async () => {
    const writeText = vi.fn(async () => undefined);
    const env: CopyEnv = { secure: false, writeText, execCopy: () => true };
    expect(await copyWith("x", env)).toBe("copied");
    expect(writeText).not.toHaveBeenCalled();
  });

  it("uses the Clipboard API on a secure page, and the older way if it's refused", async () => {
    const ok: CopyEnv = { secure: true, writeText: vi.fn(async () => undefined), execCopy: vi.fn(() => false) };
    expect(await copyWith("x", ok)).toBe("copied");
    expect(ok.execCopy).not.toHaveBeenCalled();
    const refused: CopyEnv = {
      secure: true,
      writeText: vi.fn(async () => {
        throw new Error("NotAllowedError");
      }),
      execCopy: vi.fn(() => true),
    };
    expect(await copyWith("x", refused)).toBe("copied");
    expect(refused.execCopy).toHaveBeenCalled();
  });
});
