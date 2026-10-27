import { describe, expect, it } from "vitest";
import {
  AWAY_MIN_MS,
  AwayClock,
  awayFor,
  headline,
  MAX_GROUPS,
  oneLine,
  plural,
  rowMeta,
  welcomeBack,
  whenLabel,
  worthACard,
  type WelcomeInput,
} from "./welcomeBack";
import { clockTime, threadKey } from "./format";
import type { Contact, PhoneNotification, SmsMessage } from "../types/protocol";

const T0 = Date.parse("2026-10-05T12:00:00");
const min = 60_000;
const SMS = "com.apple.MobileSMS";
// Away from 10:40 to 12:00 (1 h 20 min); fixtures are stamped in minutes relative to 12:00.
const SINCE = T0 - 80 * min;
const UNTIL = T0;

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

let mid = 1;
function sms(address: string, body: string, atMin: number, direction: "in" | "out" = "in", contactName: string | null = null): SmsMessage {
  return {
    id: mid++,
    source: "iphone-map",
    direction,
    address,
    contactName,
    body,
    sentAt: null,
    receivedAt: T0 + atMin * min,
    status: direction === "in" ? "received" : "accepted",
    msgType: null,
    gapBefore: false,
  };
}

const ZOE = "+13025550142";
const contacts: Contact[] = [{ address: ZOE, name: "Zoe" }];

function summary(over: Partial<WelcomeInput> = {}) {
  return welcomeBack({
    since: SINCE,
    until: UNTIL,
    now: UNTIL,
    notifications: [],
    messages: [],
    contacts,
    seenAt: () => 0,
    ...over,
  });
}

describe("AwayClock", () => {
  it("reports nothing for a short absence", () => {
    const c = new AwayClock(T0, true);
    c.left(T0 + 5 * min);
    expect(c.here(T0 + 5 * min + AWAY_MIN_MS - 1)).toBeNull();
  });

  it("reports an absence from when the window was hidden to when it came back", () => {
    const c = new AwayClock(T0, true);
    c.here(T0 + 2 * min);
    c.left(T0 + 10 * min);
    expect(c.here(T0 + 90 * min)).toEqual({ since: T0 + 10 * min, until: T0 + 90 * min });
  });

  it("reports exactly 20 minutes away", () => {
    const c = new AwayClock(T0, true);
    c.left(T0);
    expect(c.here(T0 + AWAY_MIN_MS)).toEqual({ since: T0, until: T0 + AWAY_MIN_MS });
  });

  it("counts from the last input when the window was left in front, untouched", () => {
    const c = new AwayClock(T0, true);
    c.here(T0 + 3 * min);
    expect(c.here(T0 + 45 * min)).toEqual({ since: T0 + 3 * min, until: T0 + 45 * min });
  });

  it("keeps the moment they left when focus goes and then the window hides", () => {
    const c = new AwayClock(T0, true);
    c.left(T0 + 1 * min); // clicked another app
    c.left(T0 + 30 * min); // the screen locked later
    expect(c.here(T0 + 60 * min)?.since).toBe(T0 + 1 * min);
  });

  it("never guesses when tug started hidden: the first sign of someone only starts the clock", () => {
    const c = new AwayClock(T0, false);
    expect(c.here(T0 + 5 * 60 * min)).toBeNull();
    c.left(T0 + 5 * 60 * min + min);
    expect(c.here(T0 + 7 * 60 * min)).toEqual({ since: T0 + 5 * 60 * min + min, until: T0 + 7 * 60 * min });
  });

  it("reports an absence once: coming back starts a new stretch", () => {
    const c = new AwayClock(T0, true);
    c.left(T0);
    expect(c.here(T0 + 30 * min)).not.toBeNull();
    expect(c.here(T0 + 31 * min)).toBeNull();
  });

  it("ignores a clock that went backwards", () => {
    const c = new AwayClock(T0, true);
    c.left(T0);
    expect(c.here(T0 - 60 * min)).toBeNull();
  });
});

