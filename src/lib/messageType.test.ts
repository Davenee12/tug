import { describe, expect, it } from "vitest";
import { ATTACHMENT_ONLY, bubbleKind, classifyType, distinguishesIMessage, messageText } from "./messageType";

describe("messageText", () => {
  it("says a wordless MMS or untyped text is a photo or attachment", () => {
    expect(messageText({ body: "", msgType: "MMS" })).toBe(ATTACHMENT_ONLY);
    expect(messageText({ body: "", msgType: null })).toBe(ATTACHMENT_ONLY);
    expect(messageText({ body: "", msgType: "EMAIL" })).toBe(ATTACHMENT_ONLY);
  });

  it("keeps the text when there is one, and plain texts stay as they were", () => {
    expect(messageText({ body: "look!", msgType: "MMS" })).toBe("look!");
    expect(messageText({ body: "", msgType: "SMS_GSM" })).toBe("(no preview)");
    expect(messageText({ body: "", msgType: "IM" })).toBe("(no preview)");
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
