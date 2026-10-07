import { describe, expect, it } from "vitest";
import {
  nameKey,
  callKey,
  callKeys,
  callName,
  callTime,
  canClear,
  cleanName,
  groupCalls,
  groupConversations,
  groupFeed,
  highlight,
  initials,
  missedCallFor,
  newestUnreadThread,
  snippet,
  threadKey,
} from "./format";
import type { CallRecord, Contact, PhoneNotification, SmsMessage } from "../types/protocol";

const T0 = Date.parse("2026-10-05T12:00:00");
const min = 60_000;

let nid = 1;
function note(title: string, message: string, atMin: number, extra: Partial<PhoneNotification> = {}): PhoneNotification {
  return {
    id: nid++,
    appId: "com.apple.MobileSMS",
    appName: "Messages",
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
  };
}

describe("names and keys", () => {
  it("treats padded and differently-cased names as one person", () => {
    expect(cleanName("  marco  ")).toBe("marco");
    expect(threadKey({ appId: "x", title: "marco " })).toBe(threadKey({ appId: "x", title: "Marco" }));
  });

  it("ignores the invisible marks apps hide in names (WhatsApp's U+200E split sam ❤️ in two)", () => {
    expect(cleanName("\u200esam ❤️")).toBe("sam ❤️");
    expect(threadKey({ appId: "net.whatsapp.WhatsApp", title: "\u200esam ❤️" })).toBe(
      threadKey({ appId: "net.whatsapp.WhatsApp", title: "sam ❤️" }),
    );
    expect(cleanName("\u202aZoe\u202c\u200f\ufeff")).toBe("Zoe");
    // Emoji variants (❤ vs ❤️) are one person; zero-width joiners inside emoji are kept.
    expect(nameKey("sam ❤")).toBe(nameKey("Sam ❤️"));
    expect(cleanName("👨‍👩‍👧 fam")).toBe("👨‍👩‍👧 fam");
  });

  it("treats iOS inline-reply titles as the same person", () => {
    expect(cleanName("zoe 💜 replied to you")).toBe("zoe 💜");
    expect(threadKey({ appId: "x", title: "zoe 💜 replied to you" })).toBe(threadKey({ appId: "x", title: "zoe 💜" }));
  });

  it("never splits an emoji into a broken initial", () => {
    expect(initials("zoe 💜")).toBe("ZO"); // emoji skipped, like any single name
    expect(initials("Jane Doe")).toBe("JD");
    expect(initials("💜")).toBe("?");
  });
});

describe("groupConversations — reply numbers (H1)", () => {
  it("one number: replies go to it", () => {
    const [c] = groupConversations([], [sms("+13025550100", "hi", 0, "in", "Zoe")], []);
    expect(c.addresses).toEqual(["+13025550100"]);
    expect(c.address).toBe("+13025550100");
  });

  it("two numbers under one name: lists both and defaults to whoever texted most recently", () => {
    const messages = [
      sms("+13025550100", "from the first John", 0, "in", "John Smith"),
      sms("+12145550199", "from the second John", 5, "in", "John Smith"),
      sms("+13025550100", "my reply to the first", 6, "out", "John Smith"),
    ];
    const convs = groupConversations([], messages, []);
    expect(convs).toHaveLength(1);
    const [c] = convs;
    expect(c.addresses).toEqual(["+13025550100", "+12145550199"]);
    expect(c.address).toBe("+12145550199"); // latest *incoming*, not my latest outgoing
  });

  it("an ambiguous contact name with no messages gets no default number", () => {
    const contacts: Contact[] = [
      { address: "+13025550100", name: "John Smith" },
      { address: "+12145550199", name: "John Smith" },
    ];
    const [c] = groupConversations([note("John Smith", "hey", 0)], [], contacts);
    expect(c.addresses.sort()).toEqual(["+12145550199", "+13025550100"]);
    expect(c.address).toBeNull();
  });

  it("a unique contact name supplies the number for a notification-only conversation", () => {
    const [c] = groupConversations([note("zoe 💜", "hey", 0)], [], [{ address: "+13025550173", name: "zoe 💜" }]);
    expect(c.address).toBe("+13025550173");
  });
});

