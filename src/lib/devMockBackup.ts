// Dev-only: the iPhone-backup commands for the browser preview (`npm run dev`), so the "Bring in
// your full history" wizard, imported conversations and photos can be reviewed
// without a backup. Loaded by devMock.ts; never bundled into tug.
//
//   /?backup            two made-up backups (one encrypted: its password is "test"); the import
//                       adds a few weeks of history and photos (group chats stay out)
//   /?backup&none       no backups on this PC: the "how to make one" step
//   /?backup&imported   history already brought in (Settings shows Remove, Messages the label)
//   /?backup&heif       photos show as "Open photo" with the HEIF Image Extensions hint

import { emit } from "@tauri-apps/api/event";
import type { BackupImported, BackupInfo, BackupStatus, SmsMessage } from "../types/protocol";

const params = new URLSearchParams(location.search);
const on = params.has("backup");
const min = 60_000;
const day = 24 * 60 * min;
const now = Date.now();

const backups: BackupInfo[] =
  !on || params.has("none")
    ? []
    : [
        {
          id: "00008110-000A11112222801E",
          deviceName: "Jordan's iPhone",
          model: "iPhone16,2",
          iosVersion: "18.1",
          lastBackup: now - 5 * day,
          sizeBytes: 48_300_000_000,
          encrypted: true,
          problem: null,
        },
        {
          id: "00008030-000B33334444802E",
          deviceName: "Old iPhone",
          model: "iPhone12,1",
          iosVersion: "9.3",
          lastBackup: now - 800 * day,
          sizeBytes: 12_000_000_000,
          encrypted: false,
          problem: "tooOld",
        },
      ];

let status: BackupStatus = {
  rev: 0,
  phase: "idle",
  backup: null,
  progress: null,
  stage: null,
  done: 0,
  total: 0,
  scan: null,
  summary: null,
  problem: null,
};
let timer: number | undefined;
let imported: BackupImported = { texts: 0, photos: 0, backupDate: null };

function set(patch: Partial<BackupStatus>) {
  status = { ...status, ...patch, rev: status.rev + 1 };
  void emit("backup-import", status);
}

function stop() {
  window.clearInterval(timer);
  timer = undefined;
}

