// The three switches on the iPhone's Settings › Bluetooth › ⓘ screen for this PC, each with a
// live state derived from real signals: Share System Notifications (ANCS subscribed /
// authorization probe), Show Notifications (MAP connect OK vs a 0xC3 refusal), Sync Contacts
// (PBAP returns contacts, vs an empty phonebook the backend has seen repeat: `contactsOff`).
//
// This is the one source of truth for those three facts: setup, Settings › iPhone, the health
// rows (lib/health), the Feed nudge and the calls/texts copy all read it, so no two surfaces can
// disagree. Pure, so it's unit-tested without a phone.
//
// tug never guesses a switch is off before the phone has answered, and never shows "Checking…"
// indefinitely either: "checking" only lasts CHECKING_MAX_MS from the moment an answer became
// possible; after that, or when no answer is possible right now, the switch is "waiting" with a
// plain reason ("Shows once your iPhone is connected").

import type { DeviceStatus } from "../types/protocol";
import { radioDown } from "./connectionStatus";

/**
 * "on"/"off" once the phone has answered; "checking" briefly while an answer is on its way (the
 * toggle pulses); "waiting" when it can't be read right now, with the reason in `note`.
 */
export type SwitchState = "on" | "off" | "checking" | "waiting";

/** The longest "Checking…" lasts before it says plainly that there's no answer yet. */
export const CHECKING_MAX_MS = 60_000;

/**
 * When answers became possible, so "checking" can be bounded. Kept by the store (`tug.switches`).
 * Without it (pure callers that only care about on/off), "checking" isn't time-bounded.
 */
export interface SwitchContext {
  now: number;
  /** When the iPhone's notifications link last came up (connection "connected"); null if it isn't. */
  connectedSince: number | null;
  /** When the texts session last opened (`services.messages`); null if it isn't open. */
  messagesSince: number | null;
  /** Contacts tug has saved (from earlier syncs), to explain names that show while Sync Contacts is off. */
  savedContacts: number;
}

export interface PhoneSwitch {
  key: "notifications" | "messages" | "contacts";
  /** The exact label on the iPhone, so the owner can find it. */
  label: string;
  /** One short line: what turning it on gets you. */
  why: string;
  /** Notifications are the point of tug; texts and contacts are optional. */
  required: boolean;
  state: SwitchState;
  /** Where the switch lives on the iPhone. */
  where: string;
  /** What to do when it's off. */
  fix: string;
  /** The line under the switch: the fix when off, "Checking…", or why it can't be read yet. Empty when on. */
  note: string;
  /** Offer "Check again" (Sync Contacts is read by asking the phone, so a re-ask helps). */
  recheck: boolean;
}

const WHERE = "Settings › Bluetooth › ⓘ next to this PC";
const CHECKING = "Checking…";
const NO_ANSWER = "No answer from your iPhone yet. tug keeps checking.";

/** Whether "checking" that started at `since` is still within its bound. */
function stillChecking(since: number | null | undefined, ctx: SwitchContext | undefined): boolean {
  if (!ctx) return true;
  return since != null && ctx.now - since < CHECKING_MAX_MS;
}

/** Why nothing on the phone can be read right now (the notifications link is down). */
function whyNotConnected(s: DeviceStatus): string {
  if (!s.device) return "Shows once your iPhone is paired.";
  if (radioDown(s)) return "Turn on Bluetooth to check.";
  if (s.awaitingUnlock) return "Unlock your iPhone to check.";
  return "Shows once your iPhone is connected.";
}

function notificationsSwitch(s: DeviceStatus): Pick<PhoneSwitch, "state" | "note"> {
  const fix = "Turn it on so your iPhone's notifications reach tug.";
  // The authorization probe tells us whether iOS is actually sharing; it answers on connect.
  if (s.connection !== "connected") return { state: "waiting", note: whyNotConnected(s) };
  return s.services.notifications ? { state: "on", note: "" } : { state: "off", note: fix };
}

