import { describe, expect, it } from "vitest";
import {
  autoDescription,
  blockerText,
  CLOSED,
  followStatus,
  heldBack,
  NOT_SET_UP,
  openConfirm,
  parseResume,
  pressRestart,
  restartFailed,
  resumeFor,
  updateAbout,
  updatePill,
} from "./update";
import type { UpdatePhase, UpdateStatus } from "../types/protocol";

const status = (s: Partial<UpdateStatus> = {}): UpdateStatus => ({
  phase: "idle",
  current: "0.5.18",
  version: null,
  headline: null,
  progress: null,
  failure: null,
  portable: false,
  auto: true,
  checkedAt: null,
  ...s,
});
const ready = (s: Partial<UpdateStatus> = {}) =>
  status({ phase: "ready", version: "0.5.19", headline: "Faster texts and a quieter tray.", ...s });

const ALL_PHASES: UpdatePhase[] = ["notConfigured", "idle", "checking", "upToDate", "available", "downloading", "ready", "applying", "failed"];

describe("updatePill", () => {
  it("shows Update once a downloaded update is ready, with the version in the tooltip", () => {
    expect(updatePill(ready())).toEqual({
      label: "Update",
      title: "tug 0.5.19 is ready — click to restart and update",
      action: "confirm",
    });
  });

  it("stays hidden while there's nothing to do, including mid-download", () => {
    for (const phase of ALL_PHASES.filter((p) => p !== "ready" && p !== "applying")) {
      expect(updatePill(status({ phase, version: "0.5.19" })), phase).toBeNull();
    }
    expect(updatePill(null)).toBeNull();
    expect(updatePill(status({ phase: "ready", version: null }))).toBeNull();
  });

  it("is never shown when updates aren't set up", () => {
    expect(updatePill(status({ phase: "notConfigured" }))).toBeNull();
  });

  it("on the portable exe offers the download instead (it can't update itself)", () => {
    const pill = updatePill(status({ phase: "available", version: "0.5.19", portable: true }));
    expect(pill).toMatchObject({ label: "Update available — Download", action: "download" });
    expect(pill?.title).toContain("0.5.19");
    // The portable copy never reaches "ready", but if it did it still wouldn't offer a restart.
    expect(updatePill(ready({ portable: true }))).toBeNull();
  });

  it("an installed copy with automatic downloads off doesn't show the button for a found release", () => {
    expect(updatePill(status({ phase: "available", version: "0.5.19" }))).toBeNull();
  });
});

describe("the confirm", () => {
  it("opens only for a ready update on an installed copy, with the notes' headline", () => {
    expect(openConfirm(ready())).toEqual({
      open: true,
      version: "0.5.19",
      headline: "Faster texts and a quieter tray.",
      applying: false,
      held: null,
      error: null,
    });
    expect(openConfirm(status({ phase: "downloading", version: "0.5.19" }))).toEqual(CLOSED);
    expect(openConfirm(ready({ portable: true }))).toEqual(CLOSED);
    expect(openConfirm(null)).toEqual(CLOSED);
  });

  it("Restart now waits for the answer, and a second press does nothing", () => {
    const pressed = pressRestart(openConfirm(ready()));
    expect(pressed).toMatchObject({ open: true, applying: true });
    expect(pressRestart(pressed)).toBe(pressed);
    expect(pressRestart(CLOSED)).toEqual(CLOSED);
  });

  it("a call, a transfer or voice typing holds the restart: it says so and keeps the confirm", () => {
    const pressed = pressRestart(openConfirm(ready()));
    const held = heldBack(pressed, "call");
    expect(held).toMatchObject({ open: true, applying: false, held: "call" });
    expect(blockerText("call")).toBe("Finish your call first");
    expect(blockerText("transfer")).toMatch(/Tugboat transfer/);
    expect(blockerText("voice")).toMatch(/voice typing/);
    // The update is still ready, so the button stays (the pill doesn't depend on the confirm).
    expect(updatePill(ready())).not.toBeNull();
    // Pressing again clears the message while it asks again.
    expect(pressRestart(held)).toMatchObject({ applying: true, held: null });
  });

  it("a failed restart says so and lets the person close it", () => {
    const failed = restartFailed(pressRestart(openConfirm(ready())), "tug couldn't start the update.");
    expect(failed).toMatchObject({ open: true, applying: false, error: "tug couldn't start the update." });
  });

  it("closes when the update is no longer ready", () => {
    const open = openConfirm(ready());
    expect(followStatus(open, ready())).toBe(open);
    expect(followStatus(open, ready({ phase: "applying" }))).toBe(open);
    expect(followStatus(open, status({ phase: "failed", failure: "apply" }))).toEqual(CLOSED);
    expect(followStatus(open, null)).toEqual(CLOSED);
    expect(followStatus(CLOSED, ready())).toEqual(CLOSED);
  });
});

