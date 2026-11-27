// node --test extension/test (run by `npm run check`).
import test from "node:test";
import assert from "node:assert/strict";
import {
  ago,
  canReply,
  charCount,
  codeAge,
  codeSource,
  composeProblem,
  explainError,
  explainSendError,
  groupCode,
  heldNowPlaying,
  initial,
  isOldCode,
  matchContacts,
  nowPlaying,
  phoneLine,
  phoneNotice,
  sendOutcome,
  shortTime,
} from "../lib/format.js";
import { lastTextNotice, SHOW_RESULT_MS, WAITING_MAX_MS } from "../lib/texts.js";
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

test("a pressed play or pause shows at once, until the phone agrees", () => {
  const np = { title: "Harbour Lights", subtitle: "", playing: true };
  const hold = { kind: "toggle", playing: false, until: 1000 };
  // The phone still says playing: show paused, keep holding.
  assert.deepEqual(heldNowPlaying(np, hold, 500), { np: { ...np, playing: false }, pending: false, hold });
  // The phone caught up: let go.
  assert.deepEqual(heldNowPlaying({ ...np, playing: false }, hold, 600), { np: { ...np, playing: false }, pending: false, hold: null });
  // Too long: believe the phone.
  assert.deepEqual(heldNowPlaying(np, hold, 1001), { np, pending: false, hold: null });
  assert.deepEqual(heldNowPlaying(np, null, 0), { np, pending: false, hold: null });
});

test("a skip dims the old track until a new title arrives", () => {
  const np = { title: "Harbour Lights", subtitle: "", playing: true };
  const hold = { kind: "skip", title: "Harbour Lights", until: 1000 };
  assert.deepEqual(heldNowPlaying(np, hold, 100), { np, pending: true, hold });
  // Between tracks the phone may report nothing: keep what was shown.
  assert.deepEqual(heldNowPlaying(null, hold, 100, np), { np, pending: true, hold });
  const next = { ...np, title: "Low Tide" };
  assert.deepEqual(heldNowPlaying(next, hold, 200), { np: next, pending: false, hold: null });
  assert.deepEqual(heldNowPlaying(np, hold, 2000), { np, pending: false, hold: null });
});

test("characters are counted as tug counts them", () => {
  assert.equal(charCount("hi"), 2);
  assert.equal(charCount("👍🏽"), 2); // Two scalar values, four UTF-16 units.
  assert.equal(charCount(undefined), 0);
});

test("a text needs someone to go to and something to say, within tug's limits", () => {
  assert.equal(composeProblem("", "hi"), "Who's it to?");
  assert.equal(composeProblem("   ", "hi"), "Who's it to?");
  assert.equal(composeProblem("Maya", "  "), "Write a message first.");
  assert.equal(composeProblem("x".repeat(101), "hi"), "A name can be 100 characters at most.");
  assert.equal(composeProblem("Maya", "x".repeat(1001)), "A text can be 1000 characters at most.");
  assert.equal(composeProblem("Maya", "😀".repeat(1000)), null); // 2000 UTF-16 units, 1000 characters.
  assert.equal(composeProblem(" Maya ", "hi"), null);
});

test("only a text someone else sent can be replied to", () => {
  assert.equal(canReply({ kind: "text", from: "Maya Chen" }), true);
  assert.equal(canReply({ kind: "text", from: "You" }), false);
  assert.equal(canReply({ kind: "notification", from: "OpenTable" }), false);
  assert.equal(canReply({ kind: "text", from: " " }), false);
  assert.equal(canReply(null), false);
});