function messagesSwitch(s: DeviceStatus, ctx?: SwitchContext): Pick<PhoneSwitch, "state" | "note"> {
  // The phone refused message access (OBEX CONNECT 0xC3 Forbidden): the switch is off.
  if (s.messagesError) return { state: "off", note: "Turn it on so tug can read and send your texts." };
  if (s.services.messages) return { state: "on", note: "" };
  // Without a working texts pairing there's no connection to judge the switch from.
  if (s.textsPairing === "missing") return { state: "waiting", note: "Set up texts in Settings › iPhone first." };
  if (s.textsPairing === "broken") return { state: "waiting", note: "Texts stopped connecting. See Settings › iPhone." };
  if (s.connection !== "connected") return { state: "waiting", note: whyNotConnected(s) };
  return stillChecking(ctx?.connectedSince, ctx) ? { state: "checking", note: CHECKING } : { state: "waiting", note: NO_ANSWER };
}

/** Sync Contacts' state on its own: what the health row, the calls copy and the switch all use. */
export function contactsSwitchState(s: DeviceStatus, ctx?: SwitchContext): SwitchState {
  return contactsSwitch(s, ctx).state;
}

function contactsSwitch(s: DeviceStatus, ctx?: SwitchContext): Pick<PhoneSwitch, "state" | "note" | "recheck"> {
  // A refusal, or empty phonebooks repeating, is a clear "off". Names saved from before (a
  // re-pair turns the switch off) keep showing, so say where they came from.
  if (s.contactsError || s.contactsOff) {
    const saved = (ctx?.savedContacts ?? 0) > 0;
    return {
      state: "off",
      note: saved
        ? "Off on your iPhone. The names you see were saved earlier; turn it on to keep them up to date."
        : "Turn it on so tug shows names instead of numbers.",
      recheck: true,
    };
  }
  // The phone sharing contacts on this connection (not contacts saved from before).
  if (s.contactsShared) return { state: "on", note: "", recheck: false };
  // tug asks for contacts over the texts connection, so it can only tell once that's open.
  if (!s.services.messages) {
    const note =
      s.textsPairing === "missing" || s.textsPairing === "broken"
        ? "tug checks this once texts are set up."
        : s.messagesError
          ? "tug checks this once Show Notifications is on."
          : "Shows once texts are connected.";
    return { state: "waiting", note, recheck: false };
  }
  return stillChecking(ctx?.messagesSince, ctx)
    ? { state: "checking", note: CHECKING, recheck: false }
    : { state: "waiting", note: NO_ANSWER, recheck: true };
}

/** The three switches with their live state. */
export function phoneSwitches(s: DeviceStatus, ctx?: SwitchContext): PhoneSwitch[] {
  return [
    {
      key: "notifications",
      label: "Share System Notifications",
      why: "Your notifications on this PC",
      required: true,
      where: WHERE,
      fix: "Turn it on so your iPhone's notifications reach tug.",
      recheck: false,
      ...notificationsSwitch(s),
    },
    {
      key: "messages",
      label: "Show Notifications",
      why: "Read and reply to texts",
      required: false,
      where: WHERE,
      fix: "Turn it on so tug can read and send your texts.",
      recheck: false,
      ...messagesSwitch(s, ctx),
    },
    {
      key: "contacts",
      label: "Sync Contacts",
      why: "Names instead of numbers",
      required: false,
      where: WHERE,
      fix: "Turn it on so tug shows names instead of numbers.",
      ...contactsSwitch(s, ctx),
    },
  ];
}

/** The switches that are definitely off right now (for a one-line "turn on X" summary). */
export function switchesOff(switches: PhoneSwitch[]): PhoneSwitch[] {
  return switches.filter((x) => x.state === "off");
}

/** Any switch not yet on: the backend checks fast while the owner may be flipping them. */
export function switchesPending(switches: PhoneSwitch[]): boolean {
  return switches.some((x) => x.state !== "on");
}
