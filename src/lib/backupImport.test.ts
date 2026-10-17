import { describe, expect, it } from "vitest";
import {
  backupDetails,
  backupLabel,
  backupUnusable,
  formatBytes,
  importedLine,
  isImported,
  photosChoice,
  problemMessage,
  progressLine,
  scanLine,
  shortDate,
  summaryLine,
  summaryNote,
  wizardStep,
} from "./backupImport";
import type { BackupInfo, BackupScan } from "../types/protocol";

const NOW = new Date(2026, 9, 17, 12, 0);
const OCT_12 = new Date(2026, 9, 12, 9, 30).getTime();

const scan = (s: Partial<BackupScan> = {}): BackupScan => ({
  texts: 0,
  already: 0,
  conversations: 0,
  photos: 0,
  videos: 0,
  mediaBytes: 0,
  photoTexts: 0,
  ...s,
});

describe("backup import words", () => {
  it("sizes read like people read them", () => {
    expect(formatBytes(0)).toBe("0 bytes");
    expect(formatBytes(999)).toBe("999 bytes");
    expect(formatBytes(820_000)).toBe("820 KB");
    expect(formatBytes(3_210_000_000)).toBe("3.2 GB");
    expect(formatBytes(48_000_000_000)).toBe("48.0 GB");
    expect(formatBytes(150_000_000_000)).toBe("150 GB");
  });

  it("dates leave out this year", () => {
    expect(shortDate(OCT_12, NOW)).toBe("Oct 12");
    expect(shortDate(new Date(2024, 2, 3).getTime(), NOW)).toBe("Mar 3, 2024");
  });

  it("labels old history with the backup's date", () => {
    expect(backupLabel({ backupDate: OCT_12 }, NOW)).toBe("From your iPhone backup, Oct 12");
    expect(backupLabel({ backupDate: null }, NOW)).toBe("From your iPhone backup");
    expect(backupLabel(null, NOW)).toBe("From your iPhone backup");
    expect(isImported({ source: "iphone-backup" })).toBe(true);
    expect(isImported({ source: "iphone-map" })).toBe(false);
  });

  it("describes each backup in the picker", () => {
    const b: BackupInfo = {
      id: "x",
      deviceName: "Test iPhone",
      model: "iPhone16,2",
      iosVersion: "18.1",
      lastBackup: OCT_12,
      sizeBytes: 48_300_000_000,
      encrypted: true,
      problem: null,
    };
    expect(backupDetails(b, NOW)).toBe("iPhone 15 Pro Max · iOS 18.1 · Backed up Oct 12 · 48.3 GB");
    expect(backupDetails({ ...b, model: null, iosVersion: null, lastBackup: null }, NOW)).toBe("48.3 GB");
    expect(backupUnusable(b)).toBeNull();
    expect(backupUnusable({ problem: "tooOld" })).toMatch(/iOS 10/);
    expect(backupUnusable({ problem: "incomplete" })).toMatch(/didn't finish/);
  });

  it("says what a problem means and what to do", () => {
    expect(problemMessage("wrongPassword")).toMatch(/not your iPhone's passcode/);
    expect(problemMessage("noMessages")).toMatch(/Messages in iCloud/);
    expect(problemMessage("cantWrite")).toMatch(/free space/);
    expect(problemMessage(null)).toMatch(/Try again/);
  });

  it("says what a scan found", () => {
    expect(scanLine(scan())).toBe("Everything in this backup is already in tug.");
    expect(scanLine(scan({ texts: 12431, conversations: 214, photos: 1100, videos: 104, mediaBytes: 3_200_000_000 }))).toBe(
      "12,431 texts in 214 conversations and 1,100 photos and 104 videos (3.2 GB) to bring in.",
    );
    expect(scanLine(scan({ texts: 1, conversations: 1 }))).toBe("1 text in 1 conversation to bring in.");
    expect(scanLine(scan({ photos: 2, mediaBytes: 4_000_000 }))).toBe("2 photos (4.0 MB) to bring in.");
    expect(photosChoice(scan())).toBe("Photos and videos (none to copy)");
    expect(photosChoice(scan({ videos: 3, mediaBytes: 90_000_000 }))).toBe("Photos and videos · 3 videos, 90.0 MB");
  });

  it("follows the work", () => {
    expect(progressLine({ phase: "unlocking", stage: null, done: 0, total: 0, progress: 0.426 })).toBe("Unlocking your backup… 42%");
    expect(progressLine({ phase: "reading", stage: null, done: 0, total: 0, progress: null })).toBe("Reading your messages…");
    expect(progressLine({ phase: "importing", stage: "texts", done: 4000, total: 12431, progress: 0.3 })).toBe("Adding texts… 4,000 of 12,431");
    expect(progressLine({ phase: "importing", stage: "photos", done: 12, total: 1204, progress: 0.01 })).toBe(
      "Copying photos and videos… 12 of 1,204",
    );
    expect(progressLine({ phase: "importing", stage: "texts", done: 0, total: 0, progress: 0 })).toBe("Getting started…");
  });

  it("sums up what came in", () => {
    expect(summaryLine({ texts: 12431, photos: 1204, conversations: 214, already: 0, missing: 0 })).toBe(
      "Added 12,431 texts and 1,204 photos from 214 conversations.",
    );
    expect(summaryLine({ texts: 1, photos: 0, conversations: 1, already: 3, missing: 0 })).toBe("Added 1 text from 1 conversation.");
    expect(summaryLine({ texts: 0, photos: 5, conversations: 0, already: 9, missing: 0 })).toBe("Added 5 photos.");
    expect(summaryLine({ texts: 0, photos: 0, conversations: 0, already: 9, missing: 0 })).toBe(
      "Everything in this backup was already in tug.",
    );
    expect(summaryNote({ texts: 1, photos: 0, conversations: 1, already: 0, missing: 0 })).toBeNull();
    expect(summaryNote({ texts: 1, photos: 0, conversations: 1, already: 0, missing: 2 })).toMatch(/^2 photos or videos weren't/);
  });

  it("says what's in tug from a backup", () => {
    expect(importedLine(null)).toBeNull();
    expect(importedLine({ texts: 0, photos: 0, backupDate: null })).toBeNull();
    expect(importedLine({ texts: 12431, photos: 1204, backupDate: OCT_12 }, NOW)).toBe(
      "12,431 texts and 1,204 photos from your iPhone backup of Oct 12.",
    );
    expect(importedLine({ texts: 3, photos: 0, backupDate: null }, NOW)).toBe("3 texts from your iPhone backup.");
  });

  it("picks the wizard's step from the status", () => {
    expect(wizardStep({ phase: "idle", problem: null }, false)).toBe("pick");
    expect(wizardStep({ phase: "idle", problem: null }, true)).toBe("password");
    expect(wizardStep({ phase: "unlocking", problem: null }, true)).toBe("working");
    expect(wizardStep({ phase: "ready", problem: null }, false)).toBe("choose");
    expect(wizardStep({ phase: "importing", problem: null }, false)).toBe("working");
    expect(wizardStep({ phase: "done", problem: null }, false)).toBe("done");
    expect(wizardStep({ phase: "cancelled", problem: null }, false)).toBe("stopped");
    expect(wizardStep({ phase: "failed", problem: "wrongPassword" }, false)).toBe("password");
    expect(wizardStep({ phase: "failed", problem: "noMessages" }, false)).toBe("failed");
  });
});
