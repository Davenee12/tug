import { describe, expect, it } from "vitest";
import { findSmartActions, sameUrlKey, smartChips, smartSegments, type SmartKind } from "./smartActions";

/** What was found, as [kind, text] pairs in reading order. */
const found = (body: string | null) => findSmartActions(body).map((a) => [a.kind, a.text]);
const only = (body: string, kind: SmartKind) => findSmartActions(body).filter((a) => a.kind === kind).map((a) => a.text);

describe("findSmartActions: links", () => {
  it("opens http(s) links as written, without the sentence's punctuation", () => {
    expect(found("see https://example.com/a?b=1#c")).toEqual([["link", "https://example.com/a?b=1#c"]]);
    expect(found("Read this: https://example.com/post.")).toEqual([["link", "https://example.com/post"]]);
    expect(found("(details at https://example.com/x)")).toEqual([["link", "https://example.com/x"]]);
    expect(found("http://example.com, then call")).toEqual([["link", "http://example.com"]]);
    expect(found("“https://example.com/q”")).toEqual([["link", "https://example.com/q"]]);
    expect(found("https://example.com/ok?!")).toEqual([["link", "https://example.com/ok"]]);
    const [link] = findSmartActions("go https://example.com/a");
    expect(link).toMatchObject({ label: "Open link", url: "https://example.com/a", start: 3, end: 24 });
  });

  it("keeps a link's own brackets", () => {
    expect(only("https://en.example.org/wiki/Tug_(boat) is neat", "link")).toEqual(["https://en.example.org/wiki/Tug_(boat)"]);
    expect(only("(https://en.example.org/wiki/Tug_(boat))", "link")).toEqual(["https://en.example.org/wiki/Tug_(boat)"]);
  });

  it("stops a link at an emoji, a quote or the next link", () => {
    expect(only("https://example.com/party🎉", "link")).toEqual(["https://example.com/party"]);
    expect(only('"https://example.com/a"', "link")).toEqual(["https://example.com/a"]);
    expect(only("https://example.com/a,https://example.com/b", "link")).toEqual(["https://example.com/a", "https://example.com/b"]);
  });

  it("opens bare website names with https", () => {
    expect(findSmartActions("try example.com/deals today")[0]).toMatchObject({ text: "example.com/deals", url: "https://example.com/deals" });
    expect(findSmartActions("www.example.org.")[0]).toMatchObject({ text: "www.example.org", url: "https://www.example.org" });
    expect(only("Book at Example.com!", "link")).toEqual(["Example.com"]);
    expect(only("shop.example.co.uk/sale, now", "link")).toEqual(["shop.example.co.uk/sale"]);
    expect(only("t.co/abc123 and wa.me/15550100123", "link")).toEqual(["t.co/abc123", "wa.me/15550100123"]);
    expect(only("EXAMPLE.COM/HELP", "link")).toEqual(["EXAMPLE.COM/HELP"]);
  });

  it("doesn't take words, files or versions for websites", () => {
    for (const body of [
      "this.is fine",
      "please log.in",
      "lol.me too",
      "Hola.es tarde",
      "Ciao.io sono qui",
      "e.g. the photo.jpg and notes.txt",
      "running v1.2.3 now",
      "Mr.Me said hi",
      "ok.Come over",
      "the U.S. and the U.K.",
      "next.js and vue.js",
      "it costs 3.50",
    ]) {
      expect(only(body, "link"), body).toEqual([]);
    }
  });

  it("never treats an email's domain as a website", () => {
    expect(found("mail jane@example.com today")).toEqual([["email", "jane@example.com"]]);
  });

  it("refuses links that aren't to a real host", () => {
    expect(only("http:// nothing", "link")).toEqual([]);
    expect(only("https://localhost:3000/x", "link")).toEqual([]);
    expect(only("https://192.0.2.1/status", "link")).toEqual(["https://192.0.2.1/status"]);
  });

  it("only produces addresses the backend will open: http(s) with no whitespace", () => {
    const bodies = [
      "see example.com/a b",
      "https://example.com/a\tb",
      "Meet at 1 Infinite Loop Way, Cupertino, CA 95014",
      "UPS 1Z999AA10123456784",
    ];
    for (const a of bodies.flatMap((b) => findSmartActions(b))) {
      if (!a.url) continue;
      expect(a.url).toMatch(/^https?:\/\/\S+$/);
    }
  });
});

