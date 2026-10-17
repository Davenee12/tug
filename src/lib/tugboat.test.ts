import { describe, expect, it } from "vitest";
import {
  TUGBOAT_OFF,
  HELP_AFTER_MS,
  capitalised,
  clipEventMessage,
  clipLine,
  clipNeedsPanel,
  clipPhone,
  closeWarning,
  codeCaption,
  folderBlockMessage,
  openUnlessAbandoned,
  percent,
  phoneTextMessage,
  showConnectHelp,
  skippedMessage,
  timeLeft,
  transferring,
} from "./tugboat";
import type { TugboatClip, TugboatOffer, TugboatStatus } from "../types/protocol";

describe("openUnlessAbandoned (the game's phone controller)", () => {
  it("keeps it open when it's still wanted", async () => {
    const log: string[] = [];
    const ok = await openUnlessAbandoned(
      async () => void log.push("open"),
      async () => void log.push("close"),
      () => true,
    );
    expect(ok).toBe(true);
    expect(log).toEqual(["open"]);
  });

  it("closes it once the open lands if the game closed while it was opening", async () => {
    const log: string[] = [];
    let wanted = true;
    let finishOpen!: () => void;
    const pending = openUnlessAbandoned(
      () => new Promise<void>((r) => (finishOpen = () => (log.push("open"), r()))),
      async () => void log.push("close"),
      () => wanted,
    );
    wanted = false; // the game was closed mid-open
    finishOpen();
    expect(await pending).toBe(false);
    expect(log).toEqual(["open", "close"]);
  });

  it("a failing close doesn't throw, and a failing open does", async () => {
    await expect(openUnlessAbandoned(async () => {}, async () => Promise.reject("gone"), () => false)).resolves.toBe(false);
    await expect(openUnlessAbandoned(async () => Promise.reject("no network"), async () => {}, () => true)).rejects.toBe("no network");
  });
});

describe("the shared clipboard", () => {
  const clip = (state: TugboatClip["state"], kind: TugboatClip["kind"] = "text"): TugboatClip => ({ id: 1, kind, state, expiresAt: 200_000 });

  it("opens the panel unless the phone's page is open right now", () => {
    expect(clipNeedsPanel({ phase: "connected", phoneActive: true })).toBe(false);
    expect(clipNeedsPanel({ phase: "connected", phoneActive: false })).toBe(true);
    expect(clipNeedsPanel({ phase: "waiting", phoneActive: false })).toBe(true);
    expect(clipNeedsPanel({ phase: "noNetwork", phoneActive: false })).toBe(true);
  });

  it("names the phone, or says iPhone before one has connected", () => {
    expect(clipPhone(null)).toBe("iPhone");
    expect(clipPhone("Android phone")).toBe("Android phone");
  });

  it("only says the clipboard is on the phone once the phone said so", () => {
    expect(clipEventMessage({ kind: "text", state: "delivered" }, "iPhone")).toEqual({ kind: "info", text: "Your clipboard is on your iPhone" });
    expect(clipEventMessage({ kind: "image", state: "delivered" }, null)?.text).toBe("The picture from your clipboard is on your iPhone");
    expect(clipEventMessage({ kind: "text", state: "expired" }, "iPhone")).toEqual({
      kind: "error",
      text: "Your clipboard wasn't sent: Tugboat wasn't opened on your iPhone within 2 minutes.",
    });
    expect(clipEventMessage({ kind: "text", state: "waiting" }, "iPhone")).toBeNull();
  });

  it("describes the last clipboard in the panel, with the time left while it waits", () => {
    const idle = { phone: "iPhone", phoneActive: false };
    expect(clipLine(null, idle, 0)).toBeNull();
    expect(clipLine(clip("waiting"), idle, 200_000 - 102_000)).toBe("Your clipboard is waiting for Tugboat to open on your iPhone · 1:42 left");
    expect(clipLine(clip("waiting"), { phone: "iPhone", phoneActive: true }, 0)).toBe("Sending your clipboard to your iPhone…");
    expect(clipLine(clip("delivered", "image"), idle, 0)).toBe("The picture from your clipboard is on your iPhone.");
    expect(clipLine(clip("expired"), { phone: null, phoneActive: false }, 0)).toBe("Not sent: Tugboat wasn't opened on your iPhone within 2 minutes.");
  });

  it("counts down without going below zero", () => {
    expect(timeLeft(120_000, 0)).toBe("2:00");
    expect(timeLeft(120_000, 119_001)).toBe("0:01");
    expect(timeLeft(120_000, 130_000)).toBe("0:00");
  });

  it("asks for a scan to receive a waiting clipboard", () => {
    expect(codeCaption(clip("waiting"))).toBe("Scan to receive your clipboard on your iPhone.");
    expect(codeCaption(clip("delivered"))).toMatch(/^Scan with your phone's camera/);
    expect(codeCaption(null)).toMatch(/^Scan with your phone's camera/);
  });

  it("confirms text from the phone quietly, and only once it's on the clipboard", () => {
    expect(phoneTextMessage(true, "iPhone")).toEqual({ kind: "info", text: "Copied from your iPhone" });
    expect(phoneTextMessage(false, null)).toEqual({ kind: "error", text: "Text from your phone arrived, but couldn't go on the clipboard" });
  });
});

describe("Tugboat panel decisions", () => {
  it("says when Windows Security refused Pictures, and where files go instead", () => {
    expect(folderBlockMessage({ folderBlock: null })).toBeNull();
    expect(folderBlockMessage({ folderBlock: "fallback" })).toMatch(/going to Downloads\\Tugboat/);
    expect(folderBlockMessage({ folderBlock: "blocked" })).toBe(
      "Windows Security blocked tug from saving to Pictures. Allow tug under Controlled folder access.",
    );
    expect(TUGBOAT_OFF.folderBlock).toBeNull();
  });

  it("offers connection help only after 30 s with no phone", () => {
    const waiting = { phase: "waiting" as const };
    expect(showConnectHelp(waiting, 1000, 1000 + HELP_AFTER_MS - 1)).toBe(false);
    expect(showConnectHelp(waiting, 1000, 1000 + HELP_AFTER_MS)).toBe(true);
    expect(showConnectHelp({ phase: "connected" }, 1000, 1000 + 10 * HELP_AFTER_MS)).toBe(false);
    expect(showConnectHelp(waiting, null, 1e12)).toBe(false);
  });

  it("knows when files are still arriving", () => {
    expect(transferring(TUGBOAT_OFF)).toBe(false);
    expect(transferring({ receiving: true, sending: false })).toBe(true);
    // The phone downloading from the PC counts too.
    expect(transferring({ receiving: false, sending: true })).toBe(true);
  });

  it("never gets stuck 'moving' on an upload the phone abandoned", () => {
    // An unfinished file stays in the list, but with no chunks arriving nothing is moving: closing
    // must not warn "files are still moving" forever.
    const abandoned = { id: "a", name: "a.jpg", size: 10, received: 4, done: false, path: null, at: 0 };
    const status: TugboatStatus = { ...TUGBOAT_OFF, phase: "connected", incoming: [abandoned], receiving: false, sending: false };
    expect(transferring(status)).toBe(false);
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
