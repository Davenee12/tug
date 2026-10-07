// Builds the `tug` command (src-tauri/crates/tug-cli) and puts it where tug's bundle picks it up:
// src-tauri/binaries/tug-cli.exe, installed as <tug folder>\bin\tug.exe (tauri.conf.json
// `bundle.resources`). `tauri dev` runs this before starting; `build:release` runs it with
// --release (inside remap-paths, so the exe carries no local paths either).
//
// Usage: node scripts/build-cli.mjs [--release]
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const tauriDir = path.join(repo, "src-tauri");
const release = process.argv.includes("--release");

const args = ["build", "--manifest-path", path.join(tauriDir, "Cargo.toml"), "-p", "tug-cli"];
if (release) args.push("--release");
const r = spawnSync("cargo", args, { stdio: "inherit" });
if (r.status !== 0) process.exit(r.status ?? 1);

const targetDir = process.env.CARGO_TARGET_DIR ? path.resolve(process.env.CARGO_TARGET_DIR) : path.join(tauriDir, "target");
const built = path.join(targetDir, release ? "release" : "debug", "tug-cli.exe");
const dest = path.join(tauriDir, "binaries", "tug-cli.exe");
fs.mkdirSync(path.dirname(dest), { recursive: true });
// Copy only when it changed, so tug isn't rebuilt for nothing (build.rs watches this file).
const same = fs.existsSync(dest) && Buffer.compare(fs.readFileSync(dest), fs.readFileSync(built)) === 0;
if (!same) fs.copyFileSync(built, dest);
console.log(`tug command: ${path.relative(repo, dest)}${same ? " (unchanged)" : ""}`);