/** A tiny photo, drawn here, standing in for a JPEG preview. */
function fakePhoto(hue: number): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="240" height="180" viewBox="0 0 240 180"><rect width="240" height="180" fill="hsl(${hue} 45% 62%)"/><circle cx="180" cy="50" r="22" fill="hsl(${hue} 70% 88%)"/><path d="M0 180 L80 90 L140 150 L180 115 L240 180 Z" fill="hsl(${hue} 35% 38%)"/></svg>`;
  return `data:image/svg+xml;base64,${btoa(svg)}`;
}

/** What an import adds to the window's history (the backend's rows, made up). */
function history(nextId: () => number, zoe: string): SmsMessage[] {
  const backupDate = now - 5 * day;
  const m = (direction: "in" | "out", body: string, agoMin: number, extra: Partial<SmsMessage> = {}): SmsMessage => ({
    id: nextId(),
    source: "iphone-backup",
    direction,
    address: zoe,
    contactName: "Zoe",
    body,
    sentAt: new Date(backupDate - agoMin * min).toISOString(),
    receivedAt: backupDate - agoMin * min,
    status: direction === "in" ? "received" : "sent",
    msgType: "IM",
    gapBefore: false,
    edited: false,
    unsent: false,
    attachments: [],
    ...extra,
  });
  let att = 900;
  const photo = () => ({ id: att++, kind: "photo" as const, stored: true, bytes: 2_400_000 });
  return [
    m("out", "are we still on for saturday?", 60 * 24 * 20),
    m("in", "yes!! can't wait", 60 * 24 * 20 - 3),
    m("in", "", 60 * 24 * 12, { attachments: [photo(), photo()] }),
    m("out", "that view 😍", 60 * 24 * 12 - 2, { edited: true }),
    m("out", "", 60 * 24 * 12 - 4, { unsent: true }),
    m("in", "the summit at sunrise", 60 * 24 * 3, { attachments: [photo()] }),
  ];
}

/** Handles a backup command, or returns undefined for anything else. */
export function backupMock(
  cmd: string,
  a: Record<string, unknown>,
  ctx: { messages: SmsMessage[]; nextId: () => number; zoe: string },
): unknown {
  switch (cmd) {
    case "backup_list":
      return new Promise((r) => setTimeout(() => r(backups.map((b) => ({ ...b }))), 400));
    case "backup_pick_folder":
      return backups[0] ? { ...backups[0] } : null;
    case "backup_status":
      return { ...status };
    case "backup_open": {
      const b = backups.find((x) => x.id === a.id) ?? null;
      if (b?.encrypted && !a.password) {
        set({ phase: "failed", backup: b, problem: "needsPassword" });
        return null;
      }
      set({ phase: b?.encrypted ? "unlocking" : "reading", backup: b, progress: b?.encrypted ? 0 : null, problem: null, scan: null });
      let p = 0;
      stop();
      timer = window.setInterval(() => {
        p += 0.08;
        if (status.phase === "unlocking" && p < 1) return set({ progress: p });
        if (status.phase === "unlocking") {
          if (a.password !== "test") {
            stop();
            return set({ phase: "failed", progress: null, problem: "wrongPassword" });
          }
          return set({ phase: "reading", progress: null });
        }
        stop();
        set({
          phase: "ready",
          scan: { texts: 12431, already: 9, conversations: 214, photos: 1100, videos: 104, mediaBytes: 3_200_000_000, photoTexts: 980, groupTexts: 1832 },
        });
      }, 120);
      return null;
    }
    case "backup_import": {
      const total = 12431;
      const photos = 1204;
      set({ phase: "importing", stage: "texts", done: 0, total, progress: 0 });
      stop();
      timer = window.setInterval(() => {
        if (status.stage === "texts" && status.done < total) {
          const done = Math.min(total, status.done + 900);
          return set({ done, progress: done / total });
        }
        if (status.stage === "texts" && a.photos) return set({ stage: "photos", done: 0, total: photos, progress: 0 });
        if (status.stage === "photos" && status.done < photos) {
          const done = Math.min(photos, status.done + 120);
          return set({ done, progress: done / photos });
        }
        stop();
        ctx.messages.push(...history(ctx.nextId, ctx.zoe));
        imported = { texts: total, photos: a.photos ? photos : 0, backupDate: now - 5 * day };
        set({
          phase: "done",
          progress: null,
          summary: {
            texts: total,
            photos: a.photos ? photos - 104 : 0,
            videos: a.photos ? 104 : 0,
            conversations: 214,
            already: 9,
            missing: a.photos ? 3 : 0,
            groupTexts: 1832,
          },
        });
      }, 150);
      return null;
    }
    case "backup_cancel":
      stop();
      if (["unlocking", "reading", "importing"].includes(status.phase)) set({ phase: "cancelled", progress: null });
      else set({ phase: "idle", backup: null, scan: null, summary: null, problem: null, progress: null });
      return null;
    case "backup_imported":
      return { ...imported };
    case "backup_remove_imported": {
      const before = ctx.messages.length;
      for (let i = ctx.messages.length - 1; i >= 0; i--) if (ctx.messages[i].source === "iphone-backup") ctx.messages.splice(i, 1);
      imported = { texts: 0, photos: 0, backupDate: null };
      return before - ctx.messages.length;
    }
    case "conversation_heads":
    case "conversation_messages":
      return ctx.messages.map((m) => ({ ...m }));
    case "attachment_preview":
      return params.has("heif") ? { dataUri: null, needsHeif: true } : { dataUri: fakePhoto((Number(a.id) * 47) % 360), needsHeif: false };
    case "open_attachment":
      console.info("[devMock] would open the photo in Photos", a.id);
      return null;
    default:
      return undefined;
  }
}

/** ?backup&imported: start with history already brought in. */
export function seedImported(ctx: { messages: SmsMessage[]; nextId: () => number; zoe: string }) {
  if (!on || !params.has("imported")) return;
  ctx.messages.push(...history(ctx.nextId, ctx.zoe));
  imported = { texts: 12431, photos: 1204, backupDate: now - 5 * day };
}