describe("groupConversations — duplicates (M4)", () => {
  it("shows a text once when both the notification and message access report it", () => {
    const [c] = groupConversations([note("Zoe", "omw", 0)], [sms("+1302", "omw", 0, "in", "Zoe")], []);
    expect(c.items).toHaveLength(1);
    expect(c.items[0].kind).toBe("message");
  });

  it("keeps two genuinely identical texts: one message can't absorb two notifications", () => {
    const notifications = [note("Zoe", "ok", 0), note("Zoe", "ok", 2)];
    const [c] = groupConversations(notifications, [sms("+1302", "ok", 0, "in", "Zoe")], []);
    expect(c.items.map((i) => i.body)).toEqual(["ok", "ok"]);
  });

  it("puts an inline reply in the sender's conversation, shown once", () => {
    const notes = [note("zoe 💜", "hi", 0), note("zoe 💜 replied to you", "Yes", 2)];
    const convs = groupConversations(notes, [sms("+1302", "Yes", 2, "in", "zoe 💜")], []);
    expect(convs).toHaveLength(1);
    expect(convs[0].contact).toBe("zoe 💜");
    expect(convs[0].items.map((i) => i.body)).toEqual(["hi", "Yes"]);
    expect(groupFeed(notes)).toHaveLength(1);
  });

  it("merges a notification titled with padding into the same conversation", () => {
    const convs = groupConversations([note("marco ", "Yow", 1)], [sms("+1214", "hi", 0, "out", "marco")], []);
    expect(convs).toHaveLength(1);
    expect(convs[0].items).toHaveLength(2);
  });
});

// A reply sent from the Windows pop-up is stored and announced exactly like one typed in tug.
// Reported as missing from its conversation (a saved contact, rapid back-and-forth): these pin
// down that the conversation logic files it where the pop-up's notification lives, after the text
// it answered, whatever the window learned first.
describe("groupConversations — a reply sent from the pop-up", () => {
  const ZOE = "+13025550142";
  const contacts: Contact[] = [{ address: ZOE, name: "Zoe" }];
  /** Phone-local ISO time, as MAP gives a text's time (the phone's clock, not the PC's). */
  const phoneTime = (atMin: number) => {
    const d = new Date(T0 + atMin * min);
    return new Date(d.getTime() - d.getTimezoneOffset() * min).toISOString().slice(0, 19);
  };
  // The text that popped up: its notification, and the same text over MAP (with the trailing space
  // the iPhone adds), from a saved contact.
  const popped = () => note("Zoe", "ok", 0, { postedAt: phoneTime(0) });
  const text = () => ({ ...sms(ZOE, "ok ", 0, "in", "Zoe"), sentAt: phoneTime(0) });
  const reply = (contactName: string | null = "Zoe") => sms(ZOE, "on my way", 0.5, "out", contactName);

  it("lands in the notification's conversation, after the text it answers", () => {
    const n = popped();
    const r = reply();
    const convs = groupConversations([n], [text(), r], contacts);
    expect(convs).toHaveLength(1);
    expect(convs[0].key).toBe(threadKey(n));
    expect(convs[0].items.map((i) => (i.kind === "message" ? `${i.m.direction}:${i.body.trim()}` : `n:${i.body}`))).toEqual([
      "in:ok",
      "out:on my way",
    ]);
    expect(convs[0].latest.kind === "message" && convs[0].latest.m.id).toBe(r.id);
  });

  it("still lands there when it carries no contact name (the contacts list names the number)", () => {
    const n = popped();
    const convs = groupConversations([n], [text(), reply(null)], contacts);
    expect(convs).toHaveLength(1);
    expect(convs[0].key).toBe(threadKey(n));
    expect(convs[0].items.some((i) => i.kind === "message" && i.m.direction === "out")).toBe(true);
  });

  it("shows even before the text's MAP copy arrives (only the notification so far)", () => {
    const n = popped();
    const convs = groupConversations([n], [reply()], contacts);
    expect(convs).toHaveLength(1);
    expect(convs[0].key).toBe(threadKey(n));
    expect(convs[0].items.map((i) => i.kind)).toEqual(["notification", "message"]);
  });

  it("is never absorbed by a notification with the same words (only incoming texts are)", () => {
    // They answered with the very words of the reply: both show.
    const echo = note("Zoe", "on my way", 1, { postedAt: phoneTime(1) });
    const convs = groupConversations([popped(), echo], [text(), reply()], contacts);
    expect(convs[0].items.filter((i) => i.body.trim() === "on my way").map((i) => i.kind)).toEqual(["message", "notification"]);
  });
});

