import { describe, expect, it } from "vitest";
import { codeEntries, codeToastForMessage, newestCode } from "./codeFeed";
import { threadKey } from "./format";
import type { Contact, PhoneNotification, SmsMessage } from "./../types/protocol";

const T0 = Date.parse("2026-10-05T12:00:00");
const min = 60_000;
const SMS = "com.apple.MobileSMS";

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
    fresh: false,
    ...extra,
  };
}

const contacts: Contact[] = [{ address: "+13025550142", name: "Zoe" }];
// Pin "now" just past T0 so T0-stamped fixtures are a few seconds old, not in the future.
const NOW = T0 + 5_000;

describe("codeEntries", () => {
  it("surfaces a code text that never raised a notification (unknown short code)", () => {
    const entries = codeEntries([sms("98626", "480579 is your Amazon OTP. Do not share it.", -2)], [], { now: NOW });
    expect(entries).toHaveLength(1);
    expect(entries[0].code.code).toBe("480579");
    expect(entries[0].sender).toBe("98626");
    expect(entries[0].conversationKey).toBe(threadKey({ appId: SMS, title: "98626" }));
    expect(entries[0].messageIds).toHaveLength(1);
  });

  it("names a known sender from contacts, and opens their conversation", () => {
    const [e] = codeEntries([sms("+13025550142", "Your code is 123456", -1)], [], { now: NOW, contacts });
    expect(e.sender).toBe("Zoe");
    expect(e.conversationKey).toBe(threadKey({ appId: SMS, title: "Zoe" }));
  });

  it("ignores texts with no code, outgoing texts, and old texts", () => {
    const entries = codeEntries(
      [
        sms("98626", "omw, 10 mins", -1),
        sms("98626", "Your code is 222333", -1, "out"),
        sms("98626", "Your code is 444555", -30), // 30 min old, outside the 15 min window
      ],
      [],
      { now: NOW },
    );
    expect(entries).toEqual([]);
  });

  it("de-dupes against an ANCS notification carrying the same code from the same sender", () => {
    const messages = [sms("72975", "Your Acme verification code is 731904.", -1)];
    const notes = [note("72975", "Your Acme verification code is 731904.", -1)];
    expect(codeEntries(messages, notes, { now: NOW })).toEqual([]);
    // A different code from the same sender is not the same message, so it still shows.
    expect(codeEntries([sms("72975", "code 998877", -1)], notes, { now: NOW })).toHaveLength(1);
    // The same code from a different sender is not covered either.
    expect(codeEntries([sms("55555", "code 731904", -1)], notes, { now: NOW })).toHaveLength(1);
  });

  it("collapses duplicate codes (three identical OTPs) into one row, newest first", () => {
    const entries = codeEntries(
      [
        sms("98626", "480579 is your Amazon OTP. Do not share it.", -6),
        sms("98626", "480579 is your Amazon OTP. Do not share it.", -4),
        sms("98626", "480579 is your Amazon OTP. Do not share it.", -2),
      ],
      [],
      { now: NOW },
    );
    expect(entries).toHaveLength(1);
    expect(entries[0].messageIds).toHaveLength(3);
    expect(entries[0].at).toBe(T0 - 2 * min);
  });

  it("hides a code the user cleared, including every text behind it", () => {
    const messages = [sms("98626", "code 480579", -4), sms("98626", "code 480579", -2)];
    const ids = messages.map((m) => m.id);
    expect(codeEntries(messages, [], { now: NOW, cleared: ids })).toEqual([]);
    // Clearing only the older text still hides the row, because the newer one must go too.
    expect(codeEntries(messages, [], { now: NOW, cleared: [ids[0]] })).toHaveLength(1);
  });

  it("orders several codes newest first", () => {
    const entries = codeEntries(
      [sms("11111", "code 111111", -9), sms("22222", "code 222222", -1), sms("33333", "code 333333", -5)],
      [],
      { now: NOW },
    );
    expect(entries.map((e) => e.code.code)).toEqual(["222222", "333333", "111111"]);
  });
});

describe("newestCode", () => {
  it("picks the newest code across notifications and texts", () => {
    const notes = [note("Bank", "Your code is 111111", -5)];
    const messages = [sms("98626", "code 222222", -2)];
    expect(newestCode(notes, messages, { now: NOW })?.code).toBe("222222");
    expect(newestCode(notes, messages, { now: NOW })?.from).toEqual([]); // a text carries no notification to clear
  });

  it("prefers a newer notification, and carries it for clearing", () => {
    const notes = [note("72975", "code 999999", -1)];
    const messages = [sms("98626", "code 222222", -3)];
    const latest = newestCode(notes, messages, { now: NOW });
    expect(latest?.code).toBe("999999");
    expect(latest?.from).toHaveLength(1);
  });

  it("ignores codes older than the window and returns null when none", () => {
    expect(newestCode([note("Bank", "code 111111", -30)], [], { now: NOW })).toBeNull();
    expect(newestCode([], [sms("98626", "omw", -1)], { now: NOW })).toBeNull();
  });
});

describe("codeToastForMessage", () => {
  it("returns the code for a live, uncovered code text", () => {
    expect(codeToastForMessage(sms("98626", "480579 is your Amazon OTP.", 0), [], { now: NOW })).toBe("480579");
  });

  it("stays quiet when an ANCS notification already carried the code", () => {
    const m = sms("72975", "Your code is 731904", 0);
    const covered = codeToastForMessage(m, [note("72975", "Your code is 731904", 0)], { now: NOW });
    expect(covered).toBeNull();
  });

  it("stays quiet for backlog (old), outgoing, and code-less texts", () => {
    expect(codeToastForMessage(sms("98626", "code 480579", -30), [], { now: NOW })).toBeNull();
    expect(codeToastForMessage(sms("98626", "code 480579", 0, "out"), [], { now: NOW })).toBeNull();
    expect(codeToastForMessage(sms("98626", "omw", 0), [], { now: NOW })).toBeNull();
  });
});
