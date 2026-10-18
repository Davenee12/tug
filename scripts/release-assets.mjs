// The release workflow's helpers (.github/workflows/release.yml), kept here so they're reviewed and
// tested like the rest of tug (`node --test scripts/release-assets.test.mjs`, part of npm run check).
//
//   node scripts/release-assets.mjs version            the version to release, checked against the
//                                                      tag (or the manual input) and every version
//                                                      file; written to $GITHUB_OUTPUT as version=
//   node scripts/release-assets.mjs stage <v> <out>    copies the built installer and app to
//                                                      <out>/tug-<v>-setup.exe and -portable.exe
//   node scripts/release-assets.mjs leaks <file>...    fails if a built exe still carries this
//                                                      machine's home or checkout path
//   node scripts/release-assets.mjs imports <file>...  fails if a built exe imports a DLL that
//                                                      isn't part of Windows (msvcp140, vcruntime140…)
//   node scripts/release-assets.mjs keycheck <setup.exe.sig>   fails if the updater signature was
//                                                      made with a key other than the one whose
//                                                      public half tauri.conf.json ships
//   node scripts/release-assets.mjs checksums <out> <file>...   SHA256SUMS.txt (sha256sum format)
//   node scripts/release-assets.mjs latest <v> <setup.exe> <out>   latest.json for the updater, from
//                                                      <setup.exe>.sig and the CHANGELOG section
//   node scripts/release-assets.mjs notes <v> <sums> <out>   the draft release's notes: the CHANGELOG
//                                                      section, then the checksums
//
// Inputs that come from people (the tag name, the manual version) are read from the environment
// (RELEASE_REF, RELEASE_INPUT), never pasted into a shell command, so they can't inject anything.
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** The download address of a release asset. */
export const DOWNLOAD_BASE = "https://github.com/Davenee12/tug/releases/download";

const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$/;

/** `v0.5.19` → `0.5.19`; anything that isn't a version tag → null. */
export function versionFromTag(ref) {
  const m = /^v(.+)$/.exec(ref ?? "");
  return m && SEMVER.test(m[1]) ? m[1] : null;
}

/**
 * The version this run releases: from the tag on a tag push, or the manual input (with or without
 * a leading v). Every version file must already say the same; the workflow never edits versions.
 */
export function releaseVersion({ ref, input, files, fromFiles = false }) {
  // A pull request's dry run builds whatever version the files say.
  if (fromFiles && !input) input = files["package.json"];
  const wanted = input ? input.trim().replace(/^v/, "") : versionFromTag(ref);
  if (!wanted || !SEMVER.test(wanted)) {
    throw new Error(input ? `"${input}" isn't a version like 0.5.19` : `the tag "${ref}" isn't a version tag like v0.5.19`);
  }
  const wrong = Object.entries(files).filter(([, v]) => v !== wanted);
  if (wrong.length) {
    throw new Error(`version ${wanted} doesn't match ${wrong.map(([f, v]) => `${f} (${v})`).join(", ")}`);
  }
  return wanted;
}

/** The versions tug's files carry (see CONTRIBUTING.md › Releases). */
export function readVersions(root = repo) {
  const json = (f) => JSON.parse(fs.readFileSync(path.join(root, f), "utf8"));
  const cargo = fs.readFileSync(path.join(root, "src-tauri/Cargo.toml"), "utf8");
  return {
    "package.json": json("package.json").version,
    "src-tauri/tauri.conf.json": json("src-tauri/tauri.conf.json").version,
    "src-tauri/Cargo.toml": /^\[package\][^[]*?^version\s*=\s*"([^"]+)"/m.exec(cargo)?.[1] ?? "(none)",
  };
}

