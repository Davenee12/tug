import { describe, expect, it } from "vitest";
import type { NowPlaying, PhoneNotification } from "../types/protocol";
import { harborLife, lifeLabel, MISSED_CALL_MS, QUIET } from "./harborLife";

const NOW = Date.UTC(2026, 9, 27, 16, 0);

const playing = (state: NowPlaying["state"]): NowPlaying =>
  ({ player: "Music", state, rate: 1, elapsed: 0, elapsedAt: NOW, volume: null, title: "Teardrop", artist: "Massive Attack", album: null, duration: 330 }) as NowPlaying;

function n(over: Partial<PhoneNotification>): PhoneNotification {
  return {
    id: 1,
    appId: "com.apple.mobilephone",
    appName: "Phone",
    category: "missedCall",
    title: "Mum",
    subtitle: "",
    message: "Missed Call",
    postedAt: null,
    receivedAt: NOW - 60_000,
    flags: { silent: false, important: false, preExisting: false, positiveAction: true, negativeAction: false },
    positiveLabel: "Call Back",
    negativeLabel: "",
    removedAt: null,
    live: true,
    fresh: true,
    ...over,
  };
}

const base = { unreadTexts: 0, notifications: [], nowPlaying: playing("paused"), connected: true, now: NOW };

describe("the harbor shows your phone", () => {
  it("is quiet when nothing's waiting", () => {
    expect(harborLife(base)).toEqual(QUIET);
    expect(lifeLabel(QUIET)).toBeNull();
  });

  it("floats a bottle for each new text, up to three", () => {
    expect(harborLife({ ...base, unreadTexts: 1 })).toMatchObject({ texts: 1, bottles: 1 });
    expect(harborLife({ ...base, unreadTexts: 9 })).toMatchObject({ texts: 9, bottles: 3 });
  });

  it("blinks the lighthouse for a recent missed call still on the phone", () => {
    expect(harborLife({ ...base, notifications: [n({})] }).missedCall).toBe(true);
    // Cleared on the phone (or with ✕ in tug), or hours old: the lighthouse rests.
    expect(harborLife({ ...base, notifications: [n({ removedAt: NOW })] }).missedCall).toBe(false);
    expect(harborLife({ ...base, notifications: [n({ receivedAt: NOW - MISSED_CALL_MS })] }).missedCall).toBe(false);
    // Only missed calls: a text or a ringing call isn't one.
    expect(harborLife({ ...base, notifications: [n({ category: "incomingCall" }), n({ category: "social" as never })] }).missedCall).toBe(false);
  });

  it("plays music only while the phone is connected and playing", () => {
    expect(harborLife({ ...base, nowPlaying: playing("playing") }).music).toBe(true);
    expect(harborLife({ ...base, nowPlaying: playing("playing"), connected: false }).music).toBe(false);
  });

  it("says it in counts only, never names or words", () => {
    const l = harborLife({ ...base, unreadTexts: 3, notifications: [n({})], nowPlaying: playing("playing") });
    expect(lifeLabel(l)).toBe("3 new texts. A missed call. Music playing.");
    expect(lifeLabel({ ...QUIET, texts: 1, bottles: 1 })).toBe("1 new text.");
  });
});
