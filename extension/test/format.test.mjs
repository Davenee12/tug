// node --test extension/test (run by `npm run check`).
import test from "node:test";
import assert from "node:assert/strict";
import { ago, codeAge, codeSource, explainError, groupCode, initial, isOldCode, nowPlaying, phoneLine, phoneNotice, shortTime } from "../lib/format.js";
import { hostError } from "../lib/tug.js";

test("codes are grouped the way people read them", () => {
  assert.equal(groupCode("482913"), "482 913");
  assert.equal(groupCode("12345678"), "1234 5678");
  assert.equal(groupCode("4821"), "4821");
  assert.equal(groupCode("G-123456"), "G-12 3456");
  assert.equal(groupCode(" 482913 "), "482 913");
  assert.equal(groupCode(undefined), "");
});

test("ages read naturally", () => {
  assert.equal(ago(10), "just now");
  assert.equal(ago(120), "2 min ago");
  assert.equal(ago(3600), "1 hr ago");
  assert.equal(ago(-5), "just now");
  assert.equal(isOldCode(299), false);
  assert.equal(isOldCode(300), true);
});

test("a code's source drops the app when it's the sender", () => {
  assert.equal(codeSource("Chase", "Messages"), "Chase · Messages");
  assert.equal(codeSource("Uber", "uber"), "Uber");
  assert.equal(codeSource("", "Messages"), "Messages");
});

test("search times follow the Feed", () => {
  const now = new Date(2026, 10, 26, 18, 0);
  assert.equal(shortTime(new Date(2026, 10, 25, 9, 0).toISOString(), now), "Yesterday");
  assert.equal(shortTime(new Date(2026, 10, 22, 9, 0).toISOString(), now, "en-US"), "Sun");
  assert.equal(shortTime(new Date(2026, 9, 3, 9, 0).toISOString(), now, "en-US"), "Oct 3");
  assert.match(shortTime(new Date(2026, 10, 26, 9, 5).toISOString(), now, "en-US"), /9:05/);
  assert.equal(shortTime("not a time", now), "");
});

test("the phone line covers every connection state", () => {
  const p = { connection: "connected", model: "iPhone 15 Pro", battery_percent: 64 };
  assert.deepEqual(phoneLine(p), { label: "iPhone 15 Pro", state: "connected", detail: "64%" });
  assert.equal(phoneLine({ ...p, connection: "connecting" }).detail, "Connecting…");
  assert.equal(phoneLine({ ...p, connection: "no_phone" }).state, "none");
  assert.equal(phoneLine({ connection: "disconnected" }).label, "iPhone");
  assert.equal(phoneLine(null).state, "unknown");
});

test("now playing hides when there's nothing to control", () => {
  const np = (o) => nowPlaying({ now_playing: { title: "Song", artist: "Band", state: "playing", app: "Spotify", ...o } });
  assert.deepEqual(np({}), { title: "Song", subtitle: "Band · Spotify", playing: true });
  assert.equal(np({ state: "stopped" }), null);
  assert.equal(np({ title: null, artist: null }), null);
  assert.equal(nowPlaying({}), null);
});

test("errors that block everything become a whole-popup screen", () => {
  for (const code of ["not_installed", "wrong_extension", "not_running", "off", "not_set_up"]) {
    assert.ok(explainError(code).screen, code);
  }
  for (const code of ["tool_off", "unavailable", "not_found", "rate_limited", "busy", "timeout", "failed", "whatever"]) {
    assert.equal(explainError(code).screen, undefined, code);
    assert.ok(explainError(code).title, code);
  }
  assert.equal(explainError("tool_off", "Codes is off.").body, "Codes is off.");
});

test("the browser's own errors are told apart: no host, another extension ID, anything else", () => {
  assert.equal(hostError("Specified native messaging host not found.").code, "not_installed");
  assert.equal(hostError("Access to the specified native messaging host is forbidden.").code, "wrong_extension");
  assert.equal(hostError("Native host has exited.").code, "failed");
  assert.equal(hostError("Native host has exited.").message, "Native host has exited.");
});

test("a missing host doesn't claim tug isn't installed: an older tug looks the same", () => {
  const why = explainError("not_installed");
  assert.equal(why.screen, "install");
  assert.doesNotMatch(`${why.title} ${why.body}`, /isn't on this PC|not installed/i);
  assert.match(why.body, /update it if you already have it/);
  assert.match(explainError("wrong_extension").body, /Update tug/);
});

test("an old code says it may have expired", () => {
  assert.equal(codeAge(120), "2 min ago");
  assert.equal(codeAge(7 * 60), "7 min ago · may have expired");
});

test("avatars take the first letter or digit", () => {
  assert.equal(initial("Chase"), "C");
  assert.equal(initial("élodie"), "É");
  assert.equal(initial("+1 555-0123"), "1");
  assert.equal(initial("💬 Group"), "G");
  assert.equal(initial(""), "#");
  assert.equal(initial(undefined), "#");
});

test("a notice shows only when the phone needs attention", () => {
  const p = { model: "iPhone 15 Pro" };
  assert.equal(phoneNotice({ ...p, connection: "connected" }), null);
  assert.equal(phoneNotice(null), null);
  assert.equal(phoneNotice({ ...p, connection: "disconnected" }).title, "iPhone 15 Pro isn't connected");
  assert.equal(phoneNotice({ ...p, connection: "disconnected" }).tone, "warning");
  assert.equal(phoneNotice({ connection: "connecting" }).title, "Connecting to your iPhone…");
  assert.equal(phoneNotice({ connection: "no_phone" }).title, "No iPhone set up yet");
});
