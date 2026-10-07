import { describe, expect, it } from "vitest";
import { groupConversations, groupThreads, newestUnreadThread, senderName, threadKey } from "./format";
import { isAddressLike, normalizeAddress } from "./address";
import {
  isKnownConversation,
  isKnownSender,
  outgoingAddresses,
  senderIndex,
  senderMayToast,
  splitConversations,
  threadCounts,
} from "./senders";
import type { Contact, PhoneNotification, SmsMessage } from "../types/protocol";

const T0 = Date.parse("2026-10-05T12:00:00");
const min = 60_000;
const SMS = "com.apple.MobileSMS";

let nid = 1;
function note(title: string, message: string, atMin = 0, extra: Partial<PhoneNotification> = {}): PhoneNotification {
  return {
    id: nid++,
    appId: SMS,
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
function sms(address: string, body: string, atMin = 0, direction: "in" | "out" = "in", contactName: string | null = null): SmsMessage {
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
const contacts: Contact[] = [
  { address: ZOE, name: "Zoe" },
  { address: "jane@example.com", name: "Jane Doe" },
  { address: "+447700900123", name: "Oli" },
];
const known = (sender: string, out: string[] = [], allow: string[] = []) => isKnownSender(sender, contacts, out, allow);

describe("isKnownSender — contacts", () => {
  it("a saved contact is known by number, and by the name iOS titles their texts with", () => {
    expect(known(ZOE)).toBe(true);
    expect(known("Zoe")).toBe(true);
    expect(known("  zoe ")).toBe(true);
  });

  it("matches a contact's number however it's formatted (+1, parentheses, dashes, dots, iOS direction marks)", () => {
    for (const shown of ["+1 (302) 555-0142", "(302) 555-0142", "302-555-0142", "302.555.0142", "13025550142", "3025550142", "‪+1 (302) 555-0142‬"]) {
      expect(known(shown), shown).toBe(true);
    }
    expect(known("+1 (302) 555-0143")).toBe(false);
  });

  it("matches an international contact written with a trunk 0 instead of the country code", () => {
    expect(known("+44 7700 900123")).toBe(true);
    expect(known("07700 900123")).toBe(true);
  });

  it("aliases: an old or reply-rewritten name is still a name iOS resolved from the phone's contacts", () => {
    // The backend maps a renamed contact's old name to the current one; when it can't (ambiguous),
    // the old name still came from the iPhone's contacts, so it's known either way.
    expect(known("zoe 💜")).toBe(true);
    expect(known("Zoe replied to you")).toBe(true);
  });

  it("a contact email matches case-insensitively", () => {
    expect(known("Jane@Example.com")).toBe(true);
  });

  it("a 'contact' whose name is just their number (learned from a stranger's notification) doesn't make them known", () => {
    const learned: Contact[] = [{ address: "+15550132244", name: "+1 (555) 013-2244" }];
    expect(isKnownSender("+1 (555) 013-2244", learned, [], [])).toBe(false);
    expect(isKnownSender("+15550132244", learned, [], [])).toBe(false);
  });
});

describe("isKnownSender — strangers", () => {
  it("an unsaved number is unknown", () => {
    expect(known("+1 (555) 013-2244")).toBe(false);
    expect(known("+15550132244")).toBe(false);
  });

  it("short codes (5–6 digits) are unknown unless saved or moved", () => {
    expect(known("72975")).toBe(false);
    expect(known("227898")).toBe(false);
    expect(isKnownSender("72975", [{ address: "72975", name: "Chase" }], [], [])).toBe(true);
    expect(known("72975", [], ["72975"])).toBe(true);
  });

  it("an email sender is unknown unless it's a contact", () => {
    expect(known("deals@promo-mail.example")).toBe(false);
  });

  it("iOS's 'Maybe:' guesses and empty titles are unknown", () => {
    expect(known("Maybe: John Appleseed")).toBe(false);
    expect(known("   ")).toBe(false);
  });
});

describe("isKnownSender — replies and the allowlist", () => {
  it("a number you've texted (any outgoing message, even one still sending) is known", () => {
    const messages = [sms("+15550132244", "who is this?", 0, "out"), sms("+15559990000", "spam", 0, "in")];
    const out = outgoingAddresses(messages);
    expect(out).toEqual(["+15550132244"]);
    expect(known("+1 (555) 013-2244", out)).toBe(true);
    expect(known("+1 (555) 999-0000", out)).toBe(false);
  });

  it("a sender moved to conversations is known, whatever format was saved", () => {
    expect(known("+1 (555) 013-2244", [], ["(555) 013-2244"])).toBe(true);
    expect(known("Deals@Promo-Mail.example", [], ["deals@promo-mail.example"])).toBe(true);
  });
});

describe("address helpers", () => {
  it("normalise like the backend's map/address.rs", () => {
    expect(normalizeAddress("+1 (302) 555-0173")).toBe("+13025550173");
    expect(normalizeAddress("3025550173")).toBe("+13025550173");
    expect(normalizeAddress("+44 20 7946 0958")).toBe("+442079460958");
    expect(normalizeAddress("72975")).toBe("72975");
    expect(normalizeAddress(" Zoe@Example.com ")).toBe("zoe@example.com");
  });

  it("tell numbers and emails from names", () => {
    expect(isAddressLike("+1 (302) 555-0173")).toBe(true);
    expect(isAddressLike("72975")).toBe(true);
    expect(isAddressLike("a@b.co")).toBe(true);
    expect(isAddressLike("Zoe")).toBe(false);
    expect(isAddressLike("Room 101")).toBe(false);
    expect(isAddressLike("zoe 💜")).toBe(false);
  });
});

describe("grouping unknown senders", () => {
  it("one stranger's notifications (however iOS formats the number) and MAP texts are one conversation, with a reply number", () => {
    expect(threadKey({ appId: SMS, title: "+1 (555) 013-2244" })).toBe(threadKey({ appId: SMS, title: "‪(555) 013-2244‬" }));
    const convs = groupConversations(
      [note("+1 (555) 013-2244", "You won a gift card!", 5)],
      [sms("+15550132244", "Final notice: car warranty", 0)],
      contacts,
    );
    expect(convs).toHaveLength(1);
    expect(convs[0].contact).toBe("(555) 013-2244");
    expect(convs[0].address).toBe("+15550132244");
    expect(convs[0].items).toHaveLength(2);
  });

  it("a notification-only stranger still gets their number to reply to (or move)", () => {
    const [c] = groupConversations([note("72975", "Your code is 731904", 0)], [], []);
    expect(c.contact).toBe("72975");
    expect(c.addresses).toEqual(["72975"]);
  });

  it("splits known conversations from unknown senders, never interleaved, each newest first", () => {
    const notes = [
      note("Zoe", "omw", 1),
      note("+1 (555) 013-2244", "Congrats! Reply YES", 4),
      note("Jane Doe", "see you at 5", 3),
      note("72975", "Your code is 731904", 6),
    ];
    const index = senderIndex(contacts, [], []);
    const { known: k, unknown: u } = splitConversations(groupConversations(notes, [], contacts), index);
    expect(k.map((c) => c.contact)).toEqual(["Jane Doe", "Zoe"]);
    expect(u.map((c) => c.contact)).toEqual(["72975", "(555) 013-2244"]);
  });

  it("replying to a stranger moves their conversation to known", () => {
    const notes = [note("+1 (555) 013-2244", "hey it's Sam, new number", 0)];
    const out = [sms("+15550132244", "oh hi Sam!", 1, "out")];
    const [before] = groupConversations(notes, [], contacts);
    expect(isKnownConversation(before, senderIndex(contacts, [], []))).toBe(false);
    const [after] = groupConversations(notes, out, contacts);
    expect(isKnownConversation(after, senderIndex(contacts, outgoingAddresses(out), []))).toBe(true);
  });

  it("other chat apps are never filtered", () => {
    const wa = note("+44 7700 900456", "hi", 0, { appId: "net.whatsapp.WhatsApp", appName: "WhatsApp" });
    const [c] = groupConversations([wa], [], []);
    expect(isKnownConversation(c, senderIndex([], [], []))).toBe(true);
    expect(senderName("+1 (555) 013-2244")).toBe("(555) 013-2244");
  });
});

describe("toasts and unread for unknown senders", () => {
  const index = senderIndex(contacts, [], []);

  it("an unknown sender's text doesn't pop up…", () => {
    expect(senderMayToast(note("+1 (555) 013-2244", "Congrats! You won a $500 gift card"), index)).toBe(false);
    expect(senderMayToast(note("deals@promo-mail.example", "50% off today only"), index)).toBe(false);
  });

  it("…unless it carries a one-time code", () => {
    expect(senderMayToast(note("72975", "Your Acme verification code is 731904."), index)).toBe(true);
    expect(senderMayToast(note("+1 (555) 013-2244", "", 0, { subtitle: "Your login code: 482-913" }), index)).toBe(true);
  });

  it("known senders and other apps pop up as before", () => {
    expect(senderMayToast(note("Zoe", "omw"), index)).toBe(true);
    expect(senderMayToast(note("Gmail", "Spam?", 0, { appId: "com.google.Gmail", appName: "Gmail" }), index)).toBe(true);
  });

  it("unread (badge, tray, taskbar dot) leaves unknown senders out", () => {
    const notes = [note("Zoe", "omw", 1), note("Zoe", "5 mins", 2), note("+1 (555) 013-2244", "Reply YES", 3), note("72975", "code 731904", 4)];
    const unread = groupThreads(notes)
      .filter((t) => threadCounts(t, index))
      .reduce((sum, t) => sum + t.items.length, 0);
    expect(unread).toBe(2);
  });

  it("the tray opens the newest known unread conversation, skipping newer unknown ones", () => {
    const notes = [note("Zoe", "omw", 1), note("+1 (555) 013-2244", "Reply YES", 9)];
    expect(newestUnreadThread(notes, () => 1, (t) => threadCounts(t, index))).toBe(threadKey({ appId: SMS, title: "Zoe" }));
    expect(newestUnreadThread([notes[1]], () => 1, (t) => threadCounts(t, index))).toBeNull();
  });
});
