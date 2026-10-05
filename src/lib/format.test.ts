import { describe, expect, it } from "vitest";
import { canClear, cleanName, groupConversations, groupFeed, highlight, initials, snippet, threadKey } from "./format";
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
  };
}

describe("names and keys", () => {
  it("treats padded and differently-cased names as one person", () => {
    expect(cleanName("  marco  ")).toBe("marco");
    expect(threadKey({ appId: "x", title: "marco " })).toBe(threadKey({ appId: "x", title: "Marco" }));
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
    expect(canClear({ ...ringing, category: "missedCall" })).toBe(true);
  });
});
