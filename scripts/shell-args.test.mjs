// Run with `node --test scripts/shell-args.test.mjs` (part of `npm run check`).
import assert from "node:assert/strict";
import { test } from "node:test";
import { quoteArg } from "./shell-args.mjs";

test("flags, values and plain paths pass through untouched", () => {
  assert.equal(quoteArg("tauri"), "tauri");
  assert.equal(quoteArg("--bundles"), "--bundles");
  assert.equal(quoteArg("--config=src-tauri/tauri.conf.json"), "--config=src-tauri/tauri.conf.json");
  assert.equal(quoteArg("C:\\build\\tug"), "C:\\build\\tug");
  assert.equal(quoteArg("x86_64-pc-windows-msvc"), "x86_64-pc-windows-msvc");
});

test("spaces are quoted, and the empty argument survives", () => {
  assert.equal(quoteArg("C:\\Program Files\\tug"), '"C:\\Program Files\\tug"');
  assert.equal(quoteArg(""), '""');
});

test("trailing backslashes are doubled so they can't swallow the closing quote", () => {
  // Unescaped, `"C:\My Files\"` would read as `C:\My Files"` plus whatever came next.
  assert.equal(quoteArg("C:\\My Files\\"), '"C:\\My Files\\\\"');
  assert.equal(quoteArg("C:\\My Files\\\\"), '"C:\\My Files\\\\\\\\"');
  // Backslashes inside the argument stay as they are.
  assert.equal(quoteArg("a\\ b"), '"a\\ b"');
});

test("anything a shell could act on is refused, not passed on mangled", () => {
  for (const arg of ['say "hi"', "a & calc", "a|b", "<in", ">out", "%PATH%", "^", "!x!", "$HOME", "`id`", "(x)", "a;b", "line\nbreak"]) {
    assert.throws(() => quoteArg(arg), /can't pass/, arg);
  }
});