describe("findSmartActions: email addresses", () => {
  it("copies email addresses, never opens mailto", () => {
    const [a] = findSmartActions("Write to jane.doe+tug@example.com.");
    expect(a).toMatchObject({ kind: "email", text: "jane.doe+tug@example.com", label: "Copy email", copy: "jane.doe+tug@example.com" });
    expect(a.url).toBeUndefined();
    expect(only("support@mail.example.co.uk, thanks", "email")).toEqual(["support@mail.example.co.uk"]);
  });

  it("ignores things that only look a bit like email", () => {
    expect(only("@jane on the app", "email")).toEqual([]);
    expect(only("me@home", "email")).toEqual([]);
  });
});

describe("findSmartActions: phone numbers", () => {
  it("copies numbers in the usual shapes", () => {
    expect(only("Call +1 (555) 010-0123 now", "phone")).toEqual(["+1 (555) 010-0123"]);
    expect(only("my cell is 555-010-0142", "phone")).toEqual(["555-010-0142"]);
    expect(only("(555) 010-0199 anytime", "phone")).toEqual(["(555) 010-0199"]);
    expect(only("555.010.0188", "phone")).toEqual(["555.010.0188"]);
    expect(only("Questions? 1-800-555-0100.", "phone")).toEqual(["1-800-555-0100"]);
    expect(only("UK office +44 20 7946 0958, ask for Sam", "phone")).toEqual(["+44 20 7946 0958"]);
    expect(only("text +44 7700 900123", "phone")).toEqual(["+44 7700 900123"]);
    expect(only("or 07700 900123 or 020 7946 0958", "phone")).toEqual(["07700 900123", "020 7946 0958"]);
    expect(only("+15550100123", "phone")).toEqual(["+15550100123"]);
    const [a] = findSmartActions("Call 555-010-0123");
    expect(a).toMatchObject({ label: "Copy number", copy: "555-010-0123", title: "Copy 555-010-0123" });
  });

  it("drops the sentence's bracket and a next word that's a number", () => {
    expect(only("(call +44 20 7946 0958)", "phone")).toEqual(["+44 20 7946 0958"]);
    expect(only("+44 20 7946 0958 2 days a week", "phone")).toEqual(["+44 20 7946 0958"]);
  });

  it("ignores numbers that aren't phone numbers", () => {
    for (const body of [
      "Order 3025550173 shipped", // bare digits: could be anything
      "555-0123", // no area code
      "Your total is $555 010 0123",
      "meet 10:30-11:45",
      "on 2026-10-27 at 7.30",
      "Ref 555-010-0123-77",
      "card ending 4111 1111 1111 1111",
      "+1 555 010", // too short
      "+1 555 010 0123 4567 8901", // too long
      "growth was 555 010 0123%",
      "ISBN 978-555-010-0123",
    ]) {
      expect(only(body, "phone"), body).toEqual([]);
    }
  });

  it("never offers a one-time code as a number", () => {
    // The code chip copies it; a "Copy number" for the same digits would be a second, wrong button.
    expect(found("Your verification code is 482-9133")).toEqual([]);
    expect(found("Your code is 48291337. Questions? Call 1-800-555-0100")).toEqual([["phone", "1-800-555-0100"]]);
  });
});

describe("findSmartActions: street addresses", () => {
  it("opens addresses in Google Maps", () => {
    const [a] = findSmartActions("Party at 742 Evergreen Terrace? No: 1600 Amphitheatre Pkwy, Mountain View, CA 94043.");
    expect(a).toMatchObject({ kind: "address", text: "1600 Amphitheatre Pkwy, Mountain View, CA 94043", label: "Open in Maps" });
    expect(a.url).toBe(
      `https://www.google.com/maps/search/?api=1&query=${encodeURIComponent("1600 Amphitheatre Pkwy, Mountain View, CA 94043")}`,
    );
  });

  it("finds the usual shapes", () => {
    expect(only("Meet me at 221B Baker St tonight", "address")).toEqual(["221B Baker St"]);
    expect(only("Pick up from 12 Oak Street.", "address")).toEqual(["12 Oak Street"]);
    expect(only("Deliver to 1600 Pennsylvania Ave NW, Washington, DC 20500", "address")).toEqual([
      "1600 Pennsylvania Ave NW, Washington, DC 20500",
    ]);
    expect(only("Now at 350 5th Ave, Apt 4B", "address")).toEqual(["350 5th Ave, Apt 4B"]);
    expect(only("12 St. Marks Pl", "address")).toEqual(["12 St. Marks Pl"]);
    expect(only("SHIP TO 45 MAPLE DR", "address")).toEqual(["45 MAPLE DR"]);
    expect(only("Office: 100 Main St., Suite 200, Springfield, IL 62704-1234 (back door)", "address")).toEqual([
      "100 Main St., Suite 200, Springfield, IL 62704-1234",
    ]);
  });

  it("stops before a next sentence that isn't a city and state", () => {
    expect(only("42 Wallaby Way, Sydney", "address")).toEqual(["42 Wallaby Way"]);
    expect(only("At 9 Elm Rd, Then we eat", "address")).toEqual(["9 Elm Rd"]);
  });

  it("leaves ordinary sentences alone", () => {
    for (const body of [
      "I drove 5 miles down the road",
      "meet at 42 elm st", // lower case: too easily prose
      "Part 2 Down The Road",
      "Top 10 Places To Visit",
      "I have 3 big dogs way too many",
      "see Dr Smith at 5",
      "$40 Main St deals",
      "at 5:30 Main St",
      "NEW: 4 Bedroom House Drive-in",
    ]) {
      expect(only(body, "address"), body).toEqual([]);
    }
  });
});

