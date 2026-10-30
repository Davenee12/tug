import { describe, expect, it } from "vitest";
import { contactRecipients, fold, recentRecipients, recipientOptions, RECENT_SHOWN, type Recipient } from "./recipients";
import type { Conversation } from "./format";

const conv = (contact: string, address: string | null, appId = "com.apple.MobileSMS") =>
  ({ key: `k:${contact}`, appId, appLabel: "Messages", contact, address, addresses: address ? [address] : [] }) as unknown as Conversation;

const recentOf = (...names: Array<[string, string]>): Recipient[] =>
  names.map(([name, address]) => ({ key: `r:${address}`, name, address, section: "Recent" }));

describe("recentRecipients", () => {
  it("keeps texts with a known number, newest first, and leaves out unknown senders", () => {
    const convs = [
      conv("Zoe", "+13025550101"),
      conv("Spam", "+13025550199"),
      conv("Slack", "x", "com.tinyspeck.chatlyio"),
      conv("Sam", null),
      conv("Ana", "+13025550102"),
    ];
    const out = recentRecipients(convs, (c) => c.contact !== "Spam");
    expect(out.map((r) => r.name)).toEqual(["Zoe", "Ana"]);
    expect(out[0]).toEqual({ key: "r:+13025550101", name: "Zoe", address: "+13025550101", section: "Recent" });
  });
});

describe("contactRecipients", () => {
  it("lists contacts A to Z", () => {
    const out = contactRecipients([
      { address: "+13025550103", name: "Zoe" },
      { address: "+13025550104", name: "ana" },
      { address: "+13025550105", name: "Marco" },
    ]);
    expect(out.map((r) => r.name)).toEqual(["ana", "Marco", "Zoe"]);
    expect(out.every((r) => r.section === "Contacts")).toBe(true);
  });
});

describe("recipientOptions", () => {
  const contacts = contactRecipients([
    { address: "+13025550101", name: "Zoe" },
    { address: "+13025550104", name: "José Ruiz" },
    { address: "+13025550105", name: "Marco" },
  ]);

  it("with nothing typed: the people you text first, then everyone else, each number once", () => {
    const recent = recentOf(["Zoe", "+13025550101"], ["Dad", "+13025550106"]);
    const out = recipientOptions("", recent, contacts);
    expect(out.map((o) => `${o.section}:${o.name}`)).toEqual([
      "Recent:Zoe",
      "Recent:Dad",
      "Contacts:José Ruiz",
      "Contacts:Marco",
    ]);
  });

  it("shows only the most recent few people first when nothing is typed", () => {
    const recent = recentOf(...Array.from({ length: RECENT_SHOWN + 3 }, (_, i): [string, string] => [`P${i}`, `+130255502${i}0`]));
    const out = recipientOptions("", recent, []);
    expect(out).toHaveLength(RECENT_SHOWN);
  });

  it("matches names without minding case or accents, and numbers by 3+ digits", () => {
    expect(recipientOptions("jose", [], contacts).map((o) => o.name)).toEqual(["José Ruiz"]);
    expect(recipientOptions("0105", [], contacts).map((o) => o.name)).toEqual(["Marco"]);
    // Two digits isn't a number search.
    expect(recipientOptions("01", [], contacts)).toEqual([]);
    expect(fold("Zoë")).toBe("zoe");
  });

  it("offers a typed number nobody has, first", () => {
    const out = recipientOptions("+44 20 7946 0958", [], contacts);
    expect(out[0].section).toBe("Number");
    expect(out[0].address.replace(/\D/g, "")).toBe("442079460958");
  });

  it("doesn't offer a typed number someone already has", () => {
    const out = recipientOptions("302 555 0101", [], contacts);
    expect(out.map((o) => o.section)).toEqual(["Contacts"]);
  });
});