/** The body of CHANGELOG.md's `## v<version> …` section (without its heading). */
export function changelogSection(changelog, version) {
  const lines = changelog.replace(/\r\n/g, "\n").split("\n");
  const esc = version.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const start = lines.findIndex((l) => new RegExp(`^## v?${esc}(\\s|$)`).test(l));
  if (start < 0) return null;
  let end = lines.findIndex((l, i) => i > start && /^## /.test(l));
  if (end < 0) end = lines.length;
  return lines.slice(start + 1, end).join("\n").trim();
}

/**
 * latest.json as tauri-plugin-updater reads it: the version, notes, date and, for Windows x64, the
 * NSIS installer's address and its minisign signature (the `.sig` file's text). The updater looks
 * for `windows-x86_64-nsis` first, then `windows-x86_64`; both point at the same installer.
 */
export function latestJson({ version, notes, pubDate, url, signature }) {
  if (!SEMVER.test(version)) throw new Error(`bad version ${version}`);
  if (!/^https:\/\//.test(url)) throw new Error("the installer's address must be https");
  const sig = signature.trim();
  if (!sig) throw new Error("the updater signature is empty");
  const platform = { signature: sig, url };
  return {
    version,
    notes: notes ?? "",
    pub_date: pubDate,
    platforms: { "windows-x86_64-nsis": platform, "windows-x86_64": platform },
  };
}

/** The draft release's notes: the CHANGELOG section, then how to check the downloads. */
export function releaseNotes({ version, section, sums, signed }) {
  const parts = [section?.trim() || `tug ${version}.`];
  parts.push(
    [
      "### Checksums (SHA-256)",
      "",
      "```",
      sums.trim(),
      "```",
      "",
      "Check a download in PowerShell with `Get-FileHash .\\tug-" + version + "-setup.exe` and compare.",
    ].join("\n"),
  );
  if (!signed) parts.push("_This build isn't code-signed yet, so Windows SmartScreen may warn about it._");
  return parts.join("\n\n") + "\n";
}

/**
 * The 8-byte key id inside a minisign public key or signature as Tauri stores them (base64 of the
 * minisign file's text). Public key line: 2-byte algorithm, key id, 32-byte key. Signature line:
 * 2-byte algorithm, key id, 64-byte signature. Null if it isn't one.
 */
export function minisignKeyId(b64, kind) {
  let text;
  try {
    text = Buffer.from((b64 ?? "").trim(), "base64").toString("utf8");
  } catch {
    return null;
  }
  const lines = text.split(/\r?\n/);
  if (!lines[0]?.startsWith("untrusted comment:") || !lines[1]) return null;
  const raw = Buffer.from(lines[1].trim(), "base64");
  const size = kind === "pubkey" ? 42 : 74;
  if (raw.length !== size || raw[0] !== 0x45 /* E */) return null;
  return raw.subarray(2, 10).toString("hex");
}

/**
 * Whether the updater signature was made with the key whose public half tug ships: their key ids
 * must match, or every installed copy would refuse the update. ("missing": tug ships no key yet.)
 */
export function keyCheck(pubkey, signature) {
  if (!pubkey?.trim()) return "missing";
  const pub = minisignKeyId(pubkey, "pubkey");
  const sig = minisignKeyId(signature, "signature");
  if (!pub) return "bad-pubkey";
  if (!sig) return "bad-signature";
  return pub === sig ? "match" : "mismatch";
}

/** One line of `sha256sum` output. */
export function sha256Line(bytes, name) {
  return `${crypto.createHash("sha256").update(bytes).digest("hex")}  ${name}`;
}

/** The ways a path can appear inside an exe: as written, with / or \, and as UTF-16 (Windows APIs). */
function variants(p) {
  const forms = new Set([p, p.replace(/\\/g, "/"), p.replace(/\//g, "\\")]);
  return [...forms].flatMap((f) => [Buffer.from(f, "utf8"), Buffer.from(f, "utf16le")]);
}

/** Which of `paths` occur in `bytes` (case-sensitive, as rustc writes them). */
export function findLeaks(bytes, paths) {
  return paths.filter((p) => p && p.length > 3 && variants(p).some((v) => bytes.includes(v)));
}

/**
 * The DLLs a Windows exe (PE32+) imports, from its import table and its delay-load table,
 * lowercase, in table order. Throws on something that isn't a 64-bit PE.
 */
export function peImports(bytes) {
  const buf = Buffer.from(bytes);
  if (buf.length < 0x40 || buf.readUInt16LE(0) !== 0x5a4d) throw new Error("not an exe (no MZ header)");
  const pe = buf.readUInt32LE(0x3c);
  if (buf.readUInt32LE(pe) !== 0x00004550) throw new Error("not a PE file");
  const sections = buf.readUInt16LE(pe + 6);
  const optSize = buf.readUInt16LE(pe + 20);
  const opt = pe + 24;
  if (buf.readUInt16LE(opt) !== 0x20b) throw new Error("not a 64-bit (PE32+) exe");
  const dirs = opt + 112;
  const table = [];
  for (let i = 0, s = opt + optSize; i < sections; i++, s += 40) {
    table.push({ va: buf.readUInt32LE(s + 12), size: Math.max(buf.readUInt32LE(s + 8), buf.readUInt32LE(s + 16)), raw: buf.readUInt32LE(s + 20) });
  }
  const offset = (rva) => {
    const sec = table.find((t) => rva >= t.va && rva < t.va + t.size);
    if (!sec) throw new Error(`RVA 0x${rva.toString(16)} is in no section`);
    return rva - sec.va + sec.raw;
  };
  const cstring = (rva) => {
    const at = offset(rva);
    const end = buf.indexOf(0, at);
    return buf.toString("latin1", at, end).toLowerCase();
  };
  const names = [];
  // [directory index, descriptor size, offset of the DLL name's RVA in the descriptor]
  for (const [index, size, nameAt] of [
    [1, 20, 12], // IMAGE_IMPORT_DESCRIPTOR
    [13, 32, 4], // IMAGE_DELAYLOAD_DESCRIPTOR
  ]) {
    const rva = buf.readUInt32LE(dirs + index * 8);
    if (!rva) continue;
    for (let d = offset(rva); ; d += size) {
      const name = buf.readUInt32LE(d + nameAt);
      if (!name) break;
      names.push(cstring(name));
    }
  }
  return names;
}

/**
 * DLLs a release exe may import: parts of Windows itself (every supported Windows 10 and 11 has
 * them, including the Universal CRT's api-ms-win-crt-* forwarders). Anything else, above all the
 * Visual C++ runtime (msvcp140, vcruntime140), would make tug fail to start on a PC without the
 * matching redistributable.
 */
export const SYSTEM_DLLS = new Set([
  "advapi32.dll",
  "bcrypt.dll",
  "bcryptprimitives.dll",
  "combase.dll",
  "comctl32.dll",
  "comdlg32.dll",
  "crypt32.dll",
  "d3d11.dll",
  "dbghelp.dll",
  "dwmapi.dll",
  "dxgi.dll",
  "gdi32.dll",
  "gdiplus.dll",
  "imm32.dll",
  "iphlpapi.dll",
  "kernel32.dll",
  "mpr.dll",
  "msimg32.dll",
  "ncrypt.dll",
  "netapi32.dll",
  "ntdll.dll",
  "ole32.dll",
  "oleaut32.dll",
  "powrprof.dll",
  "propsys.dll",
  "rpcrt4.dll",
  "secur32.dll",
  "setupapi.dll",
  "shell32.dll",
  "shlwapi.dll",
  "ucrtbase.dll",
  "user32.dll",
  "userenv.dll",
  "uxtheme.dll",
  "version.dll",
  "winhttp.dll",
  "winmm.dll",
  "ws2_32.dll",
  "wtsapi32.dll",
]);

/** The imports a release exe mustn't have: anything not part of Windows. */
export function unexpectedImports(dlls) {
  return [...new Set(dlls)].filter((d) => !SYSTEM_DLLS.has(d) && !/^(api|ext)-ms-win-[a-z0-9-]+\.dll$/.test(d));
}

function writeOutput(name, value) {
  if (process.env.GITHUB_OUTPUT) fs.appendFileSync(process.env.GITHUB_OUTPUT, `${name}=${value}\n`);
}

function targetDir() {
  return process.env.CARGO_TARGET_DIR ? path.resolve(process.env.CARGO_TARGET_DIR) : path.join(repo, "src-tauri", "target");
}

function main([cmd, ...args]) {
  switch (cmd) {
    case "version": {
      const v = releaseVersion({
        ref: process.env.RELEASE_REF,
        input: process.env.RELEASE_INPUT,
        files: readVersions(),
        fromFiles: process.env.RELEASE_FROM_FILES === "true",
      });
      writeOutput("version", v);
      console.log(v);
      return;
    }
    case "stage": {
      const [version, out] = args;
      const release = path.join(targetDir(), "release");
      const setup = path.join(release, "bundle", "nsis", `tug_${version}_x64-setup.exe`);
      const app = path.join(release, "tug.exe");
      for (const f of [setup, app]) if (!fs.existsSync(f)) throw new Error(`missing build output ${f}`);
      fs.mkdirSync(out, { recursive: true });
      fs.copyFileSync(setup, path.join(out, `tug-${version}-setup.exe`));
      fs.copyFileSync(app, path.join(out, `tug-${version}-portable.exe`));
      console.log(`staged tug-${version}-setup.exe and tug-${version}-portable.exe in ${out}`);
      return;
    }
    case "leaks": {
      const paths = [os.homedir(), repo, process.env.CARGO_TARGET_DIR ?? ""].map((p) => p && path.resolve(p));
      let bad = false;
      for (const f of args) {
        const found = findLeaks(fs.readFileSync(f), paths);
        if (found.length) {
          bad = true;
          console.error(`${path.basename(f)} still contains ${found.length} local path(s); path remapping didn't apply`);
        } else console.log(`${path.basename(f)}: no local paths`);
      }
      if (bad) process.exit(1);
      return;
    }
    case "imports": {
      let bad = false;
      for (const f of args) {
        const dlls = peImports(fs.readFileSync(f));
        const extra = unexpectedImports(dlls);
        console.log(`${path.basename(f)} imports: ${[...new Set(dlls)].join(", ")}`);
        if (extra.length) {
          bad = true;
          console.error(
            `::error::${path.basename(f)} imports ${extra.join(", ")}, which aren't part of Windows: it wouldn't start on a PC without them (the Visual C++ runtime is linked statically; see .cargo/config.toml)`,
          );
        }
      }
      if (bad) process.exit(1);
      return;
    }
    case "keycheck": {
      const [sigFile] = args;
      const conf = JSON.parse(fs.readFileSync(path.join(repo, "src-tauri/tauri.conf.json"), "utf8"));
      const result = keyCheck(conf.plugins?.updater?.pubkey, fs.readFileSync(sigFile, "utf8"));
      switch (result) {
        case "match":
          console.log("the updater signature matches the public key in tauri.conf.json");
          return;
        case "missing":
          console.warn(
            "::warning::tauri.conf.json has no updater public key, so this version won't check for updates. latest.json is still published for the copies that do.",
          );
          return;
        default:
          throw new Error(
            `${result}: the updater signature wasn't made with the key whose public half is in tauri.conf.json, so installed copies would refuse it. See docs/RELEASING.md.`,
          );
      }
    }
    case "checksums": {
      const [out, ...files] = args;
      const lines = files.map((f) => sha256Line(fs.readFileSync(f), path.basename(f)));
      fs.writeFileSync(out, lines.join("\n") + "\n");
      console.log(lines.join("\n"));
      return;
    }
    case "latest": {
      const [version, setup, out] = args;
      const section = changelogSection(fs.readFileSync(path.join(repo, "CHANGELOG.md"), "utf8"), version);
      const json = latestJson({
        version,
        notes: section ?? "",
        pubDate: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
        url: `${DOWNLOAD_BASE}/v${version}/${path.basename(setup)}`,
        signature: fs.readFileSync(`${setup}.sig`, "utf8"),
      });
      fs.writeFileSync(out, JSON.stringify(json, null, 2) + "\n");
      console.log(`latest.json for ${version} → ${json.platforms["windows-x86_64"].url}`);
      return;
    }
    case "notes": {
      const [version, sums, out] = args;
      const section = changelogSection(fs.readFileSync(path.join(repo, "CHANGELOG.md"), "utf8"), version);
      if (!section) console.warn(`::warning::CHANGELOG.md has no "## v${version}" section; the notes say only the version`);
      fs.writeFileSync(
        out,
        releaseNotes({ version, section, sums: fs.readFileSync(sums, "utf8"), signed: process.env.RELEASE_SIGNED === "true" }),
      );
      return;
    }
    default:
      console.error("usage: node scripts/release-assets.mjs version|stage|leaks|imports|keycheck|checksums|latest|notes …");
      process.exit(2);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (e) {
    console.error(`release-assets: ${e.message}`);
    process.exit(1);
  }
}
