// The inline Connection panel beside the feed (wide windows only) is for an iPhone that
// needs the user: nothing paired yet, or the phone dropped the pairing. A short reconnect is
// not that. Windows tears down tug's Bluetooth objects every few minutes on real hardware and
// tug relinks in 1-17 s; showing the panel for each one slid a Settings-like pane in and out
// of the window ("settings randomly opens and closes"). The rail's "Connecting…" already says
// what's happening, so the panel waits out a grace period before it appears.

import type { ConnectionState } from "../types/protocol";

/** How long the phone has to be away before the panel slides in. Longest relink seen: 17 s. */
export const RECONNECT_GRACE_MS = 30_000;

export interface PanelInputs {
  /** The window is wide enough to show the panel beside the feed. */
  wide: boolean;
  /** Settings is open (it has its own iPhone page). */
  inSettings: boolean;
  /** The real status has arrived from the backend (before that, the placeholder says "noDevice"). */
  statusKnown: boolean;
  connection: ConnectionState;
  hasDevice: boolean;
  /** The phone refused the bond twice in a row: it needs pairing again. */
  pairingStale: boolean;
  /** When the phone was last seen going away (null while connected). */
  downSince: number | null;
  now: number;
}

/** Whether the inline Connection panel shows. Pure, so it's tested. */
export function showConnectionPanel(p: PanelInputs, graceMs = RECONNECT_GRACE_MS): boolean {
  if (!p.wide || p.inSettings || !p.statusKnown || p.connection === "connected") return false;
  // Nothing paired, or the pairing is dead: the user has to act, so show it straight away.
  if (!p.hasDevice || p.pairingStale) return true;
  // Paired and (re)connecting: only once it's clearly not a quick relink.
  return p.downSince != null && p.now - p.downSince >= graceMs;
}

/** Track when the phone went away: kept across a reconnect's connecting/disconnected flips, cleared once it's back. */
export function nextDownSince(prev: number | null, connected: boolean, now: number): number | null {
  if (connected) return null;
  return prev ?? now;
}
