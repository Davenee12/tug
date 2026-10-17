import { describe, expect, it } from "vitest";
import {
  cleanReplies,
  conversationReplies,
  DEFAULT_REPLIES,
  isShortCode,
  looksAutomated,
  MAX_REPLIES,
  parseReplyDefaults,
  REPLY_MAX_CHARS,
  ruleFor,
  suggestReplies,
} from "./smartReplies";
import type { Conversation, ConversationItem } from "./format";
import type { PhoneNotification, SmsMessage } from "../types/protocol";

const on = { enabled: true, defaults: [...DEFAULT_REPLIES] };
const say = (text: string, sender: string | null = "+13025550123") => suggestReplies(text, { ...on, sender });

describe("suggestReplies: the rules", () => {
  it("questions get Yes / No / Let me check", () => {
    for (const text of ["Free tonight?", "are you coming to the party", "Can you grab milk", "did you see the game", "what time is dinner", "Do you still have my charger"]) {
      expect(say(text), text).toEqual(["Yes", "No", "Let me check"]);
    }
  });

  it("where / ETA gets On my way / 5 minutes / Running late, before it counts as a question", () => {
    for (const text of ["Where are you?", "where r u", "ETA?", "how long till you're here", "How far away are you", "When will you be here?"]) {
      expect(say(text), text).toEqual(["On my way", "5 minutes", "Running late"]);
    }
  });

  it("them on their way gets See you soon / 👍 / Drive safe, never On my way back", () => {
    for (const text of ["omw", "OMW!", "omw, 10 mins 🚗", "On my way", "on my way, where are you?", "Be there soon"]) {
      expect(say(text), text).toEqual(["See you soon", "👍", "Drive safe"]);
      expect(say(text), text).not.toContain("On my way");
    }
  });

  it("asking whether you're on your way is a question, not an ETA or an acknowledgement", () => {
    expect(ruleFor("Are you on your way?")).toBe("question");
    expect(ruleFor("on your way yet?")).toBe("question");
  });

  it("thanks gets You're welcome / 👍", () => {
    for (const text of ["Thanks!", "thank you so much", "thx for dinner", "ty", "Cheers mate", "Really appreciate it"]) {
      expect(say(text), text).toEqual(["You're welcome", "👍"]);
    }
  });

  it("call requests get Calling you now / Call you later, before they count as questions", () => {
    for (const text of ["Call me", "can you call me when you're free?", "Give me a ring later", "call me back pls"]) {
      expect(say(text), text).toEqual(["Calling you now", "Call you later"]);
    }
  });

  it("anything else gets the default list", () => {
    expect(say("Running 10 min behind, sorry")).toEqual(["👍", "OK", "Talk soon"]);
    expect(say("Happy birthday 🎉")).toEqual(["👍", "OK", "Talk soon"]);
  });

  it("a question that's also a thank-you is answered as the question", () => {
    expect(ruleFor("Thanks! Are you free tomorrow?")).toBe("question");
    expect(ruleFor("thanks for today")).toBe("thanks");
  });

  it("matches whole words only", () => {
    // "beta" and "Metal" aren't "eta", "Tyler" isn't "ty".
    expect(ruleFor("the beta is out")).toBe("default");
    expect(ruleFor("Tyler says hi")).toBe("default");
    expect(ruleFor("Metal concert")).toBe("default");
  });

  it("never more than three", () => {
    expect(say("Where are you?").length).toBeLessThanOrEqual(MAX_REPLIES);
  });
});

describe("suggestReplies: when there are none", () => {
  it("switched off", () => {
    expect(suggestReplies("Where are you?", { ...on, enabled: false })).toEqual([]);
  });

  it("nothing to read: empty, blank, or no preview", () => {
    expect(say("")).toEqual([]);
    expect(say("   ")).toEqual([]);
    expect(suggestReplies(null, on)).toEqual([]);
    expect(suggestReplies(undefined, on)).toEqual([]);
  });

  it("verification codes, using tug's code detection", () => {
    expect(say("Your verification code is 482913")).toEqual([]);
    expect(say("G-591204 is your Google verification code.")).toEqual([]);
    expect(say("Your login code: 4417-993. Don't share it. Did you request it?")).toEqual([]);
  });

  it("short codes (6 digits or fewer), whatever they say", () => {
    expect(say("Your order is on its way, where should we leave it?", "72975")).toEqual([]);
    expect(say("Thanks for shopping!", "123456")).toEqual([]);
    expect(say("Thanks for shopping!", "+1 (302) 555-0123")).toEqual(["You're welcome", "👍"]);
  });

  it("opt-out and no-reply wording", () => {
    for (const text of [
      "Flash sale today! Reply STOP to opt out",
      "Txt STOP to end",
      "Your parcel arrives today. Do not reply to this message.",
      "Msg & data rates may apply",
      "Click to unsubscribe",
    ]) {
      expect(say(text), text).toEqual([]);
    }
  });

  it("an emptied default list means none when no rule matches (the rules still work)", () => {
    expect(suggestReplies("Happy birthday", { enabled: true, defaults: [], sender: "+13025550123" })).toEqual([]);
    expect(suggestReplies("Thanks!", { enabled: true, defaults: [], sender: "+13025550123" })).toEqual(["You're welcome", "👍"]);
  });
});

