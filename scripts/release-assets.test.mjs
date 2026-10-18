import assert from "node:assert/strict";
import { test } from "node:test";
import {
  changelogSection,
  DOWNLOAD_BASE,
  findLeaks,
  keyCheck,
  latestJson,
  minisignKeyId,
  peImports,
  readVersions,
  releaseNotes,
  releaseVersion,
  sha256Line,
  unexpectedImports,
  versionFromTag,
} from "./release-assets.mjs";

const files = (v) => ({ "package.json": v, "src-tauri/tauri.conf.json": v, "src-tauri/Cargo.toml": v });

test("the tag names the version, and only a version tag does", () => {
  assert.equal(versionFromTag("v0.5.19"), "0.5.19");
  assert.equal(versionFromTag("v1.0.0-rc.1"), "1.0.0-rc.1");
  assert.equal(versionFromTag("0.5.19"), null);
  assert.equal(versionFromTag("v0.5"), null);
  assert.equal(versionFromTag("vnext"), null);
  assert.equal(versionFromTag("feat/auto-update"), null);
  assert.equal(versionFromTag(undefined), null);
});

test("the release version must match every version file", () => {
  assert.equal(releaseVersion({ ref: "v0.5.19", files: files("0.5.19") }), "0.5.19");
  assert.equal(releaseVersion({ ref: "main", input: "0.5.19", files: files("0.5.19") }), "0.5.19");
  assert.equal(releaseVersion({ ref: "main", input: " v0.5.19 ", files: files("0.5.19") }), "0.5.19");
  assert.throws(() => releaseVersion({ ref: "v0.5.19", files: files("0.5.18") }), /doesn't match package\.json \(0\.5\.18\)/);
  assert.throws(
    () => releaseVersion({ ref: "v0.5.19", files: { ...files("0.5.19"), "src-tauri/Cargo.toml": "0.5.18" } }),
    /Cargo\.toml/,
  );
  assert.throws(() => releaseVersion({ ref: "main", files: files("0.5.19") }), /isn't a version tag/);
  // A pull request's dry run builds the version the files carry.
  assert.equal(releaseVersion({ ref: "refs/pull/1/merge", files: files("0.5.19"), fromFiles: true }), "0.5.19");
  assert.throws(() => releaseVersion({ ref: "main", input: "latest; rm -rf /", files: files("0.5.19") }), /isn't a version/);
});

test("this checkout's version files agree with each other", () => {
  const v = readVersions();
  assert.equal(new Set(Object.values(v)).size, 1, JSON.stringify(v));
});

const CHANGELOG = `# Changelog

## v0.5.19 — 2026-10-24

Automatic updates with an Update button.

### New
- **Updates:** quietly in the background.

## v0.5.18 — 2026-10-20

Older notes.
`;

test("the notes are the matching CHANGELOG section, without its heading", () => {
  assert.equal(
    changelogSection(CHANGELOG, "0.5.19"),
    "Automatic updates with an Update button.\n\n### New\n- **Updates:** quietly in the background.",
  );
  assert.equal(changelogSection(CHANGELOG, "0.5.18"), "Older notes.");
  assert.equal(changelogSection(CHANGELOG, "0.5.1"), null, "0.5.1 isn't 0.5.18");
  assert.equal(changelogSection(CHANGELOG, "0.5.20"), null);
  assert.equal(changelogSection(CHANGELOG.replace(/\n/g, "\r\n"), "0.5.18"), "Older notes.");
});

test("latest.json is what the updater reads, for the NSIS installer", () => {
  const url = `${DOWNLOAD_BASE}/v0.5.19/tug-0.5.19-setup.exe`;
  const json = latestJson({ version: "0.5.19", notes: "n", pubDate: "2026-10-24T10:00:00Z", url, signature: "c2ln\n" });
  assert.deepEqual(json, {
    version: "0.5.19",
    notes: "n",
    pub_date: "2026-10-24T10:00:00Z",
    platforms: {
      "windows-x86_64-nsis": { signature: "c2ln", url },
      "windows-x86_64": { signature: "c2ln", url },
    },
  });
  assert.equal(url, "https://github.com/Davenee12/tug/releases/download/v0.5.19/tug-0.5.19-setup.exe");
  assert.throws(() => latestJson({ version: "0.5.19", pubDate: "x", url, signature: "  " }), /signature is empty/);
  assert.throws(() => latestJson({ version: "0.5.19", pubDate: "x", url: "http://example.com/a.exe", signature: "s" }), /https/);
  assert.throws(() => latestJson({ version: "next", pubDate: "x", url, signature: "s" }), /bad version/);
});

test("the release notes carry the checksums, and say when the build isn't code-signed", () => {
  const notes = releaseNotes({ version: "0.5.19", section: "Hello.", sums: "abc  tug-0.5.19-setup.exe\n", signed: false });
  assert.match(notes, /^Hello\./);
  assert.match(notes, /```\nabc {2}tug-0\.5\.19-setup\.exe\n```/);
  assert.match(notes, /Get-FileHash \.\\tug-0\.5\.19-setup\.exe/);
  assert.match(notes, /isn't code-signed/);
  assert.doesNotMatch(releaseNotes({ version: "0.5.19", section: "Hi", sums: "x", signed: true }), /code-signed/);
  assert.match(releaseNotes({ version: "0.5.19", section: null, sums: "x", signed: true }), /^tug 0\.5\.19\./);
});

// Made-up bytes in minisign's layout: no real key or signature exists for them.
const b64 = (s) => Buffer.from(s).toString("base64");
const fakePub = (id) => b64(`untrusted comment: minisign public key\n${Buffer.concat([Buffer.from("Ed"), Buffer.from(id, "hex"), Buffer.alloc(32, 1)]).toString("base64")}\n`);
const fakeSig = (id) =>
  b64(
    `untrusted comment: signature from tauri secret key\n${Buffer.concat([Buffer.from("ED"), Buffer.from(id, "hex"), Buffer.alloc(64, 2)]).toString("base64")}\ntrusted comment: timestamp:1\tfile:tug-0.5.19-setup.exe\tversion:0.5.19\n${Buffer.alloc(64, 3).toString("base64")}\n`,
  );

test("the updater signature must come from the key tug ships", () => {
  assert.equal(keyCheck(fakePub("0102030405060708"), fakeSig("0102030405060708")), "match");
  assert.equal(keyCheck(fakePub("0102030405060708"), fakeSig("1111111111111111")), "mismatch");
  assert.equal(keyCheck("", fakeSig("0102030405060708")), "missing");
  assert.equal(keyCheck("not base64 at all", fakeSig("0102030405060708")), "bad-pubkey");
  assert.equal(keyCheck(fakePub("0102030405060708"), "garbage"), "bad-signature");
  assert.equal(minisignKeyId(fakePub("a1b2c3d4e5f60718"), "pubkey"), "a1b2c3d4e5f60718");
  assert.equal(minisignKeyId(fakeSig("a1b2c3d4e5f60718"), "signature"), "a1b2c3d4e5f60718");
  assert.equal(minisignKeyId(fakeSig("a1b2c3d4e5f60718"), "pubkey"), null, "a signature isn't a public key");
});

test("checksums are sha256sum lines", () => {
  assert.equal(sha256Line(Buffer.from("abc"), "f.exe"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  f.exe");
});

test("a local path left in an exe is found in either slash style and in UTF-16", () => {
  const home = "C:\\Users\\example";
  const exe = (s, enc = "utf8") => Buffer.concat([Buffer.from([0, 1, 2]), Buffer.from(s, enc), Buffer.from([3])]);
  assert.deepEqual(findLeaks(exe("panicked at C:\\Users\\example\\.cargo\\x.rs"), [home]), [home]);
  assert.deepEqual(findLeaks(exe("C:/Users/example/.cargo/x.rs"), [home]), [home]);
  assert.deepEqual(findLeaks(exe("C:\\Users\\example\\x", "utf16le"), [home]), [home]);
  assert.deepEqual(findLeaks(exe("panicked at /cargo/registry/x.rs and ~/x"), [home]), []);
  assert.deepEqual(findLeaks(exe("anything"), ["", "C:"]), [], "empty or too-short paths never match");
});

/** A minimal 64-bit PE with these imported and delay-loaded DLLs (one section at RVA 0x1000). */
function fakeExe(imports, delayed = [], magic = 0x20b) {
  const buf = Buffer.alloc(0x1000);
  buf.write("MZ", 0, "latin1");
  buf.writeUInt32LE(0x40, 0x3c);
  buf.writeUInt32LE(0x00004550, 0x40);
  buf.writeUInt16LE(0x8664, 0x44);
  buf.writeUInt16LE(1, 0x46); // one section
  buf.writeUInt16LE(240, 0x54); // optional header size
  const opt = 0x58;
  buf.writeUInt16LE(magic, opt);
  const sec = opt + 240;
  buf.writeUInt32LE(0x800, sec + 8); // virtual size
  buf.writeUInt32LE(0x1000, sec + 12); // virtual address
  buf.writeUInt32LE(0x800, sec + 16); // raw size
  buf.writeUInt32LE(0x400, sec + 20); // raw offset
  const rva = (off) => off - 0x400 + 0x1000;
  let strings = 0x700;
  const name = (s) => {
    const at = strings;
    buf.write(`${s}\0`, at, "latin1");
    strings += s.length + 1;
    return rva(at);
  };
  const importTable = 0x400;
  imports.forEach((d, i) => buf.writeUInt32LE(name(d), importTable + i * 20 + 12));
  if (imports.length) buf.writeUInt32LE(rva(importTable), opt + 112 + 8);
  const delayTable = 0x500;
  delayed.forEach((d, i) => buf.writeUInt32LE(name(d), delayTable + i * 32 + 4));
  if (delayed.length) buf.writeUInt32LE(rva(delayTable), opt + 112 + 13 * 8);
  return buf;
}

test("an exe's imported and delay-loaded DLLs are read from its tables", () => {
  assert.deepEqual(peImports(fakeExe(["KERNEL32.dll", "msvcp140.dll"], ["comctl32.dll"])), [
    "kernel32.dll",
    "msvcp140.dll",
    "comctl32.dll",
  ]);
  assert.deepEqual(peImports(fakeExe([])), []);
  assert.throws(() => peImports(Buffer.from("not an exe at all, just text, long enough for a header....")), /MZ/);
  assert.throws(() => peImports(fakeExe(["kernel32.dll"], [], 0x10b)), /64-bit/, "a 32-bit exe (like the NSIS installer)");
});

test("a release exe may import only Windows' own DLLs", () => {
  assert.deepEqual(
    unexpectedImports(["kernel32.dll", "api-ms-win-crt-heap-l1-1-0.dll", "api-ms-win-core-winrt-l1-1-0.dll", "combase.dll"]),
    [],
  );
  assert.deepEqual(unexpectedImports(["kernel32.dll", "msvcp140.dll", "msvcp140.dll"]), ["msvcp140.dll"], "the C++ runtime");
  assert.deepEqual(unexpectedImports(["vcruntime140.dll", "vcruntime140_1.dll"]), ["vcruntime140.dll", "vcruntime140_1.dll"]);
  assert.deepEqual(unexpectedImports(["webview2loader.dll", "libclang.dll"]), ["webview2loader.dll", "libclang.dll"]);
  assert.deepEqual(unexpectedImports(peImports(fakeExe(["user32.dll"], ["msvcp140_atomic_wait.dll"]))), [
    "msvcp140_atomic_wait.dll",
  ]);
});
