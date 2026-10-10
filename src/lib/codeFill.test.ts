import { describe, expect, it } from "vitest";
import {
  CODE_FILL_MAX_AGE_MS,
  CODE_HOTKEYS,
  CODE_LIVE_MS,
  DEFAULT_CODE_HOTKEY,
  copiedTitle,
  copiedToastSpec,
  fillFeedback,
  hotkeyAlternatives,
  hotkeyKeys,
  hotkeyLabel,
  knownHotkey,
  rememberCopied,
  shouldAutoCopy,
} from "./codeFill";
import shared from "./codeHotkeys.json";
import type { ToastSpec } from "../types/protocol";

const NOW = Date.parse("2026-10-10T12:00:00");
const min = 60_000;

function candidate(over: Partial<Parameters<typeof shouldAutoCopy>[0]> = {}) {
  return { code: "482193", receivedAt: NOW - 2_000, now: NOW, replayed: false, copied: new Map<string, number>(), ...over };
}

describe("shouldAutoCopy", () => {
  it("copies a live, fresh code", () => {
    expect(shouldAutoCopy(candidate())).toBe(true);
  });

  it("never copies without a code from findCode", () => {
    expect(shouldAutoCopy(candidate({ code: null }))).toBe(false);
  });

  it("never copies a replay (reconnect backlog or a gap arrival)", () => {
    expect(shouldAutoCopy(candidate({ replayed: true }))).toBe(false);
  });

  it("never copies a code older than 10 minutes, even with a longer live window", () => {
    const old = NOW - CODE_FILL_MAX_AGE_MS - 1;
    expect(shouldAutoCopy(candidate({ receivedAt: old }))).toBe(false);
    expect(shouldAutoCopy(candidate({ receivedAt: old, liveMs: 60 * min }))).toBe(false);
    expect(shouldAutoCopy(candidate({ receivedAt: NOW - CODE_FILL_MAX_AGE_MS }))).toBe(true);
  });

  it("holds a text to the live window, so catch-up backlog isn't copied", () => {
    expect(shouldAutoCopy(candidate({ receivedAt: NOW - 3 * min, liveMs: CODE_LIVE_MS }))).toBe(false);
    expect(shouldAutoCopy(candidate({ receivedAt: NOW - 1 * min, liveMs: CODE_LIVE_MS }))).toBe(true);
  });

  it("doesn't copy the same code twice (notification, then the text) within 10 minutes", () => {
    const copied = new Map<string, number>();
    rememberCopied(copied, "482193", NOW - 30_000);
    expect(shouldAutoCopy(candidate({ copied }))).toBe(false);
    expect(shouldAutoCopy(candidate({ copied, code: "771204" }))).toBe(true);
    // Long after, the same digits are a new code.
    expect(shouldAutoCopy(candidate({ copied, now: NOW + CODE_FILL_MAX_AGE_MS, receivedAt: NOW + CODE_FILL_MAX_AGE_MS }))).toBe(true);
  });
});

describe("rememberCopied", () => {
  it("forgets codes past the window", () => {
    const copied = new Map<string, number>();
    rememberCopied(copied, "111111", NOW);
    rememberCopied(copied, "222222", NOW + CODE_FILL_MAX_AGE_MS);
    expect([...copied.keys()]).toEqual(["222222"]);
  });
});

describe("the copied pop-up", () => {
  const spec: ToastSpec = {
    id: 7,
    title: "Messages · Chase",
    body: "Your code is 482193",
    name: "Chase",
    replyTo: null,
    markRead: true,
    code: "482193",
    callBack: false,
    clear: true,
  };

  it("says what was copied and from whom", () => {
    expect(copiedTitle("482193", "Chase")).toBe("482193 copied, from Chase");
    expect(copiedTitle("482193", "  ")).toBe("482193 copied");
  });

  it("drops the Copy button but keeps the rest (Open is the body)", () => {
    const out = copiedToastSpec(spec, "482193", "Chase");
    expect(out).toEqual({ ...spec, title: "482193 copied, from Chase", code: null });
  });
});

describe("shortcuts", () => {
  it("lists the shared shortcuts, the first as the default", () => {
    expect(CODE_HOTKEYS).toEqual(shared.hotkeys);
    expect(DEFAULT_CODE_HOTKEY).toBe("ctrl+shift+v");
    expect(new Set(CODE_HOTKEYS).size).toBe(CODE_HOTKEYS.length);
  });

  it("falls back to the default for anything tug doesn't offer", () => {
    expect(knownHotkey("ctrl+alt+v")).toBe("ctrl+alt+v");
    expect(knownHotkey("ctrl+v")).toBe(DEFAULT_CODE_HOTKEY);
    expect(knownHotkey(undefined)).toBe(DEFAULT_CODE_HOTKEY);
  });

  it("names keys for people", () => {
    expect(hotkeyKeys("ctrl+shift+v")).toEqual(["Ctrl", "Shift", "V"]);
    expect(hotkeyLabel("ctrl+shift+alt+v")).toBe("Ctrl+Shift+Alt+V");
  });

  it("offers the others when one is taken", () => {
    const alts = hotkeyAlternatives("ctrl+shift+v");
    expect(alts).not.toContain("ctrl+shift+v");
    expect(alts).toHaveLength(CODE_HOTKEYS.length - 1);
  });
});

describe("fillFeedback", () => {
  it("words each outcome without the code", () => {
    expect(fillFeedback({ outcome: "typed", from: "Chase" }, "ctrl+shift+v")).toEqual({ title: "Code typed", body: "From Chase" });
    expect(fillFeedback({ outcome: "typed", from: null }, "ctrl+shift+v").body).toBe("");
    expect(fillFeedback({ outcome: "noCode", from: null }, "ctrl+shift+v").title).toBe("No recent code");
    expect(fillFeedback({ outcome: "noTarget", from: null }, "ctrl+alt+c").body).toBe("Then press Ctrl+Alt+C again.");
    expect(fillFeedback({ outcome: "failed", from: null }, "ctrl+shift+v").title).toBe("Couldn't type the code");
  });
});