describe("groupFeed", () => {
  it("groups chat notifications per person and other apps per app, newest first", () => {
    const entries = groupFeed([
      note("Zoe", "a", 0),
      note("Zoe", "b", 3),
      note("Calendar", "Standup", 1, { appId: "com.apple.mobilecal", appName: "Calendar", category: "schedule" }),
    ]);
    expect(entries.map((e) => e.kind)).toEqual(["thread", "stack"]);
    expect(entries[0].kind === "thread" && entries[0].thread.items.map((n) => n.message)).toEqual(["a", "b"]);
  });
});

describe("the tray's newest unread conversation", () => {
  const key = (title: string) => threadKey({ appId: "com.apple.MobileSMS", title });

  it("picks the newest thread that still has unread texts", () => {
    // Jane is newer overall, but all read; Zoe is older and unread → Zoe wins.
    const notes = [note("Jane Doe", "see you at 5", 10), note("Zoe", "omw 🚗", 4), note("Zoe", "did you see?", 2)];
    const unread = new Set([key("Zoe")]);
    expect(newestUnreadThread(notes, (k) => (unread.has(k) ? 1 : 0))).toBe(key("Zoe"));
  });

  it("prefers the newest when several conversations are unread", () => {
    const notes = [note("Zoe", "older", 2), note("Jane Doe", "newer", 9)];
    expect(newestUnreadThread(notes, () => 1)).toBe(key("Jane Doe"));
  });

  it("returns null when nothing is unread", () => {
    expect(newestUnreadThread([note("Zoe", "hi", 1)], () => 0)).toBeNull();
    expect(newestUnreadThread([], () => 1)).toBeNull();
  });
});

describe("search presentation", () => {
  it("highlights every typed word, case-insensitively", () => {
    const runs = highlight("Dinner at 7? See you at dinner", "din AT");
    expect(runs.filter((r) => r.match).map((r) => r.text)).toEqual(["Din", "at", "at", "din"]);
    expect(runs.map((r) => r.text).join("")).toBe("Dinner at 7? See you at dinner");
  });

  it("treats regex characters in the query literally", () => {
    expect(highlight("cost $5 (approx)", "(approx").filter((r) => r.match).map((r) => r.text)).toEqual(["(approx"]);
    expect(highlight("hello", "")).toEqual([{ text: "hello", match: false }]);
  });

  it("excerpts long text around the first match", () => {
    const long = "a ".repeat(100) + "the dinner plan " + "b ".repeat(100);
    const s = snippet(long, "dinner", 40);
    expect(s).toContain("dinner");
    expect(s.startsWith("…") && s.endsWith("…")).toBe(true);
    expect(snippet("short text", "x")).toBe("short text");
  });
});

describe("canClear", () => {
  it("clears what's still on the phone and offers a clear", () => {
    expect(canClear(note("Zoe", "hey", 0))).toBe(true);
    expect(canClear(note("Zoe", "hey", 0, { live: false }))).toBe(false);
    expect(canClear(note("Zoe", "hey", 0, { removedAt: T0 }))).toBe(false);
  });

  it("never clears a ringing call (its negative action is Decline)", () => {
    const ringing = note("Mum", "Incoming call", 0, { appId: "com.apple.mobilephone", category: "incomingCall", negativeLabel: "Decline" });
    expect(canClear(ringing)).toBe(false);
    expect(canClear({ ...ringing, category: "missedCall", negativeLabel: "Clear" })).toBe(true);
  });

  it("never ends a call in progress (WhatsApp's Active Call offers End Call)", () => {
    const active = note("davia", "Active Call", 0, { appId: "net.whatsapp.WhatsApp", category: "other", negativeLabel: "End Call" });
    expect(canClear(active)).toBe(false);
    expect(canClear({ ...active, negativeLabel: "Decline" })).toBe(false);
    expect(canClear({ ...active, negativeLabel: "Clear" })).toBe(true);
    expect(canClear({ ...active, negativeLabel: "" })).toBe(true);
  });
});

