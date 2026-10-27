import assert from "node:assert/strict";
import { test } from "node:test";
import {
  currentRollout,
  issuesFor,
  parsePercent,
  parseVersion,
  reportPrefix,
  statusLines,
  withRollout,
} from "./rollout.mjs";

const latest = {
  version: "0.5.22",
  notes: "n",
  pub_date: "2026-11-01T10:00:00Z",
  rollout: 10,
  platforms: { "windows-x86_64-nsis": { signature: "sig", url: "https://x" }, "windows-x86_64": { signature: "sig", url: "https://x" } },
};

test("versions and percentages are read the way people type them", () => {
  assert.equal(parseVersion("v0.5.22"), "0.5.22");
  assert.equal(parseVersion(" 0.5.22 "), "0.5.22");
  assert.throws(() => parseVersion("latest"), /not a version/);
  assert.equal(parsePercent("50"), 50);
  assert.equal(parsePercent("50%"), 50);
  assert.equal(parsePercent("0"), 0);
  for (const bad of ["101", "-5", "12.5", "half", ""]) assert.throws(() => parsePercent(bad), /0 to 100/, bad);
});

test("setting the rollout changes only the rollout", () => {
  const wider = withRollout(latest, 50);
  assert.equal(wider.rollout, 50);
  assert.deepEqual({ ...wider, rollout: 10 }, latest, "the signature and everything else are kept");
  assert.equal(latest.rollout, 10, "the original isn't changed");
  assert.throws(() => withRollout({ version: "0.5.22" }, 50), /latest.json/);
  assert.throws(() => withRollout(latest, 150), /0 to 100/);
});

test("a release from before staged rollouts reaches everyone", () => {
  assert.equal(currentRollout(latest), 10);
  assert.equal(currentRollout({ ...latest, rollout: undefined }), 100);
  assert.equal(currentRollout({ ...latest, rollout: "10" }), 100);
  assert.equal(currentRollout({ ...latest, rollout: 0 }), 0);
});

test("only issues about this version, filed since it was published, are counted", () => {
  const at = "2026-11-01T10:00:00Z";
  const issue = (number, title, created_at, extra = {}) => ({ number, title, created_at, state: "open", ...extra });
  const all = [
    issue(1, `${reportPrefix("0.5.22")} voice typing: no sound`, "2026-11-01T12:00:00Z"),
    issue(2, `${reportPrefix("0.5.22")} tug closed unexpectedly`, "2026-11-02T09:00:00Z"),
    issue(3, "Crash in v0.5.22 when pairing", "2026-11-02T10:00:00Z"),
    issue(4, `${reportPrefix("0.5.21")} older version`, "2026-11-02T10:00:00Z"),
    issue(5, `${reportPrefix("0.5.22")} before it was out`, "2026-10-31T10:00:00Z"),
    issue(6, `${reportPrefix("0.5.22")} a pull request`, "2026-11-02T10:00:00Z", { pull_request: {} }),
    issue(7, "Texts don't send", "2026-11-02T11:00:00Z", { body: "### tug version\n\n0.5.22\n" }),
  ];
  assert.deepEqual(
    issuesFor(all, "0.5.22", at).map((i) => i.number),
    [1, 2, 3, 7],
  );
  assert.equal(reportPrefix("0.5.22"), "[v0.5.22]");
});

test("the status adds up downloads and counts crash reports", () => {
  const lines = statusLines({
    version: "0.5.22",
    rollout: 10,
    publishedAt: "2026-11-01T10:00:00Z",
    assets: [
      { name: "tug-0.5.22-setup.exe", download_count: 150 },
      { name: "tug-0.5.22-portable.exe", download_count: 12 },
      { name: "latest.json", download_count: 9000 },
    ],
    issues: [
      { number: 2, title: "[v0.5.22] tug closed unexpectedly", state: "open" },
      { number: 1, title: "[v0.5.22] voice typing: no sound", state: "closed" },
    ],
  });
  const text = lines.join("\n");
  assert.match(text, /v0\.5\.22 at 10%/);
  assert.match(text, /Installer downloads:\s+150/);
  assert.match(text, /Portable downloads:\s+12/);
  assert.match(text, /Problems reported:\s+2/);
  assert.match(text, /Crashes reported:\s+1/);
  assert.match(text, /#1 \[v0\.5\.22\] voice typing: no sound  \(closed\)/);
  assert.match(statusLines({ version: "0.5.22", rollout: 0, publishedAt: null, assets: [], issues: [] }).join("\n"), /\(paused\)/);
});
