import { describe, expect, it } from "vitest";
import { scanOwners } from "./scanOwners";

describe("scanOwners", () => {
  it("stops the scan when the only panel lets go", () => {
    const scan = scanOwners();
    const panel = Symbol("panel");
    scan.hold(panel);
    expect(scan.release(panel)).toBe(true);
  });

  it("keeps scanning when a replaced panel unmounts after the new one started", () => {
    // Vue's order when the Feed's Connect panel replaces Settings › iPhone: the new panel is set up
    // (and starts scanning) before the old one's unmount stops it.
    const scan = scanOwners();
    const settingsPanel = Symbol("settings");
    const feedPanel = Symbol("feed");
    scan.hold(settingsPanel);
    scan.hold(feedPanel);
    expect(scan.release(settingsPanel)).toBe(false);
    expect(scan.release(feedPanel)).toBe(true);
  });

  it("lets a panel that never scanned stop a scan nobody holds", () => {
    // A panel showing a paired phone sends "stop" on every status; with nobody scanning it still does.
    const scan = scanOwners();
    expect(scan.release(Symbol("paired view"))).toBe(true);
  });

  it("doesn't let a panel that never scanned stop another panel's scan", () => {
    const scan = scanOwners();
    const finding = Symbol("finding");
    scan.hold(finding);
    expect(scan.release(Symbol("paired view"))).toBe(false);
  });

  it("counts a panel once however often it asks", () => {
    const scan = scanOwners();
    const panel = Symbol("panel");
    scan.hold(panel);
    scan.hold(panel);
    expect(scan.release(panel)).toBe(true);
  });
});