describe("findSmartActions: parcel tracking", () => {
  it("tracks UPS numbers on ups.com", () => {
    const [a] = findSmartActions("Your UPS parcel 1Z999AA10123456784 is out for delivery");
    expect(a).toMatchObject({ kind: "tracking", label: "Track parcel", url: "https://www.ups.com/track?tracknum=1Z999AA10123456784" });
    expect(a.title).toContain("UPS");
    // As UPS prints it, with spaces: tracked without them.
    expect(findSmartActions("1Z 999 AA1 01 2345 6784")[0]).toMatchObject({
      text: "1Z 999 AA1 01 2345 6784",
      url: "https://www.ups.com/track?tracknum=1Z999AA10123456784",
    });
  });

  it("tracks USPS numbers on usps.com", () => {
    expect(findSmartActions("USPS: 9400111202555842332669 arrives Tue")[0]).toMatchObject({
      kind: "tracking",
      url: "https://tools.usps.com/go/TrackConfirmAction?tLabels=9400111202555842332669",
    });
    expect(only("Tracking 9205 5000 0000 0000 0000 00.", "tracking")).toEqual(["9205 5000 0000 0000 0000 00"]);
    expect(only("label 92001902416523000012", "tracking")).toEqual(["92001902416523000012"]);
    // International mail: the S10 shape, only with its check digit right.
    expect(findSmartActions("Your parcel EC123456785US cleared customs")[0]).toMatchObject({
      url: "https://tools.usps.com/go/TrackConfirmAction?tLabels=EC123456785US",
    });
    expect(only("Ref EC 123 456 785 US", "tracking")).toEqual(["EC 123 456 785 US"]);
    expect(only("Product EC123456789US", "tracking")).toEqual([]);
  });

  it("tracks FedEx numbers only in a text that mentions FedEx", () => {
    expect(findSmartActions("FedEx: 123456789012 will arrive today")[0]).toMatchObject({
      kind: "tracking",
      url: "https://www.fedex.com/fedextrack/?trknbr=123456789012",
    });
    expect(only("Your Fed Ex package 1234 5678 9012", "tracking")).toEqual(["1234 5678 9012"]);
    expect(only("fedex 123456789012345", "tracking")).toEqual(["123456789012345"]);
    expect(only("Order 123456789012 confirmed", "tracking")).toEqual([]);
  });

  it("doesn't take other long numbers for parcels", () => {
    for (const body of [
      "1Z is a nice name",
      "1ZABCDEFGHIJKLMNOP", // no digits at all
      "Account 8400111202555842332669",
      "9400111202555842332", // 19 digits
      "FedEx order 12345678901", // 11 digits
      "FedEx price 1234567890123.50",
    ]) {
      expect(only(body, "tracking"), body).toEqual([]);
    }
  });

  it("prefers a tracking link over the number inside it", () => {
    expect(found("Track: https://www.ups.com/track?tracknum=1Z999AA10123456784")).toEqual([
      ["link", "https://www.ups.com/track?tracknum=1Z999AA10123456784"],
    ]);
  });
});

describe("findSmartActions: mixed", () => {
  it("finds several things in reading order without overlaps", () => {
    const body =
      "Hi! Your parcel 1Z999AA10123456784 ships to 12 Oak Street, Springfield, IL 62704. Questions: help@example.com or (555) 010-0123. Details: example.com/orders/42";
    expect(found(body)).toEqual([
      ["tracking", "1Z999AA10123456784"],
      ["address", "12 Oak Street, Springfield, IL 62704"],
      ["email", "help@example.com"],
      ["phone", "(555) 010-0123"],
      ["link", "example.com/orders/42"],
    ]);
    for (const a of findSmartActions(body)) expect(body.slice(a.start, a.end)).toBe(a.text);
  });

  it("finds nothing in ordinary texts", () => {
    expect(found("omw, 10 mins 🚗")).toEqual([]);
    expect(found("Are we still meeting at 5? I can grab a table if you're running late.")).toEqual([]);
    expect(found("")).toEqual([]);
    expect(found(null)).toEqual([]);
  });
});

