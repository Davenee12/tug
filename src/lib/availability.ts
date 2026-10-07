// What the Messages reply box and the Calls page say when texts or recent calls aren't available.
// Derived from the switches (lib/phoneSwitches), so they agree with the switches card and the
// health rows, and can't say "Connecting…" forever while Settings says texts aren't set up.

import type { DeviceStatus } from "../types/protocol";
import { phoneSwitches, type SwitchContext } from "./phoneSwitches";

/** Why a reply can't be sent from tug right now, or null when it can. */
export function replyUnavailable(s: DeviceStatus, ctx?: SwitchContext): string | null {
  if (s.services.messages) return null;
  // The phone's own reason, already plain English (e.g. "turn on Show Notifications").
  if (s.messagesError) return s.messagesError;
  if (!s.device) return "Set up your iPhone in Settings › iPhone to reply from here.";
  if (s.textsPairing === "missing") return "Replying from tug needs texts set up on this PC. See Settings › iPhone.";
  if (s.textsPairing === "broken") return "Texts stopped connecting. Settings › iPhone says how to fix it.";
  const texts = phoneSwitches(s, ctx).find((x) => x.key === "messages")!;
  if (texts.state === "checking") return "Connecting to your iPhone's messages…";
  if (s.connection !== "connected") return "You can reply once your iPhone reconnects.";
  return "Your iPhone hasn't opened message access yet. tug keeps trying.";
}

/** What the empty Calls page says. */
export function callsEmptyHint(s: DeviceStatus, ctx?: SwitchContext): string {
  if (s.contactsError) return s.contactsError;
  if (!s.device) return "Recent calls appear once your iPhone is set up.";
  const contacts = phoneSwitches(s, ctx).find((x) => x.key === "contacts")!;
  switch (contacts.state) {
    case "on":
      return "Your iPhone has no recent calls to share.";
    case "off":
      return "Recent calls come with Sync Contacts. Turn it on in your iPhone's Settings › Bluetooth › ⓘ next to this PC.";
    case "checking":
      return "Checking Sync Contacts on your iPhone…";
    default:
      return s.services.messages
        ? "Recent calls appear once your iPhone shares them. tug keeps checking."
        : "Recent calls come over the same link as your texts. They'll appear once it's connected.";
  }
}
