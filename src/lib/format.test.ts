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
  isChatMessage,
  isConversation,
  LIVE_TEXT_SKEW_MS,
  actionLabels,
  messageTime,
  missedCallFor,
  newestUnreadThread,
  notificationTime,
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
    gapBefore: false,
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

  it("fills a text that came through as only U+FFFD from its notification", () => {
    // The phone sent the text as a lone replacement character; its notification ~10 s later has the words.
    const [c] = groupConversations([note("Zoe", "see you at 6", 0.2)], [sms("+1302", "�", 0, "in", "Zoe")], []);
    expect(c.items).toHaveLength(1);
    expect(c.items[0].kind).toBe("message");
    expect(c.items[0].body).toBe("see you at 6");
  });

  it("fills each unreadable text from the nearest notification, and only once", () => {
    const messages = [sms("+1302", "�", 0, "in", "Zoe"), sms("+1302", "ok", 1, "in", "Zoe"), sms("+1302", "�", 2, "in", "Zoe")];
    const notes = [note("Zoe", "first", 0.2), note("Zoe", "ok", 1.1), note("Zoe", "second", 2.2)];
    const [c] = groupConversations(notes, messages, []);
    expect(c.items.map((i) => [i.kind, i.body])).toEqual([
      ["message", "first"],
      ["message", "ok"],
      ["message", "second"],
    ]);
  });

  it("leaves an unreadable text alone with no notification close by, or in another conversation", () => {
    const [c] = groupConversations([note("Zoe", "much later", 30)], [sms("+1302", "�", 0, "in", "Zoe")], []);
    expect(c.items.map((i) => i.body)).toEqual(["�", "much later"]);
    const convs = groupConversations([note("Sam", "hi", 0.2)], [sms("+1302", "�", 0, "in", "Zoe")], []);
    expect(convs.find((x) => x.contact === "Zoe")?.items.map((i) => i.body)).toEqual(["�"]);
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

  it("keeps other chat apps as app stacks in the Feed, never per-person conversation rows", () => {
    const entries = groupFeed([
      note("Jane Doe", "texted", 0),
      note("Jane Doe", "on WhatsApp", 1, { appId: WHATSAPP, appName: "WhatsApp" }),
      note("Sam", "also WhatsApp", 2, { appId: WHATSAPP, appName: "WhatsApp" }),
      note("Jane Doe", "on Teams", 3, { appId: TEAMS, appName: "Teams" }),
    ]);
    expect(entries.map((e) => [e.kind, e.kind === "thread" ? e.thread.appId : e.stack.appId])).toEqual([
      ["stack", TEAMS],
      ["stack", WHATSAPP],
      ["thread", "com.apple.MobileSMS"],
    ]);
    const wa = entries.find((e) => e.kind === "stack" && e.stack.appId === WHATSAPP);
    expect(wa?.kind === "stack" && wa.stack.items.map((n) => n.message)).toEqual(["also WhatsApp", "on WhatsApp"]);
  });
});

const WHATSAPP = "net.whatsapp.WhatsApp";
const TEAMS = "com.microsoft.skype.teams";

