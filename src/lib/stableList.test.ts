import { describe, expect, it } from "vitest";
import { groupConversations, groupFeed, type FeedEntry } from "./format";
import { reuseUnchanged, sameConversation, sameFeedEntry, sameItems } from "./stableList";
import type { Contact, PhoneNotification, SmsMessage } from "../types/protocol";

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
function sms(address: string, body: string, atMin: number, direction: "in" | "out" = "in"): SmsMessage {
  return {
    id: mid++,
    source: "iphone-map",
    direction,
    address,
    contactName: null,
    body,
    sentAt: null,
    receivedAt: T0 + atMin * min,
    status: direction === "in" ? "received" : "accepted",
    msgType: null,
    gapBefore: false,
  };
}

const ZOE = "+13025550142";
const SAM = "+12145550199";
const contacts: Contact[] = [
  { address: ZOE, name: "Zoe" },
  { address: SAM, name: "Sam" },
];
const calendar = (msg: string, at: number) =>
  note("Calendar", msg, at, { appId: "com.apple.mobilecal", appName: "Calendar", category: "schedule" });

describe("sameItems", () => {
  it("compares by identity, in order", () => {
    const a = { x: 1 };
    const b = { x: 1 };
    expect(sameItems([a], [a])).toBe(true);
    expect(sameItems([a], [b])).toBe(false);
    expect(sameItems([a, b], [b, a])).toBe(false);
    expect(sameItems([a], [a, a])).toBe(false);
  });
});

describe("reuseUnchanged", () => {
  const same = (x: { key: string; v: number }, y: { key: string; v: number }) => x.v === y.v;

  it("returns the previous list itself when nothing changed", () => {
    const prev = [{ key: "a", v: 1 }, { key: "b", v: 2 }];
    expect(reuseUnchanged(prev, [{ key: "a", v: 1 }, { key: "b", v: 2 }], same)).toBe(prev);
  });

  it("keeps the unchanged elements and takes the new one", () => {
    const prev = [{ key: "a", v: 1 }, { key: "b", v: 2 }];
    const next = [{ key: "a", v: 1 }, { key: "b", v: 3 }];
    const out = reuseUnchanged(prev, next, same);
    expect(out).not.toBe(prev);
    expect(out[0]).toBe(prev[0]);
    expect(out[1]).toBe(next[1]);
  });

  it("follows a new order, reusing the moved elements", () => {
    const prev = [{ key: "a", v: 1 }, { key: "b", v: 2 }];
    const out = reuseUnchanged(prev, [{ key: "b", v: 2 }, { key: "a", v: 1 }], same);
    expect(out).not.toBe(prev);
    expect(out[0]).toBe(prev[1]);
    expect(out[1]).toBe(prev[0]);
  });

  it("handles a first run, an addition and a removal", () => {
    const next = [{ key: "a", v: 1 }];
    expect(reuseUnchanged(undefined, next, same)).toBe(next);
    const prev = [{ key: "a", v: 1 }];
    const grown = reuseUnchanged(prev, [{ key: "a", v: 1 }, { key: "c", v: 4 }], same);
    expect(grown).toHaveLength(2);
    expect(grown[0]).toBe(prev[0]);
    const shrunk = reuseUnchanged([{ key: "a", v: 1 }, { key: "c", v: 4 }], [{ key: "a", v: 1 }], same);
    expect(shrunk).toHaveLength(1);
  });
});

describe("a stable Feed", () => {
  it("regrouping the same notifications gives back the same list", () => {
    const notes = [note("Zoe", "a", 0), note("Zoe", "b", 3), calendar("Standup", 1)];
    const first = groupFeed(notes);
    expect(reuseUnchanged(first, groupFeed(notes), sameFeedEntry)).toBe(first);
  });

  it("a new notification changes only its own row", () => {
    const notes = [note("Zoe", "a", 0), note("Sam", "hi", 1), calendar("Standup", 2)];
    const first = groupFeed(notes);
    const next = reuseUnchanged(first, groupFeed([...notes, note("Zoe", "b", 5)]), sameFeedEntry);
    const byKey = (list: FeedEntry[], k: string) => list.find((e) => e.key === k)!;
    const zoe = first.find((e) => e.kind === "thread" && e.thread.contact === "Zoe")!.key;
    const sam = first.find((e) => e.kind === "thread" && e.thread.contact === "Sam")!.key;
    const cal = first.find((e) => e.kind === "stack")!.key;
    expect(byKey(next, zoe)).not.toBe(byKey(first, zoe));
    expect(byKey(next, sam)).toBe(byKey(first, sam));
    expect(byKey(next, cal)).toBe(byKey(first, cal));
    expect(next[0].key).toBe(zoe);
  });

  it("an app's name arriving later is a change", () => {
    const n = calendar("Standup", 1);
    const first = groupFeed([n]);
    const renamed = { ...n, appName: "Kalender" };
    expect(sameFeedEntry(first[0], groupFeed([renamed])[0])).toBe(false);
  });

  it("a cleared notification leaves its row (a different set of items)", () => {
    const a = note("Zoe", "a", 0);
    const b = note("Zoe", "b", 1);
    const first = groupFeed([a, b]);
    expect(sameFeedEntry(first[0], groupFeed([a])[0])).toBe(false);
  });
});

describe("stable conversations", () => {
  it("regrouping the same texts and notifications gives back the same list", () => {
    const notes = [note("Zoe", "omw", 2)];
    const texts = [sms(ZOE, "are you coming?", 0), sms(ZOE, "yes!", 1, "out"), sms(SAM, "hey", 3)];
    const first = groupConversations(notes, texts, contacts);
    expect(reuseUnchanged(first, groupConversations(notes, texts, contacts), sameConversation)).toBe(first);
  });

  it("a new text changes only its conversation", () => {
    const texts = [sms(ZOE, "are you coming?", 0), sms(SAM, "hey", 3)];
    const first = groupConversations([], texts, contacts);
    const next = reuseUnchanged(first, groupConversations([], [...texts, sms(ZOE, "?", 5)], contacts), sameConversation);
    const zoe = (list: typeof first) => list.find((c) => c.contact === "Zoe")!;
    const sam = (list: typeof first) => list.find((c) => c.contact === "Sam")!;
    expect(zoe(next)).not.toBe(zoe(first));
    expect(sam(next)).toBe(sam(first));
  });

  it("a send moving from Sending to Sent is a change (the store replaces the text)", () => {
    const pending = { ...sms(ZOE, "on my way", 1, "out"), status: "pending" as const };
    const first = groupConversations([], [pending], contacts);
    const sent = groupConversations([], [{ ...pending, status: "accepted" }], contacts);
    expect(sameConversation(first[0], sent[0])).toBe(false);
  });

  it("a notification absorbed by its text, or a renamed contact, is a change", () => {
    const text = sms(ZOE, "omw", 0);
    const first = groupConversations([], [text], contacts);
    expect(sameConversation(first[0], groupConversations([note("Zoe", "omw", 0)], [text], contacts)[0])).toBe(false);
    const renamed = groupConversations([], [text], [{ address: ZOE, name: "Zoe B" }]);
    expect(sameConversation(first[0], renamed[0])).toBe(false);
  });
});
