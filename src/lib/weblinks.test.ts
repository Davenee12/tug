import { describe, expect, it } from "vitest";
import { webLinkFor, GMAIL_APP_ID } from "./weblinks";
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

describe("webLinkFor", () => {
  it("opens Gmail's inbox, whatever the email says", () => {
    const link = webLinkFor(note({ title: "Campus Reach", subtitle: "Southwest Careers | Summer internship applications are closing soon!" }));
    expect(link).toEqual({ url: "https://mail.google.com/mail/u/0/#inbox", label: "Gmail" });
    expect(webLinkFor(note())?.url).toBe("https://mail.google.com/mail/u/0/#inbox");
  });

  it("searches Google for what a Google app alert is about", () => {
    const n = note({ appId: "com.google.GoogleMobile", title: "Google", subtitle: "", message: "⏰ France vs Belgium · Tap to add the live score to your lock screen" });
    expect(webLinkFor(n)?.url).toBe(`https://www.google.com/search?q=${encodeURIComponent("⏰ France vs Belgium")}`);
  });

  it("never offers a web page for Messages or Phone", () => {
    expect(webLinkFor(note({ appId: "com.apple.MobileSMS" }))).toBeNull();
    expect(webLinkFor(note({ appId: "com.apple.mobilephone" }))).toBeNull();
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