describe("isShortCode / looksAutomated", () => {
  it("short codes are 6 digits or fewer, not numbers, emails or names", () => {
    expect(isShortCode("72975")).toBe(true);
    expect(isShortCode("262-966")).toBe(true);
    expect(isShortCode("+13025550123")).toBe(false);
    expect(isShortCode("3025550123")).toBe(false);
    expect(isShortCode("pat@example.com")).toBe(false);
    expect(isShortCode("Zoe")).toBe(false);
    expect(isShortCode("")).toBe(false);
    expect(isShortCode(null)).toBe(false);
  });

  it("automated texts are codes and opt-outs, not ordinary chat", () => {
    expect(looksAutomated("Your code is 482913")).toBe(true);
    expect(looksAutomated("Reply STOP to cancel")).toBe(true);
    expect(looksAutomated("Stop it, you're hilarious")).toBe(false);
    expect(looksAutomated("Meet at 5?")).toBe(false);
  });
});

describe("cleanReplies / parseReplyDefaults", () => {
  it("trims, drops blanks, repeats, overlong and non-text, keeps three", () => {
    const long = "x".repeat(REPLY_MAX_CHARS + 1);
    expect(cleanReplies(["  OK ", "", "ok", long, 7, "Sure", "On it", "Later"])).toEqual(["OK", "Sure", "On it"]);
    expect(cleanReplies(["x".repeat(REPLY_MAX_CHARS)])).toHaveLength(1);
    expect(cleanReplies(null)).toEqual([]);
  });

  it("counts characters, so an emoji isn't four", () => {
    expect(cleanReplies(["🚗".repeat(REPLY_MAX_CHARS)])).toEqual(["🚗".repeat(REPLY_MAX_CHARS)]);
  });

  it("turns control characters into spaces", () => {
    expect(cleanReplies(["On\nmy\u0000way"])).toEqual(["On my way"]);
  });

  it("reads the saved list: missing means the defaults, an emptied list stays empty, junk falls back", () => {
    expect(parseReplyDefaults(undefined)).toEqual([...DEFAULT_REPLIES]);
    expect(parseReplyDefaults("")).toEqual([...DEFAULT_REPLIES]);
    expect(parseReplyDefaults('["Sure","Later"]')).toEqual(["Sure", "Later"]);
    expect(parseReplyDefaults("[]")).toEqual([]);
    expect(parseReplyDefaults("{oops")).toEqual([...DEFAULT_REPLIES]);
    expect(parseReplyDefaults('"OK"')).toEqual([...DEFAULT_REPLIES]);
  });
});

// The chips above the composer in Messages.
describe("conversationReplies", () => {
  const at = new Date("2026-10-05T12:00:00");
  let id = 1;
  function msg(direction: "in" | "out", body: string, address = "+13025550123"): ConversationItem {
    const m: SmsMessage = {
      id: id++,
      source: "iphone-map",
      direction,
      address,
      contactName: "Zoe",
      body,
      sentAt: null,
      receivedAt: at.getTime(),
      status: direction === "in" ? "received" : "accepted",
      msgType: null,
      gapBefore: false,
    };
    return { kind: "message", id: `m${m.id}`, at, body, m };
  }
  function notification(message: string): ConversationItem {
    const n = { id: id++, appId: "com.apple.MobileSMS", title: "Zoe", subtitle: "", message } as PhoneNotification;
    return { kind: "notification", id: `n${n.id}`, at, body: message, n };
  }
  const conv = (items: ConversationItem[], over: Partial<Conversation> = {}) => ({
    appId: "com.apple.MobileSMS",
    address: "+13025550123",
    items,
    ...over,
  });

  it("answers the newest text when they sent it", () => {
    expect(conversationReplies(conv([msg("out", "hey"), msg("in", "where are you?")]), on)).toEqual(["On my way", "5 minutes", "Running late"]);
  });

  it("answers a notification that hasn't been fetched as a text yet", () => {
    expect(conversationReplies(conv([notification("thanks!")]), on)).toEqual(["You're welcome", "👍"]);
  });

  it("none once you've replied, or while your send is the newest (sending, sent or not sent)", () => {
    expect(conversationReplies(conv([msg("in", "where are you?"), msg("out", "on my way")]), on)).toEqual([]);
  });

  it("none for other chat apps (reply on the phone), empty conversations, or when switched off", () => {
    expect(conversationReplies(conv([msg("in", "where are you?")], { appId: "net.whatsapp.WhatsApp" }), on)).toEqual([]);
    expect(conversationReplies(conv([]), on)).toEqual([]);
    expect(conversationReplies(null, on)).toEqual([]);
    expect(conversationReplies(conv([msg("in", "where are you?")]), { ...on, enabled: false })).toEqual([]);
  });

  it("none for short codes and code texts", () => {
    expect(conversationReplies(conv([msg("in", "Thanks for your order!", "72975")], { address: "72975" }), on)).toEqual([]);
    expect(conversationReplies(conv([notification("Your code is 482913")], { address: "72975" }), on)).toEqual([]);
    expect(conversationReplies(conv([msg("in", "Your verification code is 482913")]), on)).toEqual([]);
  });
});
