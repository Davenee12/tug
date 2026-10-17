import { describe, expect, it } from "vitest";
import { replyAddress, replyReadsConversation, replyTarget, sendOutcome, toastSpec } from "./toastSpec";
import type { Contact, PhoneNotification, SmsMessage } from "../types/protocol";

const T0 = Date.parse("2026-10-05T12:00:00");

let nid = 1;
function note(title: string, message: string, extra: Partial<PhoneNotification> = {}): PhoneNotification {
  return {
    id: nid++,
    appId: "com.apple.MobileSMS",
    appName: "Messages",
    category: "social",
    title,
    subtitle: "",
    message,
    postedAt: null,
    receivedAt: T0,
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
function sms(address: string, contactName: string | null, direction: "in" | "out" = "in"): SmsMessage {
  return {
    id: mid++,
    source: "iphone-map",
    direction,
    address,
    contactName,
    body: "hi",
    sentAt: null,
    receivedAt: T0 - 60_000,
    status: direction === "in" ? "received" : "accepted",
    msgType: null,
    gapBefore: false,
  };
}

const contacts: Contact[] = [
  { address: "+13025550123", name: "Zoe" },
  { address: "+13025550111", name: "Sam" },
  { address: "+13025550112", name: "Sam" },
];

describe("replyAddress", () => {
  it("answers a contact at their number", () => {
    expect(replyAddress(note("Zoe", "omw"), [], contacts)).toBe("+13025550123");
    expect(replyAddress(note("zoe ", "omw"), [], contacts)).toBe("+13025550123");
  });

  it("never offers a pop-up reply when the name has several numbers, even if one just texted", () => {
    // Two different Sams: the pop-up's reply box can't show which number it sends to, and the
    // most recent texter isn't necessarily who this notification is from.
    expect(replyAddress(note("Sam", "hey"), [], contacts)).toBeNull();
    expect(replyAddress(note("Sam", "hey"), [sms("+13025550112", "Sam")], contacts)).toBeNull();
    expect(replyTarget(note("Sam", "hey"), [sms("+13025550112", "Sam")], contacts)).toEqual({ to: null, ambiguous: true });
    // One person whose texts came from two numbers is just as ambiguous for a blind reply.
    const twoPhones = [sms("+13025550123", "Zoe"), sms("+13025550124", "Zoe")];
    expect(replyTarget(note("Zoe", "hey"), twoPhones, contacts)).toEqual({ to: null, ambiguous: true });
  });

  it("a single number is not ambiguous", () => {
    expect(replyTarget(note("Zoe", "omw"), [], contacts)).toEqual({ to: "+13025550123", ambiguous: false });
    expect(replyTarget(note("Pat", "hi"), [], contacts)).toEqual({ to: null, ambiguous: false });
  });

  it("uses the number or email iOS shows for someone not in contacts", () => {
    expect(replyAddress(note("+1 (302) 555-0199", "who dis"), [], contacts)).toBe("+13025550199");
    expect(replyAddress(note("pat@example.com", "hi"), [], contacts)).toBe("pat@example.com");
    expect(replyAddress(note("Pat", "hi"), [], contacts)).toBeNull();
    // Short codes are real senders (reply STOP); they reply to their digits.
    expect(replyAddress(note("12 34", "hi"), [], contacts)).toBe("1234");
  });

  it("only offers replies for Messages (that's what message access sends)", () => {
    expect(replyAddress(note("Zoe", "hi", { appId: "net.whatsapp.WhatsApp", appName: "WhatsApp" }), [], contacts)).toBeNull();
    expect(replyAddress(note("", "hi"), [], contacts)).toBeNull();
  });
});

describe("toastSpec", () => {
  it("a text from a person: reply box and Mark read (which also clears)", () => {
    const n = note("Zoe", "omw, 10 mins");
    expect(toastSpec(n, [], contacts)).toEqual({
      id: n.id,
      title: "Messages · Zoe",
      body: "omw, 10 mins",
      name: "Zoe",
      replyTo: "+13025550123",
      open: false,
      markRead: true,
      code: null,
      callBack: false,
      clear: true,
      replies: [],
      app: "Messages",
      kind: "message",
    });
  });

  describe("suggested replies", () => {
    const on = { enabled: true, defaults: ["👍", "OK", "Talk soon"] };

    it("a text from a person gets the replies its rule picks, beside the reply box", () => {
      expect(toastSpec(note("Zoe", "where are you?"), [], contacts, on)).toMatchObject({
        replyTo: "+13025550123",
        replies: ["On my way", "5 minutes", "Running late"],
      });
      expect(toastSpec(note("Zoe", "see you at the game"), [], contacts, on).replies).toEqual(["👍", "OK", "Talk soon"]);
    });

    it("none when switched off, or unless the caller passes the settings", () => {
      expect(toastSpec(note("Zoe", "are you free?"), [], contacts, { ...on, enabled: false }).replies).toEqual([]);
      expect(toastSpec(note("Zoe", "are you free?"), [], contacts).replies).toEqual([]);
    });

    it("none without a reply box: groups and unclear numbers, other apps, ringing calls", () => {
      expect(toastSpec(note("Sam", "are you free?"), [], contacts, on)).toMatchObject({ replyTo: null, replies: [] });
      expect(toastSpec(note("Zoe", "are you free?", { appId: "net.whatsapp.WhatsApp", appName: "WhatsApp" }), [], contacts, on).replies).toEqual([]);
      const ringing = note("Zoe", "Incoming Call", { appId: "com.apple.mobilephone", appName: "Phone", category: "incomingCall" });
      expect(toastSpec(ringing, [], contacts, on).replies).toEqual([]);
    });

    it("none for codes, short codes and robots: the Copy code button stays", () => {
      const code = toastSpec(note("+1 (302) 555-0199", "Your verification code is 482913"), [], contacts, on);
      expect(code).toMatchObject({ code: "482913", replies: [] });
      expect(toastSpec(note("72975", "Your table is ready, see you soon"), [], contacts, on)).toMatchObject({ replyTo: "72975", replies: [] });
      expect(toastSpec(note("+1 (302) 555-0199", "Sale ends today! Reply STOP to opt out"), [], contacts, on).replies).toEqual([]);
    });
  });

  it("a text under a name with several numbers: Open in tug instead of a reply box", () => {
    const spec = toastSpec(note("Sam", "you around?"), [sms("+13025550111", "Sam")], contacts);
    expect(spec).toMatchObject({ replyTo: null, open: true, markRead: true });
  });

  it("a one-time code: Copy code", () => {
    const spec = toastSpec(note("Bank", "Your code is 482913. Don't share it."), [], contacts);
    expect(spec.code).toBe("482913");
    const mail = toastSpec(note("Google", "G-591204 is your Google verification code.", { appId: "com.google.Gmail", appName: "Gmail" }), [], contacts);
    expect(mail).toMatchObject({ code: "591204", replyTo: null, markRead: false, clear: true });
  });

  it("a missed call still on the phone: Call back and Clear", () => {
    const missed = note("Mum", "Missed Call", {
      appId: "com.apple.mobilephone",
      appName: "Phone",
      category: "missedCall",
      flags: { silent: false, important: false, preExisting: false, positiveAction: true, negativeAction: true },
      positiveLabel: "Dial",
    });
    expect(toastSpec(missed, [], contacts)).toMatchObject({ callBack: true, clear: true, replyTo: null, markRead: false });
    expect(toastSpec({ ...missed, live: false }, [], contacts)).toMatchObject({ callBack: false, clear: false });
  });

  it("a ringing call gets no buttons at all (its negative action is Decline)", () => {
    const ringing = note("Jane Doe", "Incoming Call", {
      appId: "com.apple.mobilephone",
      appName: "Phone",
      category: "incomingCall",
      flags: { silent: false, important: true, preExisting: false, positiveAction: true, negativeAction: true },
    });
    expect(toastSpec(ringing, [], contacts)).toMatchObject({ replyTo: null, markRead: false, callBack: false, clear: false });
  });

  it("a notification that's gone or can't be cleared offers no Clear", () => {
    expect(toastSpec(note("Zoe", "x", { removedAt: T0 }), [], contacts).clear).toBe(false);
    const sticky = note("x", "y", { appId: "com.apple.mobilecal", appName: "Calendar", flags: { silent: false, important: false, preExisting: false, positiveAction: false, negativeAction: false } });
    expect(toastSpec(sticky, [], contacts)).toMatchObject({ clear: false, markRead: false, replyTo: null });
  });
});

describe("replyReadsConversation", () => {
  const msg = (status: SmsMessage["status"]) => ({ status }) as SmsMessage;
  it("reads the conversation only for a reply the phone took", () => {
    expect(replyReadsConversation(msg("sent"))).toBe(true);
    expect(replyReadsConversation(msg("accepted"))).toBe(true);
    expect(replyReadsConversation(msg("failed"))).toBe(false);
    expect(replyReadsConversation(msg("unconfirmed"))).toBe(false);
    expect(replyReadsConversation(msg("pending"))).toBe(false);
  });

  it("keeps the old behaviour without a stored row", () => {
    expect(replyReadsConversation(null)).toBe(true);
    expect(replyReadsConversation(undefined)).toBe(true);
  });
});

describe("sendOutcome", () => {
  const msg = (status: SmsMessage["status"]) => ({ status }) as SmsMessage;
  it("counts a send as done only when the phone took it, like a pop-up reply", () => {
    expect(sendOutcome(msg("accepted"))).toBe("sent");
    expect(sendOutcome(msg("sent"))).toBe("sent");
    // "May have sent" must not clear the conversation's notifications on the phone.
    expect(sendOutcome(msg("unconfirmed"))).toBe("unconfirmed");
    expect(sendOutcome(msg("pending"))).toBe("unconfirmed");
    expect(sendOutcome(msg("failed"))).toBe("failed");
  });
});