import { describe, expect, it } from "vitest";
import type { PhoneNotification, ShareState } from "../types/protocol";
import {
  awaitingShareAnswer,
  callerLine,
  NOT_SHARING,
  previewsHidden,
  SHARE_UNKNOWN,
  SHARE_WATCH_START,
  shareBanner,
  shareWatchAnswered,
  shareWatchAsked,
  stillRevealed,
  toastKind,
} from "./sharePrivacy";
import paletteVue from "../components/SearchPalette.vue?raw";
import { toastSpec } from "./toastSpec";

const sharing: ShareState = { active: true, reason: "sharing" };
const presenting: ShareState = { active: true, reason: "presenting" };
const fullScreen: ShareState = { active: true, reason: "fullScreen" };

function n(over: Partial<PhoneNotification> = {}): PhoneNotification {
  return {
    id: 1,
    appId: "com.apple.MobileSMS",
    appName: "Messages",
    category: "social",
    title: "Mom",
    subtitle: "",
    message: "call me when you can",
    postedAt: null,
    receivedAt: 0,
    flags: { silent: false, important: false, preExisting: false, positiveAction: false, negativeAction: true },
    positiveLabel: "",
    negativeLabel: "Clear",
    removedAt: null,
    live: true,
    fresh: false,
    ...over,
  };
}

describe("previewsHidden", () => {
  it("hides while sharing with the setting on, until Show anyway", () => {
    expect(previewsHidden(true, sharing, false)).toBe(true);
    expect(previewsHidden(true, sharing, true)).toBe(false);
    expect(previewsHidden(false, sharing, false)).toBe(false);
    expect(previewsHidden(true, NOT_SHARING, false)).toBe(false);
  });
});

describe("waiting for the backend's first answer", () => {
  it("hides previews from the moment the window shows until the backend answers", () => {
    // tug comes back on screen: the last answer was "not sharing", but that's from before.
    const asked = shareWatchAsked(SHARE_WATCH_START);
    const awaiting = awaitingShareAnswer(asked, true, true);
    expect(awaiting).toBe(true);
    expect(previewsHidden(true, NOT_SHARING, false, awaiting)).toBe(true);
    // The answer for this ask: whatever it says now decides.
    const answered = shareWatchAnswered(asked, asked.episode);
    expect(awaitingShareAnswer(answered, true, true)).toBe(false);
    expect(previewsHidden(true, NOT_SHARING, false, awaitingShareAnswer(answered, true, true))).toBe(false);
    expect(previewsHidden(true, sharing, false, awaitingShareAnswer(answered, true, true))).toBe(true);
  });

  it("only counts the answer to the newest ask", () => {
    // Shown, hidden and shown again before the first answer came back.
    const first = shareWatchAsked(SHARE_WATCH_START);
    const third = shareWatchAsked(shareWatchAsked(first));
    const late = shareWatchAnswered(third, first.episode);
    expect(awaitingShareAnswer(late, true, true)).toBe(true);
    expect(awaitingShareAnswer(shareWatchAnswered(third, third.episode), true, true)).toBe(false);
  });

  it("doesn't wait with the setting off or the window hidden, and Show anyway still works", () => {
    const asked = shareWatchAsked(SHARE_WATCH_START);
    expect(awaitingShareAnswer(asked, true, false)).toBe(false);
    expect(awaitingShareAnswer(asked, false, true)).toBe(false);
    expect(previewsHidden(false, NOT_SHARING, false, true)).toBe(false);
    expect(previewsHidden(true, sharing, true, true)).toBe(false);
  });

  it("assumes hidden, and says why, when the backend can't be asked", () => {
    expect(previewsHidden(true, SHARE_UNKNOWN, false)).toBe(true);
    expect(shareBanner(true, SHARE_UNKNOWN, false)).toBe("Previews hidden until tug can check your screen");
  });
});

