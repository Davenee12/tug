// The sidebar's one-line connection status, as pure functions so the wording is unit-tested.

import type { DeviceStatus } from "../types/protocol";

type StatusInputs = Pick<DeviceStatus, "connection" | "awaitingUnlock" | "reconnecting" | "radio">;

/**
 * Whether to say tug is reconnecting on its own. Never with Bluetooth off or missing: the sidebar
 * says so instead, and there's nothing to reconnect over.
 */
export function showsReconnecting(s: Pick<DeviceStatus, "reconnecting" | "radio">): boolean {
  return s.reconnecting && s.radio !== "off" && s.radio !== "unavailable";
}

/** What the sidebar says about the iPhone link. */
export function connectionLabel(s: StatusInputs): string {
  const reconnecting = showsReconnecting(s);
  switch (s.connection) {
    case "connected":
      return "Connected";
    case "connecting":
      return reconnecting ? "Reconnecting…" : "Connecting…";
    case "disconnected":
      // A locked phone needs the user; tug rebuilding the link on its own doesn't.
      if (s.awaitingUnlock) return "Unlock your iPhone";
      return reconnecting ? "Reconnecting…" : "Waiting for iPhone";
    default:
      return "Not set up";
  }
}

/** Whether tug is actively (re)connecting, so the status dot pulses rather than sitting idle. */
export function connectionBusy(s: StatusInputs): boolean {
  if (s.connection === "connecting") return true;
  return s.connection === "disconnected" && showsReconnecting(s) && !s.awaitingUnlock;
}
