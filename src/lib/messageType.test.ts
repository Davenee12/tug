import { describe, expect, it } from "vitest";
import type { ConversationItem } from "./format";
import { bubbleKind, classifyType, distinguishesIMessage, itemContent, messageContent, messageText, previewOf, textContent } from "./messageType";

const photo = { kind: "photo", count: 1 } as const;
const unknownAttachment = { kind: "attachment", count: null } as const;

describe("messageContent", () => {
  it("shows a wordless MMS or untyped text as a photo-or-attachment chip", () => {
    for (const msgType of ["MMS", null, "EMAIL"]) {
      expect(messageContent({ body: "", msgType })).toEqual({ text: "", media: unknownAttachment, unreadable: false });
    }
    expect(messageText({ body: "", msgType: "MMS" })).toBe("📎 Photo or attachment");
  });

  it("shows iOS's photo line as a photo, whatever type the phone reports", () => {
    // On the test iPhone a photo-only text is SMS_GSM with this exact body.
    for (const msgType of ["SMS_GSM", "MMS", "IM", null]) {
      expect(messageContent({ body: "Attachment: 1 Photo", msgType })).toEqual({ text: "", media: photo, unreadable: false });
    }
    expect(messageContent({ body: "Attachment: 1 Sticker", msgType: "SMS_GSM" }).media).toEqual({ kind: "sticker", count: 1 });
    expect(messageText({ body: "Attachment: 2 Photos", msgType: "SMS_GSM" })).toBe("📷 2 photos");
  });

  it("keeps a caption beside the chip", () => {
    expect(messageContent({ body: "Look!\nAttachment: 1 Photo", msgType: "SMS_GSM" })).toEqual({ text: "Look!", media: photo, unreadable: false });
    expect(messageText({ body: "Look!\nAttachment: 1 Photo", msgType: "SMS_GSM" })).toBe("📷 Look!");
  });

  it("keeps the text when there is one, and plain texts stay as they were", () => {
    expect(messageText({ body: "look!", msgType: "MMS" })).toBe("look!");
    expect(messageText({ body: "", msgType: "SMS_GSM" })).toBe("(no preview)");
    expect(messageText({ body: "", msgType: "IM" })).toBe("(no preview)");
  });

  it("says when a text's words didn't come through", () => {
    expect(messageContent({ body: "�", msgType: "SMS_GSM" })).toEqual({ text: "", media: null, unreadable: true });
    expect(messageText({ body: "�", msgType: "SMS_GSM" })).toBe("This text didn't come through");
    // One bad character among words is still the words.
    expect(messageText({ body: "caf� ok", msgType: "SMS_GSM" })).toBe("caf� ok");
  });

  it("shows a text sent from tug exactly as typed", () => {
    expect(messageContent({ body: "Attachment: 1 Photo", msgType: null, direction: "out" })).toEqual({
      text: "Attachment: 1 Photo",
      media: null,
      unreadable: false,
    });
  });

  it("can show a body other than the stored one (filled from the notification)", () => {
    expect(messageContent({ body: "�", msgType: "SMS_GSM", direction: "in" }, "on my way").text).toBe("on my way");
  });
});

describe("itemContent", () => {
  const notification = (appId: string, body: string) => ({ kind: "notification", body, n: { appId } }) as unknown as ConversationItem;
  const message = (body: string, itemBody = body) =>
    ({ kind: "message", body: itemBody, m: { body, msgType: "SMS_GSM", direction: "in" } }) as unknown as ConversationItem;

  it("reads Messages notifications the same way as texts", () => {
    expect(itemContent(notification("com.apple.MobileSMS", "Attachment: 1 Photo")).media).toEqual(photo);
    expect(itemContent(message("Attachment: 1 Photo")).media).toEqual(photo);
  });

  it("leaves other chat apps' notifications alone", () => {
    expect(itemContent(notification("net.whatsapp.WhatsApp", "Attachment: 1 Photo"))).toEqual({
      text: "Attachment: 1 Photo",
      media: null,
      unreadable: false,
    });
    expect(itemContent(notification("net.whatsapp.WhatsApp", "")).text).toBe("(no preview)");
  });

  it("uses the item's body, so a filled-in text shows its words", () => {
    expect(itemContent(message("�", "see you soon"))).toEqual({ text: "see you soon", media: null, unreadable: false });
  });
});

describe("previewOf / textContent", () => {
  it("one line for each case", () => {
    expect(previewOf(textContent("Attachment: 1 Photo"))).toBe("📷 Photo");
    expect(previewOf(textContent("�"))).toBe("This text didn't come through");
    expect(previewOf(textContent("hi"))).toBe("hi");
  });
});

const m = (msgType: string | null) => ({ msgType });

describe("classifyType", () => {
  it("maps IM to iMessage and the SMS/MMS kinds to text", () => {
    expect(classifyType("IM")).toBe("imessage");
    expect(classifyType("SMS_GSM")).toBe("sms");
    expect(classifyType("SMS_CDMA")).toBe("sms");
    expect(classifyType("MMS")).toBe("sms");
  });

  it("is case-insensitive", () => {
    expect(classifyType("im")).toBe("imessage");
    expect(classifyType("sms_gsm")).toBe("sms");
  });

  it("treats EMAIL, unknown and missing types as neutral", () => {
    expect(classifyType("EMAIL")).toBe("unknown");
    expect(classifyType("")).toBe("unknown");
    expect(classifyType(null)).toBe("unknown");
    expect(classifyType(undefined)).toBe("unknown");
  });
});

describe("distinguishesIMessage", () => {
  it("is true only when both an iMessage and a text are present", () => {
    expect(distinguishesIMessage([m("IM"), m("SMS_GSM")])).toBe(true);
    expect(distinguishesIMessage([m("SMS_GSM"), m("MMS"), m("IM")])).toBe(true);
  });

  it("is false when the phone reports everything the same, or nothing", () => {
    expect(distinguishesIMessage([m("IM"), m("IM")])).toBe(false);
    expect(distinguishesIMessage([m("SMS_GSM"), m("SMS_GSM")])).toBe(false);
    expect(distinguishesIMessage([m(null), m(null)])).toBe(false);
    expect(distinguishesIMessage([])).toBe(false);
    // EMAIL alongside IM isn't an SMS, so still no distinction.
    expect(distinguishesIMessage([m("IM"), m("EMAIL")])).toBe(false);
  });
});

describe("bubbleKind", () => {
  it("colours by type only when the phone distinguishes", () => {
    expect(bubbleKind(m("IM"), true)).toBe("imessage");
    expect(bubbleKind(m("SMS_GSM"), true)).toBe("sms");
    expect(bubbleKind(m("IM"), false)).toBeNull();
    expect(bubbleKind(m(null), true)).toBeNull();
    expect(bubbleKind(m("EMAIL"), true)).toBeNull();
  });
});
