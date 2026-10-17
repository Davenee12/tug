// Bring in your full message history from an iPhone backup: the words the wizard, Settings and
// the conversation use (the decisions are in Rust's `backup/`; this only says them). Kept here,
// with tests, so components stay thin.

import type { BackupImported, BackupInfo, BackupProblem, BackupScan, BackupStatus, BackupSummary, SmsMessage } from "../types/protocol";
import { phoneModel } from "./phoneModel";

/** Imported history's message source (Rust `SOURCE_IPHONE_BACKUP`). */
export const BACKUP_SOURCE = "iphone-backup";

/** The free Microsoft Store extension that lets Windows (and so tug) open HEIC photos. */
export const HEIF_HINT = "Install HEIF Image Extensions from the Microsoft Store to see iPhone photos here.";
export const HEIF_STORE_URL = "https://apps.microsoft.com/detail/9pmmsr1cgpwg";
/** The other free one: the pictures inside a HEIC file are HEVC, which needs its own extension. */
export const HEVC_HINT =
  "iPhone photos need two free extensions from the Microsoft Store: HEIF Image Extensions, which you have, and HEVC Video Extensions from Device Manufacturer. Install that one to see them here.";
export const HEVC_STORE_URL = "https://apps.microsoft.com/detail/9n4wgh0z6vhq";

export type PhotoHint = { kind: "heif" | "hevc"; text: string; url: string };

/** The one-time hint above a conversation when iPhone photos can't be shown, or null. */
export function photoHint(needs: { heif: boolean; hevc: boolean }): PhotoHint | null {
  if (needs.heif) return { kind: "heif", text: HEIF_HINT, url: HEIF_STORE_URL };
  if (needs.hevc) return { kind: "hevc", text: HEVC_HINT, url: HEVC_STORE_URL };
  return null;
}

/** How to make a backup when there isn't one. Kept short; the Apple Devices app does the rest. */
export const HOW_TO_BACK_UP = [
  "Connect your iPhone to this PC with a cable and open the Apple Devices app (or iTunes).",
  "Choose “Back up all the data on your iPhone to this computer”. Encrypting it is optional; tug can read either.",
  "Select Back Up Now, wait for it to finish, then come back here.",
];

export function isImported(m: Pick<SmsMessage, "source">): boolean {
  return m.source === BACKUP_SOURCE;
}

const count = (n: number) => n.toLocaleString("en-US");
const plural = (n: number, one: string, many = `${one}s`) => `${count(n)} ${n === 1 ? one : many}`;

/** Bytes as people read sizes: "820 KB", "3.2 GB". */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 1000) return `${Math.max(0, Math.round(bytes || 0))} bytes`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = bytes / 1000;
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

/** "Oct 12" this year, "Oct 12, 2024" before it. */
export function shortDate(ms: number, now = new Date()): string {
  const d = new Date(ms);
  const sameYear = d.getFullYear() === now.getFullYear();
  return d.toLocaleDateString("en-US", sameYear ? { month: "short", day: "numeric" } : { month: "short", day: "numeric", year: "numeric" });
}

/** The quiet label above old history in a conversation. */
export function backupLabel(imported: Pick<BackupImported, "backupDate"> | null, now = new Date()): string {
  return imported?.backupDate ? `From your iPhone backup, ${shortDate(imported.backupDate, now)}` : "From your iPhone backup";
}

/** One backup in the picker: its name, then "iPhone 15 Pro · iOS 17.5 · Backed up Oct 12 · 48 GB". */
export function backupDetails(b: BackupInfo, now = new Date()): string {
  const parts: string[] = [];
  if (b.model) parts.push(phoneModel(b.model).name);
  if (b.iosVersion) parts.push(`iOS ${b.iosVersion}`);
  if (b.lastBackup) parts.push(`Backed up ${shortDate(b.lastBackup, now)}`);
  parts.push(formatBytes(b.sizeBytes));
  return parts.join(" · ");
}

/** Why a backup in the list can't be used, or null. */
export function backupUnusable(b: Pick<BackupInfo, "problem">): string | null {
  switch (b.problem) {
    case "tooOld":
      return "Made before iOS 10, so tug can't read it. Make a new backup.";
    case "incomplete":
      return "This backup didn't finish. Back up again in the Apple Devices app.";
    case "unreadable":
      return "tug can't read this backup.";
    default:
      return null;
  }
}

