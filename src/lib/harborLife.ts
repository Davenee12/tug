// The harbor shows your phone as well as the weather: new texts float in as bottles near the tug,
// a missed call sets the lighthouse blinking, and music playing on the iPhone drifts up from the
// tug as notes. A glance at the weather card says what's waiting without reading a notification.
// Counts only, never names or words, so it's safe to show while the screen is shared.
//
// Pure: the component passes tug's state and the time; weatherHarborDraw draws it.

import type { NowPlaying, PhoneNotification } from "../types/protocol";

/** The most bottles in the water; more texts still say how many in the label. */
export const MAX_BOTTLES = 3;
/** A missed call blinks the lighthouse for this long, while it's still on the phone. */
export const MISSED_CALL_MS = 6 * 60 * 60 * 1000;

/** Where the bottles float, as fractions of the harbor's width (beside the tug, toward the town). */
export const BOTTLE_AT = [0.43, 0.49, 0.545];

export interface HarborLife {
  /** New texts (the Messages badge's count): 0 to MAX_BOTTLES bottles float in. */
  texts: number;
  bottles: number;
  /** A missed call from the last few hours is still on the phone. */
  missedCall: boolean;
  /** Music playing on the iPhone. */
  music: boolean;
}

export const QUIET: HarborLife = { texts: 0, bottles: 0, missedCall: false, music: false };

export function harborLife(opts: {
  unreadTexts: number;
  notifications: PhoneNotification[];
  nowPlaying: NowPlaying;
  connected: boolean;
  now: number;
}): HarborLife {
  const { unreadTexts, notifications, nowPlaying, connected, now } = opts;
  const texts = Math.max(0, Math.floor(unreadTexts));
  return {
    texts,
    bottles: Math.min(MAX_BOTTLES, texts),
    missedCall: notifications.some(
      (n) => n.category === "missedCall" && n.removedAt == null && now - n.receivedAt >= 0 && now - n.receivedAt < MISSED_CALL_MS,
    ),
    music: connected && nowPlaying.state === "playing",
  };
}

/** What the harbor shows of the phone, for its label: "3 new texts. A missed call. Music playing." */
export function lifeLabel(l: HarborLife): string | null {
  const parts = [
    l.texts === 1 ? "1 new text" : l.texts > 1 ? `${l.texts} new texts` : null,
    l.missedCall ? "A missed call" : null,
    l.music ? "Music playing" : null,
  ].filter(Boolean);
  return parts.length ? `${parts.join(". ")}.` : null;
}
