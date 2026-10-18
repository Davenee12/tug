// Runs a command (normally `tauri build`) with this machine's paths remapped out of the Rust
// build. Without it, release exes carry absolute paths such as
// C:\Users\<name>\.cargo\registry\src\... (panic locations in dependencies), which puts the
// Windows user name into every published binary.
//
// Why a script and not .cargo/config.toml: Cargo's `trim-paths` profile option is still
// nightly-only, and config.toml rustflags can't read environment variables, so the only way to
// put the real paths in a committed file would be to hard-code them (user name included).
//
// Usage: node scripts/remap-paths.mjs <command> [args...]
//        (npm run build:release wraps `tauri build`; extra args pass through)
//        node scripts/remap-paths.mjs --print   shows the rustc flags without running anything
// It also trims the target folder from C/C++ __FILE__ paths (MSVC's /d1trimfile, see below).
import { spawnSync } from "node:child_process";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { quoteArg } from "./shell-args.mjs";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const home = os.homedir();
const cargoHome = process.env.CARGO_HOME || path.join(home, ".cargo");
const rustupHome = process.env.RUSTUP_HOME || path.join(home, ".rustup");

// rustc applies the last matching remap, so the broad home-directory rule goes first and the
// more specific ones after it.
const remaps = [
  [home, "~"],
  [rustupHome, "/rustup"],
  [cargoHome, "/cargo"],
  [repo, "/tug"],
];
if (process.env.CARGO_TARGET_DIR) remaps.push([path.resolve(process.env.CARGO_TARGET_DIR), "/target"]);

const flags = remaps.map(([from, to]) => `--remap-path-prefix=${from}=${to}`);
// CARGO_ENCODED_RUSTFLAGS replaces the rustflags in .cargo/config.toml, so the static C/C++ runtime
// it asks for is asked for here too: without it tug.exe would import msvcp140.dll (whisper.cpp's
// C++ standard library) and need the Visual C++ redistributable. See .cargo/config.toml.
if (process.platform === "win32") flags.push("-Ctarget-feature=+crt-static");

// Keep any flags the caller already set. CARGO_ENCODED_RUSTFLAGS (0x1f-separated) wins over
// RUSTFLAGS in Cargo and copes with spaces in paths, so the result goes there.
const existing = process.env.CARGO_ENCODED_RUSTFLAGS
  ? process.env.CARGO_ENCODED_RUSTFLAGS.split("\x1f")
  : (process.env.RUSTFLAGS ?? "").split(/\s+/);
const encoded = [...existing.filter(Boolean), ...flags].join("\x1f");

const [cmd, ...args] = process.argv.slice(2);
if (!cmd || cmd === "--print") {
  console.log(flags.join("\n"));
  process.exit(cmd ? 0 : 1);
}

const env = { ...process.env, CARGO_ENCODED_RUSTFLAGS: encoded };
delete env.RUSTFLAGS;

// C and C++ that build scripts compile with MSVC (whisper.cpp for voice typing, SQLite) is out of
// rustc's reach: cl.exe writes __FILE__ (ggml's and whisper.cpp's assert messages) as a full path
// under the target folder. /d1trimfile cuts that prefix off. cl.exe applies only one, so it's the
// target folder, where build scripts put the sources they compile. The cc crate splits CFLAGS on
// spaces, so a target path with a space can't be passed; the leak check after the build would say.
const target = process.env.CARGO_TARGET_DIR ? path.resolve(process.env.CARGO_TARGET_DIR) : path.join(repo, "src-tauri", "target");
if (process.platform === "win32" && !/\s/.test(target)) {
  const trim = `/d1trimfile:${target}${path.sep}`;
  for (const name of ["CFLAGS", "CXXFLAGS"]) env[name] = [process.env[name], trim].filter(Boolean).join(" ");
} else if (process.platform === "win32") {
  console.warn(`remap-paths: the target folder has a space in it, so C/C++ __FILE__ paths can't be trimmed: ${target}`);
}
// shell: true so npm's .cmd shims (tauri.cmd) resolve on Windows; quoteArg keeps each argument
// whole through cmd.exe and refuses ones it can't pass on unchanged.
let line;
try {
  line = [cmd, ...args].map(quoteArg).join(" ");
} catch (e) {
  console.error(`remap-paths: ${e.message}`);
  process.exit(1);
}
const r = spawnSync(line, { stdio: "inherit", env, shell: true });
process.exit(r.status ?? 1);