/** What went wrong, and what to do. */
export function problemMessage(p: BackupProblem | null): string {
  switch (p) {
    case "needsPassword":
      return "This backup is encrypted. Enter the password you chose when you turned on encryption.";
    case "wrongPassword":
      return "That password didn't unlock this backup. It's the backup password you set in the Apple Devices app or iTunes, not your iPhone's passcode.";
    case "tooOld":
      return "This backup was made before iOS 10, so tug can't read it. Make a new backup and try again.";
    case "incomplete":
      return "This backup didn't finish. Back up again in the Apple Devices app, then try again.";
    case "unreadable":
      return "tug couldn't read this backup. Make a new one in the Apple Devices app and try again.";
    case "noMessages":
      return "This backup has no messages in it. If Messages in iCloud is on, your iPhone keeps them in iCloud instead of the backup.";
    case "cantWrite":
      return "tug couldn't save to this PC. Check there's enough free space, then try again; what was added so far stays.";
    default:
      return "Something went wrong reading the backup. Try again.";
  }
}

/** What the backup holds that tug doesn't, before importing. */
export function scanLine(s: BackupScan): string {
  const media = s.photos + s.videos;
  if (s.texts === 0 && media === 0) return "Everything in this backup is already in tug.";
  const parts: string[] = [];
  if (s.texts) parts.push(`${plural(s.texts, "text")} in ${plural(s.conversations, "conversation")}`);
  if (media) parts.push(`${mediaPhrase(s.photos, s.videos)} (${formatBytes(s.mediaBytes)})`);
  return `${parts.join(" and ")} to bring in.`;
}

function mediaPhrase(photos: number, videos: number): string {
  if (photos && videos) return `${plural(photos, "photo")} and ${plural(videos, "video")}`;
  if (videos) return plural(videos, "video");
  return plural(photos, "photo");
}

/** The photos checkbox's words. */
export function photosChoice(s: BackupScan): string {
  const media = s.photos + s.videos;
  if (!media) return "Photos and videos (none to copy)";
  return `Photos and videos · ${mediaPhrase(s.photos, s.videos)}, ${formatBytes(s.mediaBytes)}`;
}

/** The progress line while it works. */
export function progressLine(st: Pick<BackupStatus, "phase" | "stage" | "done" | "total" | "progress">): string {
  switch (st.phase) {
    case "unlocking":
      return `Unlocking your backup… ${Math.floor((st.progress ?? 0) * 100)}%`;
    case "reading":
      return "Reading your messages…";
    case "importing":
      if (!st.total) return "Getting started…";
      return st.stage === "photos"
        ? `Copying photos and videos… ${count(st.done)} of ${count(st.total)}`
        : `Adding texts… ${count(st.done)} of ${count(st.total)}`;
    default:
      return "";
  }
}

/** "Added 12,431 texts and 1,204 photos from 214 conversations." */
export function summaryLine(s: BackupSummary): string {
  if (s.texts === 0 && s.photos === 0 && s.videos === 0) return "Everything in this backup was already in tug.";
  const parts = [s.texts ? plural(s.texts, "text") : "", s.photos ? plural(s.photos, "photo") : "", s.videos ? plural(s.videos, "video") : ""].filter(Boolean);
  const what = parts.length > 1 ? `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}` : parts[0];
  const from = s.texts && s.conversations ? ` from ${plural(s.conversations, "conversation")}` : "";
  return `Added ${what}${from}.`;
}

/**
 * Group chats aren't brought in: Messages in tug holds only conversations it can reply to, and
 * Bluetooth has no way to send to a group. Said before and after, so nobody looks for them.
 */
export function groupsNote(s: Pick<BackupScan, "groupTexts">): string | null {
  if (!s.groupTexts) return null;
  return `Group chats stay on your iPhone (${plural(s.groupTexts, "text")}), since tug can't reply to a group.`;
}

/** A second line when some photos weren't in the backup. */
export function summaryNote(s: BackupSummary): string | null {
  if (!s.missing) return null;
  return `${plural(s.missing, "photo or video", "photos or videos")} weren't in the backup (your iPhone may keep them in iCloud).`;
}

/** Settings › Messages: what's in tug from a backup now, or null when nothing is. */
export function importedLine(i: BackupImported | null, now = new Date()): string | null {
  if (!i || (i.texts === 0 && i.photos === 0)) return null;
  const what = [i.texts ? plural(i.texts, "text") : "", i.photos ? plural(i.photos, "photo") : ""].filter(Boolean).join(" and ");
  return i.backupDate ? `${what} from your iPhone backup of ${shortDate(i.backupDate, now)}.` : `${what} from your iPhone backup.`;
}

/** The wizard's step, from the status and whether a password is being asked for. */
export type WizardStep = "pick" | "password" | "working" | "choose" | "done" | "stopped" | "failed";

export function wizardStep(st: Pick<BackupStatus, "phase" | "problem">, askingPassword: boolean): WizardStep {
  switch (st.phase) {
    case "unlocking":
    case "reading":
    case "importing":
      return "working";
    case "ready":
      return "choose";
    case "done":
      return "done";
    case "cancelled":
      return "stopped";
    case "failed":
      return st.problem === "wrongPassword" || st.problem === "needsPassword" ? "password" : "failed";
    default:
      return askingPassword ? "password" : "pick";
  }
}