describe("words", () => {
  it("pluralises", () => {
    expect(plural(1, "text")).toBe("1 text");
    expect(plural(2, "text")).toBe("2 texts");
    expect(plural(0, "notification")).toBe("0 notifications");
    expect(plural(1, "person", "people")).toBe("1 person");
    expect(plural(3, "person", "people")).toBe("3 people");
  });

  it("says how long someone was away", () => {
    expect(awayFor(20 * min)).toBe("20 min");
    expect(awayFor(59 * min + 59_000)).toBe("59 min");
    expect(awayFor(60 * min)).toBe("1 h");
    expect(awayFor(80 * min)).toBe("1 h 20 min");
    expect(awayFor(5 * 60 * min + 1 * min)).toBe("5 h 1 min");
    expect(awayFor(24 * 60 * min)).toBe("1 day");
    expect(awayFor(27 * 60 * min + 15 * min)).toBe("1 day 3 h");
    expect(awayFor(2 * 24 * 60 * min)).toBe("2 days");
    expect(awayFor(-5)).toBe("0 min");
  });

  it("folds a line and cuts it at a word", () => {
    expect(oneLine("  see you\n\nat  five ")).toBe("see you at five");
    const long = "Are we still meeting at five? I can grab a table if you're running late, just let me know before you leave";
    const cut = oneLine(long, 40);
    expect(Array.from(cut).length).toBeLessThanOrEqual(40);
    expect(cut.endsWith("…")).toBe(true);
    expect(long.startsWith(cut.slice(0, -1))).toBe(true);
    expect(cut).not.toMatch(/[ ,]…$/);
  });

  it("cuts a line with no spaces mid-word, and never through an emoji", () => {
    expect(oneLine("x".repeat(100), 10)).toBe(`${"x".repeat(9)}…`);
    const emoji = oneLine(`${"a".repeat(8)}🚗🚗🚗`, 10);
    expect(emoji).toBe(`${"a".repeat(8)}🚗…`);
  });

  it("times a row by the clock today, with the day before that", () => {
    expect(whenLabel(T0 - 30 * min, T0)).toBe(clockTime(new Date(T0 - 30 * min)));
    expect(whenLabel(T0 - 24 * 60 * min, T0)).toBe(`Yesterday ${clockTime(new Date(T0 - 24 * 60 * min))}`);
  });
});

describe("rowMeta", () => {
  const g = (kind: "person" | "call" | "code" | "app", appId: string, appLabel: string, count: number) => ({ kind, appId, appLabel, count });
  it("names the app only when it isn't obvious, and counts more than one", () => {
    expect(rowMeta(g("person", SMS, "Messages", 1))).toBe("");
    expect(rowMeta(g("person", SMS, "Messages", 3))).toBe("3 texts");
    expect(rowMeta(g("person", "net.whatsapp.WhatsApp", "WhatsApp", 1))).toBe("WhatsApp");
    expect(rowMeta(g("person", "net.whatsapp.WhatsApp", "WhatsApp", 2))).toBe("WhatsApp · 2 messages");
    expect(rowMeta(g("app", "com.apple.mobilecal", "Calendar", 4))).toBe("4 notifications");
    expect(rowMeta(g("app", "com.apple.mobilecal", "Calendar", 1))).toBe("");
  });
  it("leaves a call's or a code sender's count to its line", () => {
    expect(rowMeta(g("call", "com.apple.mobilephone", "Phone", 2))).toBe("");
    expect(rowMeta(g("call", "net.whatsapp.WhatsApp", "WhatsApp", 2))).toBe("WhatsApp");
    expect(rowMeta(g("code", SMS, "Messages", 2))).toBe("");
  });
});

describe("headline", () => {
  it("reads like the example", () => {
    expect(headline("1 h 20 min", { texts: 7, chats: 0, people: 3, missedCalls: 2, codes: 0, notifications: 12 })).toBe(
      "While you were away (1 h 20 min): 7 texts from 3 people, 2 missed calls, 12 notifications",
    );
  });

  it("gets the singulars right", () => {
    expect(headline("25 min", { texts: 1, chats: 0, people: 1, missedCalls: 1, codes: 1, notifications: 1 })).toBe(
      "While you were away (25 min): 1 text, 1 missed call, 1 verification code, 1 notification",
    );
    expect(headline("25 min", { texts: 3, chats: 0, people: 1, missedCalls: 0, codes: 0, notifications: 0 })).toBe(
      "While you were away (25 min): 3 texts from 1 person",
    );
  });

  it("says messages, not texts, once other chat apps are in it", () => {
    expect(headline("2 h", { texts: 2, chats: 3, people: 2, missedCalls: 0, codes: 2, notifications: 0 })).toBe(
      "While you were away (2 h): 5 messages from 2 people, 2 verification codes",
    );
  });

  it("is just the time away when nothing is left", () => {
    expect(headline("2 h", { texts: 0, chats: 0, people: 0, missedCalls: 0, codes: 0, notifications: 0 })).toBe(
      "While you were away (2 h)",
    );
  });
});