describe("search (Ctrl+K) while previews are hidden", () => {
  // No component test runner here, so read the markup: the palette must hide its own results.
  const section = (from: string, to: string) => {
    const start = paletteVue.indexOf(from);
    const end = paletteVue.indexOf(to, start);
    expect(start, from).toBeGreaterThan(-1);
    expect(end, to).toBeGreaterThan(start);
    return paletteVue.slice(start, end);
  };

  it("blurs inside the palette whenever the Feed would, so Show anyway reveals both", () => {
    expect(paletteVue).toContain("tug.previewsHidden && 'previews-hidden'");
  });

  it("marks people, texts and notifications private, and leaves tug's own actions alone", () => {
    const person = section("<!-- Person -->", "<!-- Text message -->");
    expect(person.match(/preview-private/g)?.length, "name and number").toBe(2);
    const text = section("<!-- Text message -->", "<!-- Notification -->");
    expect(text.match(/preview-private/g)?.length, "sender and text").toBe(2);
    // The sender's avatar is theirs (photo or initials), so it blurs like the Feed's.
    expect(text).toMatch(/<AppAvatar[^>]*\bperson\b/);
    const notification = section("<!-- Notification -->", "</button>");
    expect(notification.match(/preview-private/g)?.length, "title and text").toBe(2);
    expect(section("<!-- Action -->", "<!-- Person -->")).not.toContain("preview-private");
  });
});

describe("stillRevealed", () => {
  it("keeps Show anyway for the share in progress only", () => {
    expect(stillRevealed(true, sharing)).toBe(true);
    expect(stillRevealed(true, presenting)).toBe(true);
    // Sharing stopped: forgotten, so the next share hides again.
    expect(stillRevealed(true, NOT_SHARING)).toBe(false);
    expect(stillRevealed(false, sharing)).toBe(false);
  });
});

describe("shareBanner", () => {
  it("says why, and clears the moment sharing stops", () => {
    expect(shareBanner(true, sharing, false)).toBe("Previews hidden while you share your screen");
    expect(shareBanner(true, presenting, false)).toBe("Previews hidden while you present");
    expect(shareBanner(true, fullScreen, false)).toBe("Previews hidden while an app is full screen");
    expect(shareBanner(true, sharing, true)).toBe("Previews showing while you share your screen");
    expect(shareBanner(true, NOT_SHARING, false)).toBeNull();
    expect(shareBanner(false, sharing, false)).toBeNull();
    // An active state with no reason still says something true.
    expect(shareBanner(true, { active: true, reason: null }, false)).toBe("Previews hidden while you share your screen");
  });

  it("says so when tug couldn't check the screen, and hides to be safe", () => {
    const unknown: ShareState = { active: true, reason: "unknown" };
    expect(previewsHidden(true, unknown, false)).toBe(true);
    expect(shareBanner(true, unknown, false)).toBe("Previews hidden until tug can check your screen");
    expect(shareBanner(true, unknown, true)).toBe("Previews showing, though tug can't check your screen right now");
    expect(shareBanner(false, unknown, false)).toBeNull();
  });
});

describe("toastKind", () => {
  it("tells the backend what to call a hidden pop-up", () => {
    expect(toastKind(n())).toBe("message");
    expect(toastKind(n({ appId: "com.google.Gmail", appName: "Gmail", category: "email" }))).toBe("notification");
    expect(toastKind(n({ appId: "com.apple.mobilephone", category: "incomingCall" }))).toBe("call");
    expect(toastKind(n({ appId: "com.apple.mobilephone", category: "missedCall" }))).toBe("missedCall");
  });

  it("is on every notification pop-up, with the app's name", () => {
    const spec = toastSpec(n(), [], []);
    expect(spec.kind).toBe("message");
    expect(spec.app).toBe("Messages");
    const mail = toastSpec(n({ appId: "com.google.Gmail", appName: "Gmail", title: "Dr. Lee", category: "email" }), [], []);
    expect(mail.kind).toBe("notification");
    expect(mail.app).toBe("Gmail");
  });
});

describe("callerLine", () => {
  it("names the caller unless previews are hidden", () => {
    expect(callerLine("Jane Doe", false)).toBe("Jane Doe");
    expect(callerLine("Jane Doe", true)).toBe("Incoming call");
  });
});