describe("Messages holds only texts tug can answer", () => {
  const contacts: Contact[] = [{ address: "+13025550142", name: "Jane Doe" }];
  const others = (title: string, message: string, atMin: number) => [
    note(title, message, atMin, { appId: WHATSAPP, appName: "WhatsApp" }),
    note(title, message, atMin, { appId: TEAMS, appName: "Teams" }),
    note(title, message, atMin, { appId: "com.tinyspeck.chatlyio", appName: "Slack" }),
    note(title, message, atMin, { appId: "com.burbn.instagram", appName: "Instagram" }),
    // A chat app tug has never heard of: the rule is an allow-list, so it can't leak in either.
    note(title, message, atMin, { appId: "com.example.newchat", appName: "NewChat" }),
  ];

  it("only Messages notifications form or join conversations, even under the same sender name", () => {
    const notes = [note("Jane Doe", "see you at 5", 1), ...others("Jane Doe", "see you at 5", 1), ...others("Sam", "hey", 2)];
    const convs = groupConversations(notes, [sms("+13025550142", "running late", 0)], contacts);
    expect(convs.map((c) => [c.appId, c.contact])).toEqual([["com.apple.MobileSMS", "Jane Doe"]]);
    const [jane] = convs;
    // Her texts and her Messages notification only; the WhatsApp/Teams copies of "see you at 5" stay out.
    expect(jane.items.map((i) => i.body)).toEqual(["running late", "see you at 5"]);
    expect(jane.notifications.every((n) => n.appId === "com.apple.MobileSMS")).toBe(true);
    expect(jane.addresses).toEqual(["+13025550142"]);
  });

  it("other apps alone make no conversations at all (and no thread to count unread in)", () => {
    expect(groupConversations(others("Sam", "hey", 0), [], contacts)).toEqual([]);
    expect(newestUnreadThread(others("Sam", "hey", 0), () => 1)).toBeNull();
  });

  it("an unknown sender's text still makes a Messages conversation", () => {
    const [c] = groupConversations([note("+1 (555) 013-2244", "who's this?", 0)], [], contacts);
    expect(c.appId).toBe("com.apple.MobileSMS");
    expect(c.address).toBe("+15550132244");
  });

  it("a Messages notification is a conversation; other chat apps aren't, but still read as messages", () => {
    expect(isConversation({ appId: "com.apple.MobileSMS", title: "Zoe" })).toBe(true);
    expect(isConversation({ appId: "com.apple.MobileSMS", title: "" })).toBe(false);
    for (const appId of [WHATSAPP, TEAMS, "com.example.newchat"]) expect(isConversation({ appId, title: "Zoe" })).toBe(false);
    // Pop-up wording while previews are hidden ("New message") keeps the chat apps tug knows.
    expect(isChatMessage({ appId: WHATSAPP, title: "Zoe" })).toBe(true);
    expect(isChatMessage({ appId: "com.apple.mobilecal", title: "Standup" })).toBe(false);
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

  it("clears on an iPhone in another language (the label is localized)", () => {
    for (const label of ["Löschen", "Effacer", "Borrar", "Cancella", "Apagar", "Wissen", "清除", "消去", "지우기", "Очистить"]) {
      expect(canClear(note("Zoe", "hey", 0, { negativeLabel: label })), label).toBe(true);
    }
  });

  it("still never ends or declines a call in another language", () => {
    const active = note("davia", "Active Call", 0, { appId: "net.whatsapp.WhatsApp", category: "other" });
    for (const label of ["Beenden", "Auflegen", "Raccrocher", "Terminer l’appel", "Colgar", "Rechazar", "Encerrar", "Ophangen", "終了", "挂断", "종료", "Завершить"]) {
      expect(canClear({ ...active, negativeLabel: label }), label).toBe(false);
    }
    // Unknown labels (an app's own call or room actions) get no Clear: an allow-list, not a guess.
    for (const label of ["Avböj", "Bitir", "Rozłącz", "Сбросить", "Disconnect", "Leave", "Ignore", "Mute", "Snooze"]) {
      expect(canClear({ ...active, negativeLabel: label }), label).toBe(false);
    }
    // No negative action at all: nothing to send.
    expect(canClear(note("Zoe", "hey", 0, { flags: { silent: false, important: false, preExisting: false, positiveAction: false, negativeAction: false } }))).toBe(false);
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

  it("never matches a name two people share: calling back the wrong Sam is worse than no Call back", () => {
    const known = [
      { name: "Sam", address: "+13025550111" },
      { name: "Sam", address: "+13025550112" },
    ];
    const fromSam = missed("Sam", 0);
    expect(missedCallFor([fromSam], { name: "Sam", address: "+13025550112" }, known)).toBeNull();
    expect(missedCallFor([fromSam], { name: "Sam", address: null }, known)).toBeNull();
    // The number still matches when iOS titles the call with it.
    const byNumber = missed("(302) 555-0112", 1);
    expect(missedCallFor([fromSam, byNumber], { name: "Sam", address: "+13025550112" }, known)).toBe(byNumber);
  });

  it("matches a unique name only when its one number is this person's", () => {
    const known = [{ name: "Zoe", address: "+13025550173" }];
    const fromZoe = missed("Zoe", 0);
    expect(missedCallFor([fromZoe], { name: "Zoe", address: "+13025550173" }, known)).toBe(fromZoe);
    expect(missedCallFor([fromZoe], { name: "Zoe", address: null }, known)).toBe(fromZoe);
    // A different Zoe (another number under the same name): not her missed call.
    expect(missedCallFor([fromZoe], { name: "Zoe", address: "+13025550999" }, known)).toBeNull();
  });

  it("prefers a number match over a newer name match", () => {
    const byNumber = missed("(302) 555-0173", 0);
    const byName = missed("Zoe", 5);
    expect(missedCallFor([byName, byNumber], { name: "Zoe", address: "+13025550173" })).toBe(byNumber);
  });
});

describe("message order", () => {
  // The phone's clock ran ~3 s ahead of the PC's, and its time has no zone.
  const local = (ms: number) => {
    const d = new Date(ms);
    const p = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
  };

  it("keeps a quick reply below the text it answers", () => {
    const mine = sms("+13025550100", "dinner at 7?", 0, "out");
    const reply = { ...sms("+13025550100", "yes!", 0), receivedAt: T0 + 2_000, sentAt: local(T0 + 4_000) };
    expect(messageTime(reply).getTime()).toBe(T0 + 2_000);
    const [c] = groupConversations([], [reply, mine], []);
    expect(c.items.map((i) => i.body)).toEqual(["dinner at 7?", "yes!"]);
  });

  it("keeps a reply below your text when the phone's clock runs behind the PC's", () => {
    // You sent at 12:00 (PC clock); they replied 30 s later, but the phone's clock is 2 min slow.
    const mine = { ...sms("+13025550100", "running late?", 0, "out"), receivedAt: T0 };
    const reply = { ...sms("+13025550100", "no, on time", 0), receivedAt: T0 + 30_000, sentAt: local(T0 + 30_000 - 2 * min) };
    expect(messageTime(reply).getTime()).toBe(T0 + 30_000);
    const [c] = groupConversations([], [reply, mine], []);
    expect(c.items.map((i) => i.body)).toEqual(["running late?", "no, on time"]);
  });

  it("orders texts that arrived together by arrival", () => {
    const a = { ...sms("+13025550100", "first", 0), receivedAt: T0, sentAt: local(T0 - 1 * min) };
    const b = { ...sms("+13025550100", "second", 0), receivedAt: T0, sentAt: local(T0 - 2 * min) };
    const [c] = groupConversations([], [b, a], []);
    expect(c.items.map((i) => i.body)).toEqual(["first", "second"]);
  });

  it("uses the phone's time for older texts caught up later", () => {
    const old = { ...sms("+13025550100", "earlier", 60), sentAt: local(T0) };
    expect(messageTime(old).getTime()).toBe(T0);
    // Just past the live window, it's a catch-up: the phone's time.
    const lateCatchUp = { ...sms("+13025550100", "earlier", 0), receivedAt: T0 + LIVE_TEXT_SKEW_MS + 2_000, sentAt: local(T0) };
    expect(messageTime(lateCatchUp).getTime()).toBe(T0);
  });

  it("falls back to when tug got it without a usable phone time", () => {
    expect(messageTime({ sentAt: null, receivedAt: T0 }).getTime()).toBe(T0);
    expect(messageTime({ sentAt: "not a date", receivedAt: T0 }).getTime()).toBe(T0);
  });
});

describe("notificationTime", () => {
  /** The phone's clock as ANCS gives it: local ISO with no zone, `offsetMin` from this PC's zone. */
  const phoneLocal = (atMin: number, offsetMin = 0) => {
    const d = new Date(T0 + (atMin + offsetMin) * min);
    return new Date(d.getTime() - d.getTimezoneOffset() * min).toISOString().slice(0, 19);
  };
  const preExisting = { silent: false, important: false, preExisting: true, positiveAction: false, negativeAction: true };

  it("uses the phone's time when it agrees with when tug saw it", () => {
    expect(notificationTime(note("Zoe", "hi", 2, { postedAt: phoneLocal(0) })).getTime()).toBe(T0);
    expect(notificationTime(note("Zoe", "hi", 0, { postedAt: phoneLocal(3) })).getTime()).toBe(T0 + 3 * min);
  });

  it("uses tug's receive time when the phone is in another time zone", () => {
    // Phone 5 hours ahead (or behind) of the PC, a notification arriving live.
    expect(notificationTime(note("Zoe", "hi", 0, { postedAt: phoneLocal(0, 300) })).getTime()).toBe(T0);
    expect(notificationTime(note("Zoe", "hi", 0, { postedAt: phoneLocal(0, -300) })).getTime()).toBe(T0);
    // Ahead is impossible even for a replay.
    expect(notificationTime(note("Zoe", "hi", 0, { postedAt: phoneLocal(0, 60), flags: preExisting })).getTime()).toBe(T0);
  });

  it("keeps an old notification's own time when the phone replays it on connecting", () => {
    // Posted two hours before tug first saw it (it was waiting on the phone).
    expect(notificationTime(note("Zoe", "hi", 120, { postedAt: phoneLocal(0), flags: preExisting })).getTime()).toBe(T0);
  });

  it("falls back to the receive time without a usable phone time", () => {
    expect(notificationTime(note("Zoe", "hi", 0)).getTime()).toBe(T0);
    expect(notificationTime(note("Zoe", "hi", 0, { postedAt: "not a date" })).getTime()).toBe(T0);
  });
});

describe("actionLabels", () => {
  it("never calls a ringing call's unlabelled negative action Dismiss: it declines the call", () => {
    expect(actionLabels({ category: "incomingCall", positiveLabel: "", negativeLabel: "" })).toEqual({
      positive: "Answer",
      negative: "Decline",
    });
  });

  it("keeps the phone's own words when it gives them", () => {
    expect(actionLabels({ category: "incomingCall", positiveLabel: "Accept", negativeLabel: "Decline" })).toEqual({
      positive: "Accept",
      negative: "Decline",
    });
    expect(actionLabels({ category: "social", positiveLabel: "", negativeLabel: "Clear" }).negative).toBe("Clear");
  });

  it("missed calls say Call back; other unlabelled actions are Accept / Dismiss", () => {
    expect(actionLabels({ category: "missedCall", positiveLabel: "Dial", negativeLabel: "" })).toEqual({
      positive: "Call back",
      negative: "Dismiss",
    });
    expect(actionLabels({ category: "social", positiveLabel: "", negativeLabel: "" })).toEqual({
      positive: "Accept",
      negative: "Dismiss",
    });
  });
});