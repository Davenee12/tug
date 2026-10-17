import { describe, expect, it } from "vitest";
import type { PhoneNotification, ShareState } from "../types/protocol";
import { callerLine, NOT_SHARING, previewsHidden, shareBanner, stillRevealed, toastKind } from "./sharePrivacy";
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