describe("smartSegments", () => {
  it("splits a body into plain text and actions that join back exactly", () => {
    const body = "Track 1Z999AA10123456784 or call (555) 010-0123\nthanks!";
    const segs = smartSegments(body);
    expect(segs.map((s) => s.kind)).toEqual(["text", "action", "text", "action", "text"]);
    expect(segs.map((s) => s.text).join("")).toBe(body);
    expect(segs[1]).toMatchObject({ kind: "action", text: "1Z999AA10123456784", action: { kind: "tracking" } });
  });

  it("is one plain segment when there's nothing to do, and empty for no text", () => {
    expect(smartSegments("see you soon")).toEqual([{ kind: "text", text: "see you soon" }]);
    expect(smartSegments("")).toEqual([]);
    expect(smartSegments(undefined)).toEqual([]);
  });

  it("starts and ends with actions without empty text around them", () => {
    expect(smartSegments("example.com/a").map((s) => s.kind)).toEqual(["action"]);
  });
});

describe("smartChips", () => {
  const body =
    "Order shipped! 1Z999AA10123456784. Questions? (555) 010-0123 or (555) 010-0199, help@example.com, example.com/help. Return to 12 Oak Street.";

  it("offers one of each kind, the most specific first, capped", () => {
    expect(smartChips(body).map((a) => a.label)).toEqual(["Track parcel", "Open in Maps", "Open link"]);
    expect(smartChips(body, { limit: 2 }).map((a) => a.kind)).toEqual(["tracking", "address"]);
    expect(smartChips(body, { limit: 10 }).map((a) => a.kind)).toEqual(["tracking", "address", "link", "phone", "email"]);
    expect(smartChips(body, { limit: 0 })).toEqual([]);
  });

  it("doesn't repeat the page the item's own Open button goes to", () => {
    expect(smartChips("Deals at example.com today", { skipUrls: ["https://www.example.com/"] })).toEqual([]);
    expect(smartChips("Deals at example.com/sale", { skipUrls: ["https://www.example.com/"] }).map((a) => a.text)).toEqual([
      "example.com/sale",
    ]);
    expect(smartChips("Deals at example.com", { skipUrls: [null, undefined] }).map((a) => a.text)).toEqual(["example.com"]);
  });

  it("doesn't offer the same page twice", () => {
    const twice = "1Z999AA10123456784: https://www.ups.com/track?tracknum=1Z999AA10123456784";
    expect(smartChips(twice, { limit: 5 }).map((a) => a.kind)).toEqual(["tracking"]);
  });

  it("offers nothing for a code (it has its own chip) or plain chat", () => {
    expect(smartChips("Your Acme verification code is 731904. It expires in 10 minutes.")).toEqual([]);
    expect(smartChips("did you see the photos I sent?")).toEqual([]);
  });
});

describe("sameUrlKey", () => {
  it("ignores scheme, www, host case and a trailing slash", () => {
    expect(sameUrlKey("https://www.Example.com/")).toBe("example.com");
    expect(sameUrlKey("http://example.com")).toBe("example.com");
    expect(sameUrlKey("https://example.com/Path/")).toBe("example.com/Path");
  });
});

describe("texts from someone you don't know", () => {
  const body = "Your parcel 1Z999AA10123456784 is held, pay at https://example.com/pay or call +1 (555) 010-0199";

  it("keep web links as plain words, but still act on what tug builds itself", () => {
    const kinds = (links: boolean) => smartChips(body, { links }).map((a) => a.kind);
    expect(kinds(true)).toContain("link");
    expect(kinds(false)).not.toContain("link");
    expect(kinds(false)).toEqual(expect.arrayContaining(["tracking", "phone"]));
  });

  it("render the link as text, joined back exactly", () => {
    const segments = smartSegments(body, { links: false });
    expect(segments.map((s) => s.text).join("")).toBe(body);
    expect(segments.some((s) => s.kind === "action" && s.action.kind === "link")).toBe(false);
    expect(segments.some((s) => s.kind === "action" && s.action.kind === "tracking")).toBe(true);
  });
});
