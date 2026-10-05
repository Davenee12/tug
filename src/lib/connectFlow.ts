// The one-screen "Connect your iPhone" panel's pure decisions: which phase it's in, when it gives
// way to the Feed, and which optional switches to nudge about once it has. Kept pure and
// unit-tested so the panel (ConnectPanel.vue) stays a thin render of real DeviceStatus signals,
// shared by the Feed's empty state and Settings › iPhone. It reuses the proven pairing/switch
// logic (lib/pairings, lib/phoneSwitches) rather than re-deriving any of it.

import type { DeviceStatus, PairingRequest } from "../types/protocol";
import { phoneSwitches, switchesOff, type PhoneSwitch } from "./phoneSwitches";

/**
 * The panel's phase, in the order the user moves through it:
 * - "find": nothing paired yet — show the iPhones found nearby, each with Pair.
 * - "pairing": a pairing request is in flight — the code shows in the pairing dialog.
 * - "allow": paired, and iOS is holding the connection open until Allow is tapped on the phone.
 * - "switches": paired and connecting, with the three switches ticking green as they come on.
 * - "done": notifications are working — the panel gives way to the Feed.
 */
export type ConnectStep = "find" | "pairing" | "allow" | "switches" | "done";

/** Which phase the panel is in, from the live status and any pairing request. */
export function connectStep(status: DeviceStatus, pairing: PairingRequest | null): ConnectStep {
  if (connectDone(status)) return "done";
  if (status.device) return status.awaitingPhoneAllow ? "allow" : "switches";
  if (pairing) return "pairing";
  return "find";
}

/** Notifications are working (the required switch is on): the panel yields to the Feed. */
export function connectDone(status: DeviceStatus): boolean {
  return status.connection === "connected" && status.services.notifications;
}

/**
 * The next state of the Feed-area panel latch. The panel stands in for the Feed while the user
 * sets up their iPhone and yields once notifications work. The latch only engages when there's no
 * device after the real status is known, so an upgrader whose paired phone is merely reconnecting
 * at launch goes straight to the Feed and never sees the panel; once it's up it stays through
 * pairing and the switches until notifications are working (not merely until a device appears).
 */
export function nextShowConnect(prev: boolean, statusKnown: boolean, status: DeviceStatus): boolean {
  if (!statusKnown) return prev;
  if (connectDone(status)) return false;
  if (status.device == null) return true;
  return prev;
}

/**
 * Once notifications work, the optional switches (texts, contacts) that are still definitely off:
 * a compact, dismissible nudge at the top of the Feed, never a block. Empty until notifications
 * are on, and it never includes the required notifications switch (that one keeps the panel up).
 */
export function optionalNudge(status: DeviceStatus): PhoneSwitch[] {
  if (!connectDone(status)) return [];
  return switchesOff(phoneSwitches(status)).filter((x) => !x.required);
}
