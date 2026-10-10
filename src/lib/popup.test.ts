import { describe, expect, it } from "vitest";
import { inQuietHours, shouldPopUp, type PopupEvent } from "./popup";
import type { QuietHours, UiSettings } from "../types/protocol";

const baseSettings: UiSettings = {
  toasts: true,
  doNotDisturb: false,
  mutedApps: [],
  quietHours: { enabled: false, start: "22:00", end: "07:00", days: [] },
  vips: [],
  muteCalls: false,
  closeToTray: true,
  appIcons: true,
  lowBattery: true,
  popupSound: true,
  dialing: false,
  filterUnknown: true,
  knownSenders: [],
  autoCopyCodes: true,
  typeCodeHotkey: true,
  typeCodeKeys: "ctrl+shift+v",
};
const settings = (over: Partial<UiSettings> = {}): UiSettings => ({ ...baseSettings, ...over });
const quiet = (over: Partial<QuietHours> = {}): QuietHours => ({ enabled: true, start: "22:00", end: "07:00", days: [], ...over });

// 2026-10-05 is a Monday.
const at = (iso: string) => new Date(iso);
const event = (over: Partial<PopupEvent> = {}): PopupEvent => ({ appId: "com.apple.MobileSMS", isCall: false, isVip: false, ...over });

describe("inQuietHours", () => {
  it("is off when disabled", () => {
    expect(inQuietHours(quiet({ enabled: false }), at("2026-10-05T23:00"))).toBe(false);
  });

  it("covers a same-day window", () => {
    const q = quiet({ start: "09:00", end: "17:00" });
    expect(inQuietHours(q, at("2026-10-05T12:00"))).toBe(true);
    expect(inQuietHours(q, at("2026-10-05T08:59"))).toBe(false);
    expect(inQuietHours(q, at("2026-10-05T17:00"))).toBe(false); // end is exclusive
  });

  it("covers an overnight window across midnight", () => {
    const q = quiet({ start: "22:00", end: "07:00" });
    expect(inQuietHours(q, at("2026-10-05T23:30"))).toBe(true); // Mon evening
    expect(inQuietHours(q, at("2026-10-06T06:30"))).toBe(true); // Tue morning (same window)
    expect(inQuietHours(q, at("2026-10-06T07:30"))).toBe(false);
    expect(inQuietHours(q, at("2026-10-05T21:30"))).toBe(false);
  });

  it("respects the chosen days, with the morning belonging to the night it started", () => {
    // Monday only (day 1).
    const q = quiet({ start: "22:00", end: "07:00", days: [1] });
    expect(inQuietHours(q, at("2026-10-05T23:30"))).toBe(true); // Mon night
    expect(inQuietHours(q, at("2026-10-06T06:30"))).toBe(true); // into Tue morning
    expect(inQuietHours(q, at("2026-10-06T23:30"))).toBe(false); // Tue night is not selected
    expect(inQuietHours(q, at("2026-10-07T06:30"))).toBe(false);
  });

  it("ignores a zero-length or unparseable window", () => {
    expect(inQuietHours(quiet({ start: "08:00", end: "08:00" }), at("2026-10-05T08:00"))).toBe(false);
    expect(inQuietHours(quiet({ start: "nope", end: "07:00" }), at("2026-10-05T03:00"))).toBe(false);
  });
});

describe("shouldPopUp", () => {
  it("never pops up when Windows pop-ups are off", () => {
    expect(shouldPopUp(event(), settings({ toasts: false }), at("2026-10-05T12:00"))).toBe(false);
    expect(shouldPopUp(event({ isCall: true, isVip: true }), settings({ toasts: false }), at("2026-10-05T12:00"))).toBe(false);
  });

  it("holds muted apps, even for a VIP", () => {
    const s = settings({ mutedApps: ["com.whatsapp"] });
    expect(shouldPopUp(event({ appId: "com.whatsapp" }), s, at("2026-10-05T12:00"))).toBe(false);
    expect(shouldPopUp(event({ appId: "com.whatsapp", isVip: true }), s, at("2026-10-05T12:00"))).toBe(false);
  });

  it("holds everything during Do not disturb, except VIPs", () => {
    const s = settings({ doNotDisturb: true });
    expect(shouldPopUp(event(), s, at("2026-10-05T12:00"))).toBe(false);
    expect(shouldPopUp(event({ isVip: true }), s, at("2026-10-05T12:00"))).toBe(true);
  });

  it("holds everything during quiet hours, except VIPs", () => {
    const s = settings({ quietHours: quiet({ start: "22:00", end: "07:00" }) });
    expect(shouldPopUp(event(), s, at("2026-10-05T23:30"))).toBe(false);
    expect(shouldPopUp(event({ isVip: true }), s, at("2026-10-05T23:30"))).toBe(true);
    expect(shouldPopUp(event(), s, at("2026-10-05T12:00"))).toBe(true); // outside the window
  });

  it("rings calls through quiet hours and DND unless calls are muted", () => {
    const s = settings({ doNotDisturb: true, quietHours: quiet() });
    expect(shouldPopUp(event({ isCall: true }), s, at("2026-10-05T23:30"))).toBe(true);
    const muted = settings({ muteCalls: true, doNotDisturb: true });
    expect(shouldPopUp(event({ isCall: true }), muted, at("2026-10-05T12:00"))).toBe(false);
    // A VIP still rings even when calls are muted.
    expect(shouldPopUp(event({ isCall: true, isVip: true }), muted, at("2026-10-05T12:00"))).toBe(true);
  });

  it("mutes calls at any time, not just in quiet hours or DND (as Settings says)", () => {
    const muted = settings({ muteCalls: true });
    expect(shouldPopUp(event({ isCall: true }), muted, at("2026-10-05T12:00"))).toBe(false);
    expect(shouldPopUp(event({ isCall: true, isVip: true }), muted, at("2026-10-05T12:00"))).toBe(true);
    // Texts are unaffected by Mute calls.
    expect(shouldPopUp(event(), muted, at("2026-10-05T12:00"))).toBe(true);
  });
});