test("each way a text can end reads plainly, and none invites a second send when unsure", () => {
  assert.deepEqual(sendOutcome({ outcome: "sent", to: "Maya", detail: null }), { tone: "success", title: "Sent to Maya", body: "" });
  assert.equal(sendOutcome({ outcome: "queued", to: "Maya" }).tone, "warning");
  assert.match(sendOutcome({ outcome: "queued", to: "Maya" }).body, /don't send it again/);
  assert.equal(sendOutcome({ outcome: "declined", to: "Maya" }).body, "You chose Don't send for Maya.");
  assert.match(sendOutcome({ outcome: "timed_out", to: "Maya" }).body, /2 minutes/);
  assert.equal(sendOutcome({ outcome: "cancelled", to: "Maya" }).tone, "info");
  assert.equal(
    sendOutcome({ outcome: "failed", to: "Maya", detail: "Not delivered." }).body,
    "Your iPhone didn't take it (Not delivered). You can retry it from the conversation in tug.",
  );
  assert.equal(sendOutcome(null).title, "Not sure it was sent");
  assert.equal(sendOutcome({ outcome: "something_new" }).title, "Not sure it was sent");
});

test("a failed send says what to fix, and never to just send it again", () => {
  assert.equal(explainSendError("not_found", "").title, "No contact by that name");
  assert.equal(explainSendError("invalid", "More than one contact matches").body, "More than one contact matches");
  assert.equal(explainSendError("unavailable", "").title, "Can't send right now");
  assert.match(explainSendError("failed", "port closed").body, /check the conversation there/);
  assert.equal(explainSendError("not_running", "").screen, "closed");
  assert.equal(explainSendError("tool_off", "").title, "Switched off in tug");
});

test("the popup remembers the last text only as long as it helps", () => {
  const now = 10_000_000;
  assert.equal(lastTextNotice(null, now), null);
  assert.equal(lastTextNotice({ state: "waiting", to: "Maya" }, now), null); // No time: ignore.

  const waiting = lastTextNotice({ state: "waiting", to: "Maya", at: now - 1000 }, now);
  assert.equal(waiting.waiting, true);
  assert.equal(waiting.dismissable, false);
  assert.equal(waiting.icon, "loader-circle");

  // A wait that outlived any send: unsure, not "waiting" forever.
  const cutOff = lastTextNotice({ state: "waiting", to: "Maya", at: now - WAITING_MAX_MS - 1 }, now);
  assert.equal(cutOff.title, "Not sure it was sent");
  assert.equal(cutOff.waiting, false);
  assert.equal(lastTextNotice({ state: "waiting", to: "Maya", at: now - WAITING_MAX_MS - SHOW_RESULT_MS - 1 }, now), null);

  const sent = lastTextNotice({ state: "done", to: "Maya", at: now - 1000, result: { outcome: "sent", to: "Maya" } }, now);
  assert.equal(sent.title, "Sent to Maya");
  assert.equal(sent.icon, "check");
  assert.equal(sent.dismissable, true);
  assert.equal(lastTextNotice({ state: "done", to: "Maya", at: now - SHOW_RESULT_MS - 1, result: { outcome: "sent" } }, now), null);

  const failed = lastTextNotice({ state: "error", to: "Maya", at: now - 1000, error: { code: "not_found", message: "" } }, now);
  assert.equal(failed.tone, "error");
  assert.equal(failed.title, "No contact by that name");
});

test("the To field offers contacts: word starts first, then anything containing it, in tug's order", () => {
  const list = [
    { name: "Maya Chen", recent: true },
    { name: "Isaac Brown", recent: true },
    { name: "Sam Lee", recent: false },
    { name: "Sam Rivera", recent: false },
    { name: "Samantha Hill", recent: false },
  ];
  assert.deepEqual(matchContacts(list, ""), list, "nothing typed: everyone, recent first");
  assert.deepEqual(matchContacts(list, "  "), list);
  assert.deepEqual(
    matchContacts(list, "sa").map((c) => c.name),
    ["Sam Lee", "Sam Rivera", "Samantha Hill", "Isaac Brown"],
  );
  assert.deepEqual(matchContacts(list, "sam r").map((c) => c.name), ["Sam Rivera"]);
  assert.deepEqual(matchContacts(list, "CHEN").map((c) => c.name), ["Maya Chen"]);
  assert.deepEqual(matchContacts(list, "zz"), []);
  assert.deepEqual(matchContacts(null, "sa"), []);
});
