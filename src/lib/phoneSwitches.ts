// The three switches on the iPhone's Settings › Bluetooth › ⓘ screen for this PC, each with a
// live state derived from real signals: Share System Notifications (ANCS subscribed /
// authorization probe), Show Notifications (MAP connect OK vs a 0xC3 refusal), Sync Contacts
// (PBAP returns contacts vs empty). Pure so it can be unit-tested without a phone, and shared by
// first-run setup and Settings › iPhone so both say the same thing. "unknown" until a signal
// arrives — tug never guesses a switch is off before the phone has answered.

import type { DeviceStatus } from "../types/protocol";

/** "on"/"off" once the phone has answered; "unknown" while tug has no signal either way. */
export type SwitchState = "on" | "off" | "unknown";

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
  /** What to do when it's off (or can't be read yet). */
  fix: string;
}

const WHERE = "Settings › Bluetooth › ⓘ next to this PC";

/**
 * The three switches with their live state. `contacts` is the number of synced contacts (the
 * status snapshot doesn't carry it). Notifications can only be judged once the phone is
 * connected; texts and contacts rely on the message/contacts signals the worker records.
 */
export function phoneSwitches(s: DeviceStatus, contacts: number): PhoneSwitch[] {
  const connected = s.connection === "connected";
  return [
    {
      key: "notifications",
      label: "Share System Notifications",
      why: "Your notifications on this PC",
      required: true,
      // The authorization probe tells us whether iOS is actually sharing; before we're
      // connected there's no answer yet.
      state: !connected ? "unknown" : s.services.notifications ? "on" : "off",
      where: WHERE,
      fix: "Turn it on so your iPhone's notifications reach tug.",
    },
    {
      key: "messages",
      label: "Show Notifications",
      why: "Read and reply to texts",
      required: false,
      state: messagesState(s),
      where: WHERE,
      fix: "Turn it on so tug can read and send your texts.",
    },
    {
      key: "contacts",
      label: "Sync Contacts",
      why: "Names instead of numbers",
      required: false,
      // A refusal is a clear "off"; a successful pull with people in it is "on"; an empty pull
      // reads as unknown rather than off, since it also happens for a moment before the switch
      // is flipped.
      state: s.contactsError ? "off" : contacts > 0 ? "on" : "unknown",
      where: WHERE,
      fix: "Turn it on so tug shows names instead of numbers.",
    },
  ];
}

/** Show Notifications: a 0xC3 refusal (messagesError) is a definite off; an open session is on. */
function messagesState(s: DeviceStatus): SwitchState {
  // The phone refused message access (OBEX CONNECT 0xC3 Forbidden): the switch is off.
  if (s.messagesError) return "off";
  if (s.services.messages) return "on";
  // Without a texts pairing there's no connection to judge the switch from.
  return "unknown";
}

/** The switches that are definitely off right now (for a one-line "turn on X" summary). */
export function switchesOff(switches: PhoneSwitch[]): PhoneSwitch[] {
  return switches.filter((x) => x.state === "off");
}
