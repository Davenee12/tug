import { describe, expect, it } from "vitest";
import {
  afterQuickSend,
  canQuickSend,
  DEFAULT_QUICK_TEXT_HOTKEY,
  knownQuickTextHotkey,
  QUICK_TEXT_HOTKEYS,
  quickTextAlternatives,
  quickTextShortcut,
} from "./quickText";
import { CODE_HOTKEYS } from "./codeFill";
import voiceHotkeys from "./voiceHotkeys.json";

describe("the quick text shortcuts", () => {
  it("never offers Ctrl+Alt (AltGr), a well-known combo, or another tug shortcut", () => {
    expect(QUICK_TEXT_HOTKEYS.length).toBeGreaterThan(1);
    for (const h of QUICK_TEXT_HOTKEYS) {
      const keys = h.split("+");
      expect(keys).toContain("ctrl");
      expect(keys.includes("ctrl") && keys.includes("alt")).toBe(false);
      expect(h).not.toBe("ctrl+shift+t");
      expect(CODE_HOTKEYS).not.toContain(h);
      expect(voiceHotkeys.hotkeys).not.toContain(h);
    }
  });

  it("falls back to the default for a saved shortcut it doesn't offer", () => {
    expect(DEFAULT_QUICK_TEXT_HOTKEY).toBe(QUICK_TEXT_HOTKEYS[0]);
    expect(knownQuickTextHotkey(null)).toBe(DEFAULT_QUICK_TEXT_HOTKEY);
    expect(knownQuickTextHotkey("ctrl+shift+v")).toBe(DEFAULT_QUICK_TEXT_HOTKEY);
    expect(knownQuickTextHotkey(QUICK_TEXT_HOTKEYS[1])).toBe(QUICK_TEXT_HOTKEYS[1]);
  });

  it("offers the others when one is taken", () => {
    const [first, ...rest] = QUICK_TEXT_HOTKEYS;
    expect(quickTextAlternatives(first)).toEqual(rest);
  });

  it("is listed under Keyboard only while it's registered", () => {
    expect(quickTextShortcut({ state: "on", keys: "ctrl+shift+y" })).toEqual({
      what: "Text someone (from any app)",
      keys: ["Ctrl", "Shift", "Y"],
    });
    expect(quickTextShortcut({ state: "off", keys: "ctrl+shift+y" })).toBeNull();
    expect(quickTextShortcut({ state: "inUse", keys: "ctrl+shift+y" })).toBeNull();
  });
});

describe("afterQuickSend", () => {
  it("gets out of the way only once the phone took the text", () => {
    expect(afterQuickSend("sent")).toBe("giveBack");
    // Recorded, but not known to be sent: tug stays and shows the conversation, which says so.
    expect(afterQuickSend("unconfirmed")).toBe("showConversation");
    expect(afterQuickSend("failed")).toBe("showConversation");
    // Nothing recorded: the composer keeps the text.
    expect(afterQuickSend("error")).toBe("keep");
  });
});

describe("canQuickSend", () => {
  const ok = { to: "+13025550101", text: "omw", unavailable: null, sending: false };
  it("needs someone, some text, texts up, and no send under way", () => {
    expect(canQuickSend(ok)).toBe(true);
    expect(canQuickSend({ ...ok, to: null })).toBe(false);
    expect(canQuickSend({ ...ok, text: "  \n" })).toBe(false);
    expect(canQuickSend({ ...ok, unavailable: "You can reply once your iPhone reconnects." })).toBe(false);
    expect(canQuickSend({ ...ok, sending: true })).toBe(false);
  });
});
