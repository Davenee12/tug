// Focus mode: while the person is focused (a full-screen app, a meeting, a code editor in front;
// the backend's focus_mode/ watches and says when), tug holds its Windows pop-ups (lib/popup), and
// when the focus ends one pop-up sums up what was held: "While you were focused: 4 texts from
// 2 people, 1 missed call". Everything still arrives in tug's window as usual.
//
// Pure (callers pass `now`):
//   • `FocusSpell` keeps which pop-ups one focus held. Only those are summed up: what quiet hours,
//     Do not disturb or a muted app held was never going to pop up, and calls and codes came
//     through anyway.
//   • `focusSummary` counts them the way the Welcome back card does (lib/welcomeBack), from what
//     tug holds when the focus ends: a text that came as both a notification and a message counts
//     once, and anything already seen in tug or cleared on the phone has been dealt with.
// A focus that held nothing gets no summary. A moment of full screen never becomes a focus at all:
// the backend waits a few seconds before holding anything (`focus_mode::ENTER_AFTER`).

import type { Contact, FocusSettings, FocusState, PhoneNotification, SmsMessage } from "../types/protocol";
import { notificationTime } from "./format";
import { countParts, welcomeBack, type WelcomeInput } from "./welcomeBack";

/** Off until the person turns it on; then a full-screen app and a meeting count, coding if chosen. */
export const DEFAULT_FOCUS: FocusSettings = { enabled: false, fullScreen: true, meetings: true, coding: false };

/** The summary pop-up's title. */
export const FOCUS_TITLE = "While you were focused";

/** The stored `ui.focus`, with the defaults for anything missing or unreadable. */
export function focusSettingsFrom(raw: string | undefined): FocusSettings {
  if (!raw) return { ...DEFAULT_FOCUS };
  try {
    const parsed = JSON.parse(raw) as Partial<FocusSettings> | null;
    return { ...DEFAULT_FOCUS, ...(parsed && typeof parsed === "object" ? parsed : {}) };
  } catch {
    return { ...DEFAULT_FOCUS };
  }
}

/** One focus that has ended: from `since` to `until` (unix ms), and the notifications it held. */
export interface HeldSpell {
  since: number;
  until: number;
  held: number[];
}

/** Which pop-ups the focus in progress has held, and since when. */
export class FocusSpell {
  private since: number | null = null;
  /** Notification id → its time, so the summary's window covers it (a reconnect replays older ones). */
  private held = new Map<number, number>();

  get active(): boolean {
    return this.since !== null;
  }

  /** A focus started (again, it's still the same one). */
  start(now: number): void {
    if (this.since === null) this.since = now;
  }

  /** This notification's pop-up was held for the summary. */
  hold(n: PhoneNotification): void {
    if (this.since !== null) this.held.set(n.id, notificationTime(n).getTime());
  }

  /** The focus ended: what it held (possibly nothing), or null if none was in progress. */
  end(now: number): HeldSpell | null {
    if (this.since === null) return null;
    const times = [...this.held.values()];
    // welcomeBack counts what's after `since` and up to `until`.
    const spell = { since: Math.min(this.since, ...times) - 1, until: Math.max(now, ...times), held: [...this.held.keys()] };
    this.since = null;
    this.held.clear();
    return spell;
  }
}

export interface FocusSummaryInput extends Pick<WelcomeInput, "seenAt" | "isKnown" | "clearedCodes"> {
  spell: HeldSpell;
  now: number;
  notifications: PhoneNotification[];
  messages: SmsMessage[];
  contacts: Contact[];
}

export interface FocusSummary {
  title: string;
  /** "4 texts from 2 people, 1 missed call" */
  body: string;
  /** Everything counted. */
  total: number;
}

/**
 * The pop-up for a focus that ended, or null when there's nothing to say: it held nothing, or all
 * it held has been seen in tug or cleared on the phone since.
 */
export function focusSummary(input: FocusSummaryInput): FocusSummary | null {
  if (!input.spell.held.length) return null;
  const held = new Set(input.spell.held);
  const summary = welcomeBack({
    since: input.spell.since,
    until: input.spell.until,
    now: input.now,
    notifications: input.notifications.filter((n) => held.has(n.id)),
    // Only what was held counts; texts you sent still say a conversation was answered (from the
    // phone, say), so what came before the reply isn't news.
    messages: input.messages.filter((m) => m.direction === "out"),
    contacts: input.contacts,
    seenAt: input.seenAt,
    isKnown: input.isKnown,
    clearedCodes: input.clearedCodes,
  });
  if (summary.total === 0) return null;
  return { title: FOCUS_TITLE, body: countParts(summary.counts).join(", "), total: summary.total };
}

/** What Settings says while Focus mode is holding pop-ups, or null when it isn't. */
export function focusStatus(settings: FocusSettings, state: FocusState): string | null {
  if (!settings.enabled || !state.active) return null;
  const why =
    state.reason === "meeting"
      ? "an app is using the microphone or camera"
      : state.reason === "fullScreen"
        ? "something is full screen"
        : state.reason === "coding"
          ? "a code editor or terminal is in front"
          : null;
  return why ? `Holding pop-ups now: ${why}.` : "Holding pop-ups now.";
}
