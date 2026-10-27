// Staged rollouts, from your PC (the GitHub CLI signed in as the owner). See docs/RELEASING.md.
//
//   node scripts/rollout.mjs status [version]        how far a release has rolled out, its
//                                                    downloads, and the problems reported against it
//                                                    since it was published (newest release if no
//                                                    version)
//   node scripts/rollout.mjs set <version> <percent> roll it out to that much of tug (0 pauses it,
//                                                    100 is everyone): rewrites the release's
//                                                    latest.json
//
// Every number here is public already (GitHub's download counts and issues). tug itself sends
// nothing: each copy keeps its rollout number to itself, and a report is only ever an issue the
// person chose to file.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { checkRollout } from "./release-assets.mjs";

export const REPO = "Davenee12/tug";

const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$/;

/** A version as typed (`0.5.22` or `v0.5.22`), checked. */
export function parseVersion(text) {
  const v = String(text ?? "").trim().replace(/^v/, "");
  if (!SEMVER.test(v)) throw new Error(`not a version: ${text}`);
  return v;
}

/** A percentage as typed (`10`, `10%`), checked. */
export function parsePercent(text) {
  const t = String(text ?? "").trim().replace(/%$/, "");
  if (!/^\d+$/.test(t)) throw new Error(`a rollout is a whole number from 0 to 100, not ${text}`);
  return checkRollout(Number(t));
}

/** The release's latest.json with its rollout set; everything else (the signature above all) kept. */
export function withRollout(latest, percent) {
  if (!latest || typeof latest !== "object" || !latest.platforms) throw new Error("that isn't tug's latest.json");
  return { ...latest, rollout: checkRollout(percent) };
}

/** How far it has rolled out: a release from before staged rollouts has no field and reaches everyone. */
export function currentRollout(latest) {
  const r = latest?.rollout;
  return Number.isInteger(r) && r >= 0 && r <= 100 ? r : 100;
}

/**
 * The title prefix tug's "Report a problem" gives an issue (`src-tauri/src/report.rs`), so issues
 * about one version can be found again.
 */
export function reportPrefix(version) {
  return `[v${version}]`;
}

/** Issues about this version: filed through tug (the prefix) or mentioning it, since it was published. */
export function issuesFor(issues, version, publishedAt) {
  const since = Date.parse(publishedAt);
  const prefix = reportPrefix(version);
  return issues.filter(
    (i) =>
      !i.pull_request &&
      Date.parse(i.created_at) >= since &&
      (i.title.startsWith(prefix) || i.title.includes(`v${version}`) || (i.body ?? "").includes(`tug version\n\n${version}`)),
  );
}

/** The status report, as lines to print. */
export function statusLines({ version, rollout, publishedAt, assets, issues }) {
  const count = (suffix) => assets.filter((a) => a.name.endsWith(suffix)).reduce((n, a) => n + a.download_count, 0);
  const crashes = issues.filter((i) => /closed unexpectedly/i.test(i.title));
  const lines = [
    `Rollout status: v${version} at ${rollout}%${rollout === 0 ? " (paused)" : rollout === 100 ? " (everyone)" : ""}`,
    `  Published:              ${publishedAt ?? "not yet (draft)"}`,
    `  Installer downloads:    ${count("-setup.exe")}  (new installs and updates)`,
    `  Portable downloads:     ${count("-portable.exe")}`,
    `  Problems reported:      ${issues.length}`,
    `  Crashes reported:       ${crashes.length}`,
  ];
  for (const i of issues) lines.push(`    #${i.number} ${i.title}  (${i.state})`);
  lines.push("", "Nothing here was sent by tug: downloads are GitHub's counts, and reports are issues people chose to file.");
  return lines;
}

function gh(args, opts = {}) {
  return execFileSync("gh", args, { encoding: opts.binary ? "buffer" : "utf8", stdio: ["ignore", "pipe", "inherit"] });
}

function release(version) {
  const which = version ? `tags/v${version}` : "latest";
  return JSON.parse(gh(["api", `repos/${REPO}/releases/${which}`]));
}

function readLatest(rel) {
  const asset = rel.assets.find((a) => a.name === "latest.json");
  if (!asset) throw new Error(`v${rel.tag_name} has no latest.json (the updater signing key wasn't set up for it)`);
  const raw = gh(["api", "-H", "Accept: application/octet-stream", `repos/${REPO}/releases/assets/${asset.id}`]);
  return JSON.parse(raw);
}

async function main([command, ...args]) {
  switch (command) {
    case "status": {
      const rel = release(args[0] ? parseVersion(args[0]) : null);
      const version = parseVersion(rel.tag_name);
      const latest = readLatest(rel);
      const all = JSON.parse(
        gh(["api", "--paginate", `repos/${REPO}/issues?state=all&per_page=100&since=${rel.published_at ?? rel.created_at}`]),
      );
      const issues = issuesFor(all, version, rel.published_at ?? rel.created_at);
      for (const line of statusLines({ version, rollout: currentRollout(latest), publishedAt: rel.published_at, assets: rel.assets, issues })) {
        console.log(line);
      }
      return;
    }
    case "set": {
      const version = parseVersion(args[0]);
      const percent = parsePercent(args[1]);
      const rel = release(version);
      const updated = withRollout(readLatest(rel), percent);
      const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tug-rollout-"));
      const file = path.join(dir, "latest.json");
      try {
        fs.writeFileSync(file, JSON.stringify(updated, null, 2) + "\n");
        // Replaces the asset in place; for a moment there's none, and a check then just tries again.
        gh(["release", "upload", `v${version}`, file, "--clobber", "--repo", REPO]);
      } finally {
        fs.rmSync(dir, { recursive: true, force: true });
      }
      console.log(`v${version} now rolls out to ${percent}% of tug${percent === 0 ? " (paused: nobody new gets it)" : ""}.`);
      if (rel.draft) console.log("It's still a draft: nobody gets it until you publish it.");
      return;
    }
    default:
      console.error("usage: node scripts/rollout.mjs status [version] | set <version> <percent>");
      process.exit(2);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((e) => {
    console.error(e.message);
    process.exit(1);
  });
}
