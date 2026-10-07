// The iPhone link's status in words, as pure functions so the wording is unit-tested. One source
// for every surface that describes the link — the sidebar's label and dot, the Connect panel in
// Settings › iPhone, the inline Connection panel and the health rows' "can't tell yet" line — so
// none of them can say "Connecting…" while another says "Waiting for iPhone".

import type { DeviceStatus } from "../types/protocol";

type StatusInputs = Pick<DeviceStatus, "connection" | "awaitingUnlock" | "reconnecting" | "radio" | "away">;

/** Bluetooth is off or missing: nothing can connect, whatever the link last said. */
export function radioDown(s: Pick<DeviceStatus, "radio">): boolean {
  return s.radio === "off" || s.radio === "unavailable";
}

/**
 * Whether to say tug is reconnecting on its own. Never with Bluetooth off or missing: the sidebar
 * says so instead, and there's nothing to reconnect over.
 */
export function showsReconnecting(s: Pick<DeviceStatus, "reconnecting" | "radio">): boolean {
  return s.reconnecting && !radioDown(s);
}

/** What the sidebar says about the iPhone link. */
export function connectionLabel(s: StatusInputs): string {
  if (s.connection === "noDevice") return "Not set up";
  if (s.connection === "connected") return "Connected";
  // Nothing connects with Bluetooth off, so never "Connecting…" then (a relink that was under way
  // when the radio went off would otherwise pulse until it came back).
  if (radioDown(s)) return s.radio === "off" ? "Bluetooth is off" : "No Bluetooth";
  const reconnecting = showsReconnecting(s);
  if (s.connection === "connecting") return reconnecting ? "Reconnecting…" : "Connecting…";
  // A locked phone needs the user; tug rebuilding the link on its own doesn't.
  if (s.awaitingUnlock) return "Unlock your iPhone";
  // Out of range: one steady state while tug keeps trying quietly (failed retries don't flip it).
  if (s.away) return "iPhone away";
  return reconnecting ? "Reconnecting…" : "Waiting for iPhone";
}

/** Whether tug is actively (re)connecting, so the status dot pulses rather than sitting idle. */
export function connectionBusy(s: StatusInputs): boolean {
  if (radioDown(s)) return false;
  if (s.connection === "connecting") return true;
  return s.connection === "disconnected" && showsReconnecting(s) && !s.awaitingUnlock;
}

/**
 * The link in one full sentence, for the panels (Settings › iPhone, the inline Connection panel)
 * and the health rows that can't be judged until the phone is connected. Same states, same order
 * as `connectionLabel`.
 */
export function connectionSentence(s: StatusInputs & Pick<DeviceStatus, "device">): string {
  if (!s.device || s.connection === "noDevice") return "Pair your iPhone to set this up.";
  if (s.connection === "connected") return "Connected. tug reconnects by itself when you come back in range.";
  if (radioDown(s)) {
    return s.radio === "off" ? "Bluetooth is off in Windows. Turn it on to reach your iPhone." : "This PC has no usable Bluetooth adapter.";
  }
  const reconnecting = "Reconnecting to your iPhone. No need to do anything.";
  if (s.connection === "connecting") return showsReconnecting(s) ? reconnecting : "Connecting to your iPhone…";
  // A stale awaitingUnlock flag must never claim a forgotten phone is "connected" (handled above).
  if (s.awaitingUnlock) return "Your iPhone is connected but locked. Unlock it to reconnect.";
  // Away before reconnecting, as in the label: one steady "out of range" while tug retries quietly.
  if (s.away) return "Your iPhone is out of range. tug reconnects by itself when it's back.";
  if (showsReconnecting(s)) return reconnecting;
  return "Waiting for your iPhone to reconnect. Keep Bluetooth on and the phone nearby.";
}