describe("updateAbout", () => {
  it("says updates aren't set up, with no button or switch, when the build has no key", () => {
    expect(updateAbout(status({ phase: "notConfigured" }))).toEqual({ line: NOT_SET_UP, busy: false, action: null, showAuto: false });
    expect(updateAbout(null).line).toBe(NOT_SET_UP);
  });

  it("walks through Checking… to up to date, downloading and ready", () => {
    expect(updateAbout(status({ phase: "checking" }))).toMatchObject({ line: "Checking…", busy: true });
    expect(updateAbout(status({ phase: "upToDate" }))).toMatchObject({ line: "You're up to date", action: { run: "check" } });
    expect(updateAbout(status({ phase: "downloading", version: "0.5.19" })).line).toBe("Downloading…");
    expect(updateAbout(status({ phase: "downloading", version: "0.5.19", progress: 42 })).line).toBe("Downloading… 42%");
    expect(updateAbout(ready())).toMatchObject({ line: "Ready — Restart to update", action: { run: "restart" } });
    expect(updateAbout(status({ phase: "applying", version: "0.5.19" }))).toMatchObject({ busy: true, action: null });
  });

  it("offers Download for a found release when automatic downloads are off", () => {
    expect(updateAbout(status({ phase: "available", version: "0.5.19", auto: false }))).toMatchObject({
      line: "tug 0.5.19 is available",
      action: { label: "Download", run: "download" },
    });
  });

  it("on the portable exe, Download opens the release page", () => {
    expect(updateAbout(status({ phase: "available", version: "0.5.19", portable: true })).action).toEqual({ label: "Download", run: "open" });
    expect(autoDescription(true)).not.toMatch(/Download new versions/);
    expect(autoDescription(false)).toMatch(/only restarts when you click Update/);
  });

  it("a failure says Couldn't update with Try again, and never pretends otherwise", () => {
    expect(updateAbout(status({ phase: "failed", failure: "download" }))).toMatchObject({
      line: "Couldn't update",
      action: { label: "Try again", run: "check" },
    });
    expect(updateAbout(status({ phase: "failed", failure: "apply" })).line).toBe("Couldn't update");
    expect(updateAbout(status({ phase: "failed", failure: "check" })).line).toBe("Couldn't check for updates");
  });

  it("every phase has a line", () => {
    for (const phase of ALL_PHASES) expect(updateAbout(status({ phase })).line.length, phase).toBeGreaterThan(0);
  });
});

describe("coming back to the same screen", () => {
  it("round-trips every view and Settings section", () => {
    for (const view of ["feed", "messages", "calls"] as const) {
      expect(parseResume(resumeFor(view, "general"))).toEqual({ view });
    }
    expect(resumeFor("settings", "about")).toBe("settings:about");
    expect(parseResume("settings:about")).toEqual({ view: "settings", section: "about" });
    expect(parseResume("settings:iphone")).toEqual({ view: "settings", section: "iphone" });
  });

  it("ignores anything that isn't one of tug's screens", () => {
    for (const bad of [null, undefined, "", "settings", "settings:nope", "feed:extra", "nowhere", "settings:about:x"]) {
      expect(parseResume(bad), String(bad)).toBeNull();
    }
  });
});