describe("welcomeBack", () => {
  it("groups texts by person, newest line and time, and opens the conversation", () => {
    const s = summary({
      notifications: [text("Zoe", "omw", -5), text("Zoe", "running late", -20), text("Jane Doe", "Booked for Thursday", -10)],
    });
    expect(s.groups.map((g) => [g.name, g.count, g.line])).toEqual([
      ["Zoe", 2, "omw"],
      ["Jane Doe", 1, "Booked for Thursday"],
    ]);
    expect(s.groups[0].at).toBe(T0 - 5 * min);
    expect(s.groups[0].kind).toBe("person");
    expect(s.groups[0].target).toEqual({ kind: "conversation", key: threadKey({ appId: SMS, title: "Zoe" }) });
    expect(s.headline).toBe("While you were away (1 h 20 min): 3 texts from 2 people");
    expect(s.total).toBe(3);
  });

  it("counts a text that came as a notification and a message once, and adds texts that only came as messages", () => {
    const s = summary({
      notifications: [text("Zoe", "omw", -5)],
      messages: [sms(ZOE, "omw", -5, "in", "Zoe"), sms(ZOE, "Attachment: 1 Photo", -6, "in", "Zoe")],
    });
    expect(s.groups).toHaveLength(1);
    expect(s.groups[0].count).toBe(2);
    expect(s.groups[0].line).toBe("omw");
    expect(s.counts.texts).toBe(2);
  });

  it("says a photo in words", () => {
    const s = summary({ messages: [sms(ZOE, "Attachment: 1 Photo", -6, "in", "Zoe")] });
    expect(s.groups[0].line).toMatch(/Photo/);
    expect(s.groups[0].line).not.toMatch(/Attachment:/);
  });

  it("leaves out what arrived before or after the absence", () => {
    const s = summary({
      notifications: [text("Zoe", "before", -81), text("Zoe", "during", -79), text("Zoe", "after", 1)],
      messages: [sms(ZOE, "old", -200, "in", "Zoe"), sms(ZOE, "later", 3, "in", "Zoe")],
    });
    expect(s.total).toBe(1);
    expect(s.groups[0].line).toBe("during");
  });

  it("leaves out what's been seen, under the Feed's own keys", () => {
    const zoe = threadKey({ appId: SMS, title: "Zoe" });
    const s = summary({
      notifications: [
        text("Zoe", "seen", -30),
        text("Zoe", "new", -10),
        note("com.google.Gmail", "Google", "seen mail", -40),
        note("com.slack", "Slack", "new ping", -5),
      ],
      seenAt: (key) => (key === zoe ? T0 - 20 * min : key === "app\u0000com.google.Gmail" ? T0 - 30 * min : 0),
    });
    expect(s.groups.map((g) => g.line)).toEqual(["new", "Slack · new ping"]);
  });

  it("leaves out what was cleared on the phone, including a text whose notification was cleared", () => {
    const s = summary({
      notifications: [
        text("Zoe", "read on the phone", -30, { removedAt: T0 - 25 * min, live: false }),
        note("com.google.Gmail", "Google", "cleared", -40, { removedAt: T0 - 35 * min }),
        text("Jane Doe", "still waiting", -10),
      ],
      messages: [sms(ZOE, "read on the phone", -30, "in", "Zoe")],
    });
    expect(s.groups.map((g) => g.name)).toEqual(["Jane Doe"]);
  });

  it("never counts texts you sent", () => {
    const s = summary({ messages: [sms(ZOE, "on my way", -10, "out", "Zoe"), sms(ZOE, "ok!", -9, "in", "Zoe")] });
    expect(s.total).toBe(1);
    expect(s.groups[0].line).toBe("ok!");
  });

  it("leaves out texts answered since: a reply from the phone or tug means they were read", () => {
    const s = summary({
      notifications: [text("Zoe", "are you coming tonight?", -40), text("Zoe", "great, see you there", -20)],
      messages: [sms(ZOE, "yes! leaving soon", -38, "out", "Zoe")],
    });
    expect(s.total).toBe(1);
    expect(s.groups[0].line).toBe("great, see you there");
    // A reply sent after coming back counts too (the card is worked out again as things change).
    const later = summary({
      notifications: [text("Zoe", "are you coming tonight?", -40)],
      messages: [sms(ZOE, "on my way", 2, "out", "Zoe")],
    });
    expect(later.total).toBe(0);
  });

  it("leaves out code texts cleared from the Feed", () => {
    const code = sms("98626", "480579 is your Amazon OTP. Do not share it.", -2);
    expect(summary({ messages: [code] }).total).toBe(1);
    expect(summary({ messages: [code], clearedCodes: [code.id] }).total).toBe(0);
  });

  it("leaves out unknown senders, but not their codes", () => {
    const isKnown = (c: { contact: string }) => c.contact === "Zoe";
    const s = summary({
      notifications: [
        text("Zoe", "hi", -10),
        text("+1 (555) 013-2244", "You've won a gift card!", -20),
        text("72975", "Your Acme verification code is 731904.", -3),
      ],
      isKnown,
    });
    expect(s.groups.map((g) => [g.name, g.kind])).toEqual([
      ["72975", "code"],
      ["Zoe", "person"],
    ]);
  });

  it("groups other chat apps by person and opens their row in the Feed, but a #channel stays with its app", () => {
    const sam = note("net.whatsapp.WhatsApp", "Sam Okafor", "Sent you the slides", -9, { appName: "WhatsApp" });
    const s = summary({
      notifications: [
        note("net.whatsapp.WhatsApp", "Sam Okafor", "Running late", -30, { appName: "WhatsApp" }),
        sam,
        note("com.tinyspeck.chatlyio", "#design", "Priya: new mocks are up", -11, { appName: "Slack" }),
      ],
    });
    expect(s.groups.map((g) => [g.kind, g.name, g.count])).toEqual([
      ["person", "Sam Okafor", 2],
      ["app", "Slack", 1],
    ]);
    expect(s.groups[0].appLabel).toBe("WhatsApp");
    expect(s.groups[0].target).toEqual({ kind: "feed", item: `n${sam.id}` });
    expect(s.groups[1].line).toBe("#design · Priya: new mocks are up");
    expect(s.headline).toBe("While you were away (1 h 20 min): 2 messages from 1 person, 1 notification");
  });

  it("groups other notifications by app", () => {
    const newest = note("com.apple.mobilecal", "Design review", "In 15 minutes · Room 4", -18, { appName: "Calendar" });
    const s = summary({
      notifications: [note("com.apple.mobilecal", "Standup", "Now", -60, { appName: "Calendar" }), newest],
    });
    expect(s.groups).toHaveLength(1);
    expect(s.groups[0]).toMatchObject({ kind: "app", name: "Calendar", count: 2, line: "Design review · In 15 minutes · Room 4" });
    expect(s.groups[0].target).toEqual({ kind: "feed", item: `n${newest.id}` });
  });

  it("highlights missed calls, by caller", () => {
    const s = summary({ notifications: [missed("Mum", -3), missed("Mum", -40), missed("(302) 555-0177", -50)] });
    expect(s.groups.map((g) => [g.kind, g.name, g.count, g.line])).toEqual([
      ["call", "Mum", 2, "2 missed calls"],
      ["call", "(302) 555-0177", 1, "Missed call"],
    ]);
    expect(s.counts.missedCalls).toBe(3);
    expect(s.headline).toBe("While you were away (1 h 20 min): 3 missed calls");
  });

  it("leaves out a call that was ringing (its missed call counts instead)", () => {
    const ringing = note("com.apple.mobilephone", "Mum", "Incoming Call", -3, { category: "incomingCall", live: false });
    expect(summary({ notifications: [ringing] }).total).toBe(0);
  });

  it("puts people first, newest first, then apps", () => {
    const s = summary({
      notifications: [
        note("com.google.Gmail", "Google", "newest of all", -1, { appName: "Gmail" }),
        text("Zoe", "hi", -30),
        missed("Mum", -10),
        note("com.apple.mobilecal", "Standup", "Now", -5, { appName: "Calendar" }),
        text("Jane Doe", "yo", -20),
      ],
    });
    expect(s.groups.map((g) => g.name)).toEqual(["Mum", "Jane Doe", "Zoe", "Gmail", "Calendar"]);
  });

  it(`shows at most ${MAX_GROUPS} rows, and says how many more`, () => {
    const many = Array.from({ length: 9 }, (_, i) => text(`Person ${i}`, "hi", -(i + 1)));
    const s = summary({ notifications: many });
    expect(s.groups).toHaveLength(MAX_GROUPS);
    expect(s.more).toBe(3);
    expect(s.groups[0].name).toBe("Person 0");
    // The headline still counts everything.
    expect(s.headline).toBe("While you were away (1 h 20 min): 9 texts from 9 people");
  });

  it("never says 'and 1 more': that row is shown instead", () => {
    const seven = Array.from({ length: MAX_GROUPS + 1 }, (_, i) => text(`Person ${i}`, "hi", -(i + 1)));
    const s = summary({ notifications: seven });
    expect(s.groups).toHaveLength(MAX_GROUPS + 1);
    expect(s.more).toBe(0);
  });

  it("offers a fresh code to copy, from the notification it came on", () => {
    const gmail = note("com.google.Gmail", "Google", "G-591204 is your Google verification code.", -6, { appName: "Gmail" });
    const s = summary({ notifications: [gmail], now: T0 });
    expect(s.groups[0].code).toBe("591204");
    expect(s.groups[0].codeFrom).toEqual([gmail]);
    expect(s.groups[0].staleCodes).toBe(0);
  });

  it("counts a code older than 10 minutes instead of showing its digits", () => {
    const s = summary({
      notifications: [text("Bank", "Your code is 482913. Don't share it.", -30), text("Bank", "Your code is 118822.", -25)],
      now: T0,
    });
    const bank = s.groups[0];
    expect(bank.kind).toBe("code");
    expect(bank.code).toBeNull();
    expect(bank.staleCodes).toBe(2);
    expect(bank.line).toBe("2 verification codes");
    expect(JSON.stringify(s)).not.toMatch(/482913|118822/);
    expect(s.headline).toBe("While you were away (1 h 20 min): 2 verification codes");
  });

  it("ages a code with the clock: fresh at first, a count ten minutes later", () => {
    const n = [text("72975", "Your Acme verification code is 731904.", -3)];
    expect(summary({ notifications: n, now: T0 }).groups[0].code).toBe("731904");
    const later = summary({ notifications: n, now: T0 + 8 * min }).groups[0];
    expect(later.code).toBeNull();
    expect(later.line).toBe("Verification code");
  });

  it("keeps a person a person when only some of their texts carry a code", () => {
    const s = summary({ notifications: [text("Zoe", "the door code is 4821, see you", -40), text("Zoe", "here!", -5)] });
    expect(s.groups[0].kind).toBe("person");
    expect(s.groups[0].line).toBe("here!");
  });

  it("matches the made-up example: 7 texts from 3 people, 2 missed calls, 12 notifications", () => {
    const notifications = [
      ...["a", "b", "c"].map((m, i) => text("Zoe", m, -10 - i)),
      ...["d", "e"].map((m, i) => text("Jane Doe", m, -20 - i)),
      ...["f", "g"].map((m, i) => text("Chris Smith", m, -30 - i)),
      missed("Mum", -15),
      missed("Mum", -45),
      ...Array.from({ length: 12 }, (_, i) => note(`com.example.app${i % 3}`, "Update", `news ${i}`, -50 - i)),
    ];
    const s = summary({ notifications });
    expect(s.headline).toBe("While you were away (1 h 20 min): 7 texts from 3 people, 2 missed calls, 12 notifications");
    expect(s.total).toBe(21);
    expect(worthACard(s)).toBe(true);
  });

  it("only makes a card for a long enough absence with enough waiting", () => {
    const three = [text("Zoe", "a", -3), text("Zoe", "b", -2), text("Zoe", "c", -1)];
    expect(worthACard(summary({ notifications: three }))).toBe(true);
    expect(worthACard(summary({ notifications: three.slice(0, 2) }))).toBe(false);
    expect(worthACard(summary({ notifications: three, since: UNTIL - AWAY_MIN_MS + 1 }))).toBe(false);
  });
});