describe("recent calls", () => {
  const call = (number: string | null, name: string | null, at: string | null, direction: CallRecord["direction"] = "incoming"): CallRecord => ({
    direction,
    name,
    number,
    at,
  });

  it("names a call by the contact first, then the phone's name, then the number", () => {
    const nameFor = new Map([["+13025550142", "Zoe 💜 "]]);
    expect(callName(call("+13025550142", "Zoey", null), nameFor)).toBe("Zoe 💜");
    expect(callName(call("+12145550199", "Chris Smith", null), nameFor)).toBe("Chris Smith");
    expect(callName(call("+12145550199", null, null), nameFor)).toBe("(214) 555-0199");
    expect(callName(call(null, null, null), nameFor)).toBe("No caller ID");
  });

  it("groups by day, newest first, with untimed calls at the end", () => {
    const now = new Date("2026-10-05T18:00:00");
    const groups = groupCalls(
      [
        call("+1", null, "2026-10-05T09:30:00", "missed"),
        call("+2", null, "2026-10-05T08:00:00"),
        call("+3", null, "2026-10-04T21:00:00", "outgoing"),
        call("+4", null, null),
        call("+5", null, "not a time"),
      ],
      now,
    );
    expect(groups.map((g) => [g.label, g.calls.map((c) => c.number)])).toEqual([
      ["Today", ["+1", "+2"]],
      ["Yesterday", ["+3"]],
      ["Earlier", ["+4", "+5"]],
    ]);
    expect(callTime(call("+1", null, "2026-10-04T18:15:00Z"))?.toISOString()).toBe("2026-10-04T18:15:00.000Z");
  });

  it("keys a call on when/who/direction, not its position", () => {
    const a = call("+13025550142", "Zoey", "2026-10-05T09:30:00", "missed");
    expect(callKey(a)).toBe("2026-10-05T09:30:00|+13025550142|missed");
    // A withheld number and no time still key (empty time and number fields), differing by direction.
    expect(callKey(call(null, null, null, "incoming"))).toBe("||incoming");
    expect(callKey(call(null, null, null, "outgoing"))).toBe("||outgoing");
  });

  it("gives every row a stable, unique key even when calls are identical", () => {
    const newCall = call("+2", null, "2026-10-05T08:00:00");
    const dupeA = call("+1", null, "2026-10-05T09:30:00", "missed");
    const dupeB = call("+1", null, "2026-10-05T09:30:00", "missed");
    const before = [dupeA, dupeB];
    const after = [newCall, dupeA, dupeB];
    const keysBefore = callKeys(before);
    const keysAfter = callKeys(after);
    // Identical calls are disambiguated by order of appearance.
    expect(new Set(keysBefore).size).toBe(2);
    expect(keysBefore[0]).toBe("2026-10-05T09:30:00|+1|missed");
    expect(keysBefore[1]).toBe("2026-10-05T09:30:00|+1|missed#1");
    // Prepending a new call doesn't shift the keys of the rows below it (no re-key, no jump).
    expect(keysAfter.slice(1)).toEqual(keysBefore);
  });
});

describe("missedCallFor", () => {
  const missed = (title: string, atMin: number, extra: Partial<PhoneNotification> = {}) =>
    note(title, "Missed Call", atMin, {
      appId: "com.apple.mobilephone",
      category: "missedCall",
      positiveLabel: "Dial",
      flags: { silent: false, important: false, preExisting: false, positiveAction: true, negativeAction: true },
      ...extra,
    });

  it("finds the newest missed call still on the phone, by name or number", () => {
    const old = missed("zoe 💜", 0);
    const recent = missed("zoe 💜", 5);
    expect(missedCallFor([old, recent], { name: "Zoe 💜", address: "+13025550173" })).toBe(recent);
    const byNumber = missed("(302) 555-0173", 1);
    expect(missedCallFor([byNumber], { name: "Someone else", address: "+13025550173" })).toBe(byNumber);
  });

  it("ignores missed calls that are gone or can't be dialed, and other people", () => {
    expect(missedCallFor([missed("zoe 💜", 0, { removedAt: T0 })], { name: "zoe 💜", address: null })).toBeNull();
    expect(missedCallFor([missed("zoe 💜", 0, { live: false })], { name: "zoe 💜", address: null })).toBeNull();
    expect(missedCallFor([missed("Priya", 0)], { name: "zoe 💜", address: "+13025550173" })).toBeNull();
    expect(missedCallFor([note("zoe 💜", "hey", 0)], { name: "zoe 💜", address: null })).toBeNull();
  });
});
