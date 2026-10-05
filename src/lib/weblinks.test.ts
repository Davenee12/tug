import { describe, expect, it } from "vitest";
import { gmailQuery, webLinkFor, GMAIL_APP_ID } from "./weblinks";
import type { PhoneNotification } from "../types/protocol";

function note(extra: Partial<PhoneNotification> = {}): PhoneNotification {
  return {
    id: 1,
    appId: "com.google.Gmail",
    appName: "Gmail",
    category: "email",
    title: "",
    subtitle: "",
    message: "",
    postedAt: null,
    receivedAt: 0,
    flags: { silent: false, important: false, preExisting: false, positiveAction: false, negativeAction: true },
    positiveLabel: "",
    negativeLabel: "Clear",
    removedAt: null,
    live: true,
    ...extra,
  };
}

describe("gmailQuery", () => {
  it("builds from sender and the first line of the message", () => {
    expect(gmailQuery({ title: "Jane Doe", subtitle: "", message: "Your order shipped\nArriving Tuesday" })).toBe(
      'from:"Jane Doe" "Your order shipped"',
    );
  });

  it("prefers the subtitle as the subject when present", () => {
    expect(gmailQuery({ title: "Jane Doe", subtitle: "Your order shipped", message: "Arriving Tuesday" })).toBe(
      'from:"Jane Doe" "Your order shipped"',
    );
  });

  it("strips double quotes from both sender and subject", () => {
    expect(gmailQuery({ title: 'Jane "JD" Doe', subtitle: 'Re: "the plan"', message: "" })).toBe(
      'from:"Jane JD Doe" "Re: the plan"',
    );
  });

  it("collapses whitespace and newlines", () => {
    expect(gmailQuery({ title: "  Jane   Doe ", subtitle: "", message: "Order\t shipped \n more" })).toBe(
      'from:"Jane Doe" "Order shipped"',
    );
  });

  it("uses only the sender when there is no subject", () => {
    expect(gmailQuery({ title: "Google", subtitle: "", message: "" })).toBe('from:"Google"');
  });

  it("uses only the subject when there is no sender", () => {
    expect(gmailQuery({ title: "", subtitle: "Security alert", message: "" })).toBe('"Security alert"');
  });

  it("returns an empty string when there is nothing to search on", () => {
    expect(gmailQuery({ title: "   ", subtitle: "", message: "\n \t" })).toBe("");
  });

  it("trims an overlong query to a sane length", () => {
    const long = "x".repeat(300);
    const q = gmailQuery({ title: "Jane", subtitle: long, message: "" });
    expect(q.length).toBeLessThanOrEqual(120);
    expect(q.startsWith('from:"Jane" "xxx')).toBe(true);
  });
});

describe("webLinkFor", () => {
  it("deep-searches Gmail from the notification, url-encoding the query", () => {
    const link = webLinkFor(note({ title: "Jane Doe", message: "Your order shipped" }));
    expect(link).not.toBeNull();
    expect(link!.label).toBe("Gmail");
    expect(link!.url).toBe(
      "https://mail.google.com/mail/u/0/#search/" + encodeURIComponent('from:"Jane Doe" "Your order shipped"'),
    );
    // Spaces and quotes must be percent-encoded, not raw, in the URL fragment.
    expect(link!.url).not.toContain('"');
    expect(link!.url).not.toContain(" ");
  });

  it("falls back to the Gmail inbox when there is nothing to search", () => {
    const link = webLinkFor(note({ title: "", subtitle: "", message: "" }));
    expect(link!.url).toBe("https://mail.google.com/mail/u/0/#inbox");
    expect(link!.label).toBe("Gmail");
  });

  it("confirms Gmail is matched by its bundle id constant", () => {
    expect(note().appId).toBe(GMAIL_APP_ID);
  });

  it("returns a fixed landing page for mapped apps", () => {
    expect(webLinkFor(note({ appId: "com.linkedin.LinkedIn", title: "Someone" }))).toEqual({
      url: "https://www.linkedin.com/notifications/",
      label: "LinkedIn",
    });
    expect(webLinkFor(note({ appId: "com.github.stormbreaker.prod", title: "octocat" }))).toEqual({
      url: "https://github.com/notifications",
      label: "GitHub",
    });
  });

  it("ignores the sender/subject for non-Gmail apps (static page only)", () => {
    const a = webLinkFor(note({ appId: "com.spotify.client", title: "New release", message: "x" }));
    const b = webLinkFor(note({ appId: "com.spotify.client", title: "Something else", message: "y" }));
    expect(a).toEqual(b);
  });

  it("returns null for apps with no useful web page", () => {
    expect(webLinkFor(note({ appId: "com.apple.MobileSMS", title: "Tay" }))).toBeNull();
    expect(webLinkFor(note({ appId: "com.apple.mobilephone", title: "Mum" }))).toBeNull();
    expect(webLinkFor(note({ appId: "com.unknown.app", title: "x" }))).toBeNull();
  });

  it("only produces http(s) urls", () => {
    for (const appId of [
      "com.google.Gmail",
      "com.linkedin.LinkedIn",
      "net.whatsapp.WhatsApp",
      "notion.id",
      "com.amazon.Amazon",
    ]) {
      const link = webLinkFor(note({ appId, title: "a", message: "b" }));
      expect(link!.url).toMatch(/^https:\/\//);
    }
  });
});
