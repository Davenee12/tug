import { describe, expect, it } from "vitest";
import { cleanName, groupConversations, groupFeed, highlight, initials, newestUnreadThread, snippet, threadKey } from "./format";
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
    expect(cleanName("  damian  ")).toBe("damian");
    expect(threadKey({ appId: "x", title: "damian " })).toBe(threadKey({ appId: "x", title: "Damian" }));
  });

  it("treats iOS inline-reply titles as the same person", () => {
    expect(cleanName("tay 🤎 replied to you")).toBe("tay 🤎");
    expect(threadKey({ appId: "x", title: "tay 🤎 replied to you" })).toBe(threadKey({ appId: "x", title: "tay 🤎" }));
  });

  it("never splits an emoji into a broken initial", () => {
    expect(initials("tay 🤎")).toBe("TA"); // emoji skipped, like any single name
    expect(initials("Jane Doe")).toBe("JD");
    expect(initials("🤎")).toBe("?");
  });
});

describe("groupConversations — reply numbers (H1)", () => {
  it("one number: replies go to it", () => {
    const [c] = groupConversations([], [sms("+13025550100", "hi", 0, "in", "Tay")], []);
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
    const [c] = groupConversations([note("tay 🤎", "hey", 0)], [], [{ address: "+13026698133", name: "tay 🤎" }]);
    expect(c.address).toBe("+13026698133");
  });
});

describe("groupConversations — duplicates (M4)", () => {
  it("shows a text once when both the notification and message access report it", () => {
    const [c] = groupConversations([note("Tay", "omw", 0)], [sms("+1302", "omw", 0, "in", "Tay")], []);
    expect(c.items).toHaveLength(1);
    expect(c.items[0].kind).toBe("message");
  });

  it("keeps two genuinely identical texts: one message can't absorb two notifications", () => {
    const notifications = [note("Tay", "ok", 0), note("Tay", "ok", 2)];
    const [c] = groupConversations(notifications, [sms("+1302", "ok", 0, "in", "Tay")], []);
    expect(c.items.map((i) => i.body)).toEqual(["ok", "ok"]);
  });

  it("puts an inline reply in the sender's conversation, shown once", () => {
    const notes = [note("tay 🤎", "hi", 0), note("tay 🤎 replied to you", "Yes", 2)];
    const convs = groupConversations(notes, [sms("+1302", "Yes", 2, "in", "tay 🤎")], []);
    expect(convs).toHaveLength(1);
    expect(convs[0].contact).toBe("tay 🤎");
    expect(convs[0].items.map((i) => i.body)).toEqual(["hi", "Yes"]);
    expect(groupFeed(notes)).toHaveLength(1);
  });

  it("merges a notification titled with padding into the same conversation", () => {
    const convs = groupConversations([note("damian ", "Yow", 1)], [sms("+1214", "hi", 0, "out", "damian")], []);
    expect(convs).toHaveLength(1);
    expect(convs[0].items).toHaveLength(2);
  });
});

describe("groupFeed", () => {
  it("groups chat notifications per person and other apps per app, newest first", () => {
    const entries = groupFeed([
      note("Tay", "a", 0),
      note("Tay", "b", 3),
      note("Calendar", "Standup", 1, { appId: "com.apple.mobilecal", appName: "Calendar", category: "schedule" }),
    ]);
    expect(entries.map((e) => e.kind)).toEqual(["thread", "stack"]);
    expect(entries[0].kind === "thread" && entries[0].thread.items.map((n) => n.message)).toEqual(["a", "b"]);
  });
});

describe("the tray's newest unread conversation", () => {
  const key = (title: string) => threadKey({ appId: "com.apple.MobileSMS", title });

  it("picks the newest thread that still has unread texts", () => {
    // Jane is newer overall, but all read; Tay is older and unread → Tay wins.
    const notes = [note("Jane Doe", "see you at 5", 10), note("Tay", "omw 🚗", 4), note("Tay", "did you see?", 2)];
    const unread = new Set([key("Tay")]);
    expect(newestUnreadThread(notes, (k) => (unread.has(k) ? 1 : 0))).toBe(key("Tay"));
  });

  it("prefers the newest when several conversations are unread", () => {
    const notes = [note("Tay", "older", 2), note("Jane Doe", "newer", 9)];
    expect(newestUnreadThread(notes, () => 1)).toBe(key("Jane Doe"));
  });

  it("returns null when nothing is unread", () => {
    expect(newestUnreadThread([note("Tay", "hi", 1)], () => 0)).toBeNull();
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
