import { describe, expect, it } from "vitest";
import { compareVersions, isVersion, notesUpTo, RELEASE_NOTES, whatsNewToShow, type ReleaseNote } from "./whatsNew";

const NOTES: ReleaseNote[] = [
  { version: "0.5.9", title: "nine", highlights: ["a"] },
  { version: "0.5.8", title: "eight", highlights: ["b"] },
  { version: "0.5.7", title: "seven", highlights: ["c"] },
];

describe("compareVersions", () => {
  it("orders by each numeric part", () => {
    expect(compareVersions("0.5.9", "0.5.8")).toBeGreaterThan(0);
    expect(compareVersions("0.5.8", "0.5.9")).toBeLessThan(0);
    expect(compareVersions("0.6.0", "0.5.9")).toBeGreaterThan(0);
    expect(compareVersions("1.0.0", "0.9.9")).toBeGreaterThan(0);
  });

  it("treats equal and zero-padded versions as equal", () => {
    expect(compareVersions("0.5.8", "0.5.8")).toBe(0);
    expect(compareVersions("0.5", "0.5.0")).toBe(0);
    expect(compareVersions("0.5.0", "0.5")).toBe(0);
  });

  it("compares multi-digit parts numerically, not as strings", () => {
    expect(compareVersions("0.5.10", "0.5.9")).toBeGreaterThan(0);
    expect(compareVersions("0.10.0", "0.9.0")).toBeGreaterThan(0);
  });
});

describe("isVersion", () => {
  it("accepts dotted numbers and rejects everything else", () => {
    expect(isVersion("0.5.8")).toBe(true);
    expect(isVersion("1")).toBe(true);
    expect(isVersion(null)).toBe(false);
    expect(isVersion(undefined)).toBe(false);
    expect(isVersion("")).toBe(false);
    expect(isVersion("dev")).toBe(false);
    expect(isVersion("0.5.8-beta")).toBe(false);
  });
});

describe("whatsNewToShow", () => {
  it("shows the current version's note after an upgrade from the previous one", () => {
    const shown = whatsNewToShow(NOTES, "0.5.8", "0.5.7");
    expect(shown.map((n) => n.version)).toEqual(["0.5.8"]);
  });

  it("shows every skipped version, newest first, when several were missed", () => {
    const shown = whatsNewToShow(NOTES, "0.5.9", "0.5.7");
    expect(shown.map((n) => n.version)).toEqual(["0.5.9", "0.5.8"]);
  });

  it("never shows a note newer than the running version (an upcoming entry stays hidden)", () => {
    // Running 0.5.8 with 0.5.9 already in the file: 0.5.9 is held back.
    const shown = whatsNewToShow(NOTES, "0.5.8", "0.5.7");
    expect(shown.map((n) => n.version)).toEqual(["0.5.8"]);
  });

  it("shows nothing when the user has already seen the current version", () => {
    expect(whatsNewToShow(NOTES, "0.5.8", "0.5.8")).toEqual([]);
  });

  it("shows nothing when the last seen version is newer than the current (a downgrade)", () => {
    expect(whatsNewToShow(NOTES, "0.5.8", "0.5.9")).toEqual([]);
  });

  it("shows nothing on a fresh install (lastSeen null) so the card never interrupts onboarding", () => {
    expect(whatsNewToShow(NOTES, "0.5.8", null)).toEqual([]);
  });

  it("shows nothing when the current version can't be read", () => {
    expect(whatsNewToShow(NOTES, null, "0.5.7")).toEqual([]);
    expect(whatsNewToShow(NOTES, "dev", "0.5.7")).toEqual([]);
  });

  it("treats a corrupt stored version as nothing-seen and shows everything up to current", () => {
    const shown = whatsNewToShow(NOTES, "0.5.9", "garbage");
    expect(shown.map((n) => n.version)).toEqual(["0.5.9", "0.5.8", "0.5.7"]);
  });
});

describe("notesUpTo", () => {
  it("returns notes at or below the current version, newest first", () => {
    expect(notesUpTo(NOTES, "0.5.8").map((n) => n.version)).toEqual(["0.5.8", "0.5.7"]);
  });

  it("falls back to all notes, newest first, when the version is unreadable", () => {
    expect(notesUpTo(NOTES, "dev").map((n) => n.version)).toEqual(["0.5.9", "0.5.8", "0.5.7"]);
  });
});

describe("RELEASE_NOTES content", () => {
  it("is ordered newest first", () => {
    for (let i = 1; i < RELEASE_NOTES.length; i++) {
      expect(compareVersions(RELEASE_NOTES[i - 1].version, RELEASE_NOTES[i].version)).toBeGreaterThan(0);
    }
  });

  it("gives every release a title and 3–6 highlights", () => {
    for (const note of RELEASE_NOTES) {
      expect(isVersion(note.version)).toBe(true);
      expect(note.title.length).toBeGreaterThan(0);
      expect(note.highlights.length).toBeGreaterThanOrEqual(3);
      expect(note.highlights.length).toBeLessThanOrEqual(6);
    }
  });

  it("includes the seeded 0.5.8 and 0.5.9 entries", () => {
    expect(RELEASE_NOTES.map((n) => n.version)).toEqual(expect.arrayContaining(["0.5.8", "0.5.9"]));
  });
});
