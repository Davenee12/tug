import { describe, expect, it } from "vitest";
import { DEFAULT_FOCUS, FOCUS_TITLE, FocusSpell, focusSettingsFrom, focusStatus, focusSummary, type FocusSummaryInput, type HeldSpell } from "./focusMode";
import { threadKey } from "./format";
import type { Contact, PhoneNotification, SmsMessage } from "../types/protocol";

const T0 = Date.parse("2026-10-05T12:00:00");
const min = 60_000;
const SMS = "com.apple.MobileSMS";

let nid = 1;
function note(appId: string, title: string, message: string, atMin: number, extra: Partial<PhoneNotification> = {}): PhoneNotification {
  return {
    id: nid++,
    appId,
    appName: null,
    category: "social",
    title,
    subtitle: "",
    message,
    postedAt: null,
    receivedAt: T0 + atMin * min,
    flags: { silent: false, important: false, preExisting: false, positiveAction: false, negativeAction: true },
    positiveLabel: "",
    negativeLabel: "Clear",
    removedAt: null,
    live: true,
    fresh: false,
    ...extra,
  };
}
const text = (title: string, message: string, atMin: number, extra: Partial<PhoneNotification> = {}) =>
  note(SMS, title, message, atMin, { appName: "Messages", ...extra });
const missed = (title: string, atMin: number) =>
  note("com.apple.mobilephone", title, "Missed Call", atMin, { appName: "Phone", category: "missedCall" });

const contacts: Contact[] = [
  { address: "+13025550142", name: "Zoe" },
  { address: "+12145550199", name: "Priya" },
];

/** A focus from 12:00 that held `held`, ended at `endMin`. */
function spellOf(held: PhoneNotification[], endMin = 30): HeldSpell {
  const spell = new FocusSpell();
  spell.start(T0);
  for (const n of held) spell.hold(n);
  const ended = spell.end(T0 + endMin * min);
  if (!ended) throw new Error("no spell");
  return ended;
}

function summary(spell: HeldSpell, notifications: PhoneNotification[], over: Partial<FocusSummaryInput> = {}) {
  return focusSummary({ spell, now: spell.until, notifications, messages: [], contacts, seenAt: () => 0, ...over });
}

describe("focusSettingsFrom", () => {
  it("is off by default, with full screen and meetings ready for when it's turned on", () => {
    expect(focusSettingsFrom(undefined)).toEqual({ enabled: false, fullScreen: true, meetings: true, coding: false });
    expect(focusSettingsFrom("nonsense")).toEqual(DEFAULT_FOCUS);
    expect(focusSettingsFrom("null")).toEqual(DEFAULT_FOCUS);
    expect(focusSettingsFrom(JSON.stringify({ enabled: true, coding: true }))).toEqual({ enabled: true, fullScreen: true, meetings: true, coding: true });
  });
});

describe("FocusSpell", () => {
  it("only holds while a focus is under way, and ends once", () => {
    const spell = new FocusSpell();
    spell.hold(text("Zoe", "before", 0));
    expect(spell.end(T0)).toBeNull();
    spell.start(T0);
    expect(spell.active).toBe(true);
    const n = text("Zoe", "during", 5);
    spell.hold(n);
    spell.start(T0 + 6 * min); // still the same focus
    expect(spell.end(T0 + 10 * min)).toEqual({ since: T0 - 1, until: T0 + 10 * min, held: [n.id] });
    expect(spell.active).toBe(false);
    expect(spell.end(T0 + 11 * min)).toBeNull();
  });

  it("covers a held notification from before the focus (a reconnect's replay)", () => {
    const old = text("Zoe", "sent while the phone was away", -20);
    const ended = spellOf([old]);
    expect(ended.since).toBeLessThan(old.receivedAt);
  });
});

describe("focusSummary", () => {
  it("sums up what was held: texts by person, missed calls, other apps", () => {
    const held = [
      text("Zoe", "are you free?", 2),
      text("Zoe", "call me after", 4),
      text("Priya", "lunch?", 6),
      text("Priya", "or Friday", 7),
      missed("Mum", 9),
    ];
    expect(summary(spellOf(held), held)).toEqual({ title: FOCUS_TITLE, body: "4 texts from 2 people, 1 missed call", total: 5 });
  });

  it("says nothing for a focus that held nothing", () => {
    const n = text("Zoe", "arrived but popped up (a VIP)", 3);
    expect(summary(spellOf([]), [n])).toBeNull();
  });

  it("counts only what the focus held, not everything that arrived meanwhile", () => {
    const held = text("Zoe", "held", 3);
    // A muted app's notification arrived too: it was never going to pop up.
    const muted = note("com.burbn.instagram", "Instagram", "someone liked your photo", 4);
    expect(summary(spellOf([held]), [held, muted])?.body).toBe("1 text");
  });

  it("leaves out what was dealt with during the focus: cleared on the phone, or seen in tug", () => {
    const cleared = text("Zoe", "cleared on the watch", 3, { removedAt: T0 + 4 * min });
    const read = text("Priya", "read in tug", 5);
    const waiting = missed("Mum", 8);
    const all = [cleared, read, waiting];
    const seenAt = (key: string) => (key === threadKey(read) ? T0 + 6 * min : 0);
    expect(summary(spellOf(all), all, { seenAt })?.body).toBe("1 missed call");
    // All dealt with: nothing to say.
    expect(summary(spellOf([cleared, read]), [cleared, read], { seenAt })).toBeNull();
  });

  it("counts a held text once even though it also came over message access, and not after a reply", () => {
    const n = text("Zoe", "running late", 3);
    const sameText: SmsMessage = {
      id: 1,
      source: "iphone-map",
      direction: "in",
      address: "+13025550142",
      contactName: "Zoe",
      body: "running late",
      sentAt: null,
      receivedAt: T0 + 3 * min,
      status: "received",
      msgType: null,
      gapBefore: false,
    };
    expect(summary(spellOf([n]), [n], { messages: [sameText] })?.body).toBe("1 text");
    // Answered from the phone during the focus: what came before it isn't news.
    const reply: SmsMessage = { ...sameText, id: 2, direction: "out", body: "ok", receivedAt: T0 + 5 * min, status: "accepted" };
    expect(summary(spellOf([n]), [n], { messages: [sameText, reply] })).toBeNull();
  });
});

describe("focusStatus", () => {
  const on = { ...DEFAULT_FOCUS, enabled: true };
  it("says why pop-ups are held, by category only", () => {
    expect(focusStatus(on, { active: true, reason: "meeting" })).toBe("Holding pop-ups now: an app is using the microphone or camera.");
    expect(focusStatus(on, { active: true, reason: "fullScreen" })).toBe("Holding pop-ups now: something is full screen.");
    expect(focusStatus(on, { active: true, reason: "coding" })).toBe("Holding pop-ups now: a code editor or terminal is in front.");
  });
  it("says nothing when nothing is held", () => {
    expect(focusStatus(on, { active: false, reason: null })).toBeNull();
    expect(focusStatus(DEFAULT_FOCUS, { active: true, reason: "meeting" })).toBeNull();
  });
});
