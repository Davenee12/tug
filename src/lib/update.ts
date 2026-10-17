// Automatic updates in the window: the Update button, its "Restart tug to update?" confirm, and
// the Settings › About row. The backend (src-tauri/src/updater) checks, downloads, verifies and
// applies; this decides what the person sees, in plain words. Pure, so it's tested.

import type { SettingsSection } from "../stores/tug";
import type { UpdateBlocker, UpdateStatus } from "../types/protocol";

/** Settings › About when the build has no update key (a dev build, or before the owner set it up). */
export const NOT_SET_UP = "Automatic updates aren't set up for this build";

/** The compact button beside tug's name: only once there's something to do. */
export interface UpdatePill {
  label: string;
  /** The tooltip. */
  title: string;
  /** Installed copy: open the confirm. Portable exe: open the download page. */
  action: "confirm" | "download";
}

export function updatePill(s: UpdateStatus | null): UpdatePill | null {
  if (!s?.version) return null;
  if (!s.portable && (s.phase === "ready" || s.phase === "applying")) {
    return { label: "Update", title: `tug ${s.version} is ready — click to restart and update`, action: "confirm" };
  }
  if (s.portable && s.phase === "available") {
    return {
      label: "Update available — Download",
      title: `tug ${s.version} is out. This copy can't update itself: click to download it from GitHub`,
      action: "download",
    };
  }
  return null;
}

/** What has to finish before tug restarts to update; the button stays meanwhile. */
export function blockerText(b: UpdateBlocker): string {
  switch (b) {
    case "call":
      return "Finish your call first";
    case "transfer":
      return "Let the Tugboat transfer finish first";
    case "voice":
      return "Finish voice typing first";
  }
}

/** The confirm behind the Update button. */
export type Confirm =
  | { open: false }
  | {
      open: true;
      version: string;
      headline: string | null;
      /** Restart now was pressed and the answer hasn't come (or tug is restarting). */
      applying: boolean;
      /** Restart now was held back: what has to finish first. */
      held: UpdateBlocker | null;
      /** Restart now failed: what to say. */
      error: string | null;
    };

export const CLOSED: Confirm = { open: false };

/** The Update button was pressed: open the confirm for a ready update (nothing else opens it). */
export function openConfirm(s: UpdateStatus | null): Confirm {
  if (!s?.version || s.portable || s.phase !== "ready") return CLOSED;
  return { open: true, version: s.version, headline: s.headline, applying: false, held: null, error: null };
}

/** Restart now: wait for the backend (a second press while waiting does nothing). */
export function pressRestart(c: Confirm): Confirm {
  if (!c.open || c.applying) return c;
  return { ...c, applying: true, held: null, error: null };
}

/** The backend held the restart back (a call, a transfer, voice typing): say so, keep the confirm. */
export function heldBack(c: Confirm, blocked: UpdateBlocker): Confirm {
  if (!c.open) return c;
  return { ...c, applying: false, held: blocked };
}

/** The restart failed outright: say so; the update is retried later. */
export function restartFailed(c: Confirm, error: string): Confirm {
  if (!c.open) return c;
  return { ...c, applying: false, error };
}

/** A new status arrived: the confirm only makes sense while an update is ready (or applying). */
export function followStatus(c: Confirm, s: UpdateStatus | null): Confirm {
  if (!c.open) return c;
  if (!s || (s.phase !== "ready" && s.phase !== "applying")) return CLOSED;
  return c;
}

/** Settings › About: one line about updates and, when there's something to press, a button. */
export interface UpdateAbout {
  line: string;
  /** Working on it: the button (if any) is disabled. */
  busy: boolean;
  action: { label: string; run: "check" | "download" | "restart" | "open" } | null;
  /** Show the "Download updates automatically" switch. */
  showAuto: boolean;
}

export function updateAbout(s: UpdateStatus | null): UpdateAbout {
  if (!s || s.phase === "notConfigured") return { line: NOT_SET_UP, busy: false, action: null, showAuto: false };
  const check = { label: "Check for updates", run: "check" as const };
  const v = s.version ? `tug ${s.version}` : "A new version";
  const base = { busy: false, showAuto: true };
  switch (s.phase) {
    case "idle":
      return { ...base, line: s.portable ? "This portable copy can't update itself; tug can tell you when a new version is out." : "tug updates itself quietly in the background.", action: check };
    case "checking":
      return { ...base, line: "Checking…", busy: true, action: check };
    case "upToDate":
      return { ...base, line: "You're up to date", action: check };
    case "available":
      return s.portable
        ? { ...base, line: `${v} is available`, action: { label: "Download", run: "open" } }
        : { ...base, line: `${v} is available`, action: { label: "Download", run: "download" } };
    case "downloading":
      return { ...base, line: s.progress == null ? "Downloading…" : `Downloading… ${s.progress}%`, busy: true, action: null };
    case "ready":
      return { ...base, line: "Ready — Restart to update", action: { label: "Restart to update", run: "restart" } };
    case "applying":
      return { ...base, line: "Restarting to update…", busy: true, action: null };
    case "failed":
      return {
        ...base,
        line: s.failure === "check" ? "Couldn't check for updates" : "Couldn't update",
        action: { label: "Try again", run: "check" },
      };
  }
}

/** The "Download updates automatically" switch's description (the portable exe only checks). */
export function autoDescription(portable: boolean): string {
  return portable
    ? "Look for new versions on GitHub every few hours. When it's off, tug only looks when you press Check for updates."
    : "Download new versions quietly in the background; tug only restarts when you click Update. When it's off, tug only looks when you press Check for updates.";
}

// ---- Coming back to the same screen after the restart ----

/** A screen tug can reopen. */
export type Screen = { view: "feed" | "messages" | "calls" } | { view: "settings"; section: SettingsSection };

const VIEWS = ["feed", "messages", "calls"] as const;
const SECTIONS: readonly SettingsSection[] = ["general", "iphone", "notifications", "connectors", "weather", "developer", "privacy", "about"];

/** The screen as the backend keeps it across the restart ("messages", "settings:about"). */
export function resumeFor(view: "feed" | "messages" | "calls" | "settings", section: SettingsSection): string {
  return view === "settings" ? `settings:${section}` : view;
}

/** Read it back, accepting only screens tug has. */
export function parseResume(saved: string | null | undefined): Screen | null {
  if (!saved) return null;
  const [view, section, extra] = saved.split(":");
  if (extra !== undefined) return null;
  if (view === "settings") {
    const s = SECTIONS.find((x) => x === section);
    return s ? { view: "settings", section: s } : null;
  }
  const v = VIEWS.find((x) => x === view);
  return v && section === undefined ? { view: v } : null;
}
