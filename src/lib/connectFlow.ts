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
 * - "done": all three switches are on (or the user chose "Skip for now") — the panel gives way to
 *   the Feed. Dave: it mustn't rush off while he's still turning switches on.
 */
export type ConnectStep = "find" | "pairing" | "allow" | "switches" | "done";

/** Which phase the panel is in, from the live status and any pairing request. */
export function connectStep(status: DeviceStatus, pairing: PairingRequest | null, skipped = false): ConnectStep {
  if (connectDone(status, skipped)) return "done";
  if (status.device) return status.awaitingPhoneAllow ? "allow" : "switches";
  if (pairing) return "pairing";
  return "find";
}

/** Notifications are working (the required switch is on). */
export function notificationsWorking(status: DeviceStatus): boolean {
  return status.connection === "connected" && status.services.notifications;
}

/** Every one of the three switches reads "on" from a real signal. */
export function allSwitchesOn(status: DeviceStatus): boolean {
  return phoneSwitches(status).every((x) => x.state === "on");
}

/**
 * The panel yields to the Feed once notifications work and all three switches are on, or once
 * the user skipped the optional ones ("Skip for now") with notifications working.
 */
export function connectDone(status: DeviceStatus, skipped = false): boolean {
  return notificationsWorking(status) && (skipped || allSwitchesOn(status));
}

/** Offer "Skip for now" once notifications work but an optional switch isn't on yet. */
export function canSkipSwitches(status: DeviceStatus): boolean {
  return notificationsWorking(status) && !allSwitchesOn(status);
}

/**
 * The next state of the Feed-area panel latch. The panel stands in for the Feed while the user
 * sets up their iPhone and yields once notifications work. The latch only engages when there's no
 * device after the real status is known, so an upgrader whose paired phone is merely reconnecting
 * at launch goes straight to the Feed and never sees the panel; once it's up it stays through
 * pairing and the switches until notifications are working (not merely until a device appears).
 */
export function nextShowConnect(prev: boolean, statusKnown: boolean, status: DeviceStatus, skipped = false): boolean {
  if (!statusKnown) return prev;
  // Only an open panel waits for the switches; a returning user's panel never engages.
  if (prev ? connectDone(status, skipped) : notificationsWorking(status)) return false;
  if (status.device == null) return true;
  return prev;
}

/**
 * Once notifications work, the optional switches (texts, contacts) that are still definitely off:
 * a compact, dismissible nudge at the top of the Feed, never a block. Empty until notifications
 * are on, and it never includes the required notifications switch (that one keeps the panel up).
 */
export function optionalNudge(status: DeviceStatus): PhoneSwitch[] {
  if (!notificationsWorking(status)) return [];
  return switchesOff(phoneSwitches(status)).filter((x) => !x.required);
}

/**
 * Whether the backend should run fast checks ("watching"): the iPhone's switches show up green
 * within ~2 s of being flipped, and contacts are retried every ~10 s. On whenever the Connect
 * panel is on screen (the Feed stand-in or Settings › iPhone) with a switch still off, and for a
 * short window after launch/pairing so a switch flipped with tug in the tray still turns green
 * fast. Off once every switch is on, so an idle, fully set-up phone isn't polled every 2 s.
 */
export function shouldWatchSwitches(o: { panelVisible: boolean; fresh: boolean; switchesPending: boolean }): boolean {
  return o.switchesPending && (o.panelVisible || o.fresh);
}

/** How often to re-inquire for the iPhone while the find step is up (see `rescanDue`). */
export const RESCAN_INTERVAL_MS = 15_000;

/**
 * Whether to restart the iPhone scan now. An AEP watcher for an unpaired Classic device inquires
 * once when it starts and never again, so an iPhone made discoverable *after* tug began scanning
 * never appears. While the find step is on screen, re-inquire every ~15 s so a phone that becomes
 * discoverable later shows up within that window. Pure (time passed in), so the timing is tested.
 */
export function rescanDue(findStepVisible: boolean, sinceLastRescanMs: number): boolean {
  return findStepVisible && sinceLastRescanMs >= RESCAN_INTERVAL_MS;
}
