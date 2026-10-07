import { describe, expect, it } from "vitest";
import { parseActions, type ActionContext, type Person } from "./commands";
import type { PhoneNotification } from "../types/protocol";

const people: Person[] = [
  { name: "zoe 💜", address: "+13025550173" },
  { name: "Priya", address: "+12145550186" },
  { name: "Jo Smith", address: "+15550000001" },
  { name: "Jo Brown", address: "+15550000002" },
  { name: "Mary Ann", address: "+15550000003" },
  { name: "Mary", address: "+15550000004" },
];

// Shape doesn't matter for the parse: it only counts clearable and carries them through.
const noteIds = (n: number): PhoneNotification[] =>
  Array.from({ length: n }, (_, i) => ({ id: i }) as unknown as PhoneNotification);

const ctx: ActionContext = {
  code: { code: "591204", from: noteIds(1) },
  clearable: noteIds(7),
  doNotDisturb: false,
  unread: 3,
  apps: [
    { appId: "com.google.Gmail", label: "Gmail", focusId: 42 },
    { appId: "com.linkedin.LinkedIn", label: "LinkedIn", focusId: 43 },
  ],
};

describe("parseActions", () => {
  it("sends a text to someone by name, emoji and all", () => {
    const [a] = parseActions("text zoe running late", people);
    expect(a).toMatchObject({ kind: "send", text: "running late", person: { address: "+13025550173" } });
    expect(a.label).toBe("Send “running late” to zoe 💜");
  });

  it("accepts a few verbs and keeps the message as typed", () => {
    expect(parseActions("tell Priya On my way!", people)[0]).toMatchObject({ text: "On my way!" });
    expect(parseActions("msg priya ok", people)[0]).toMatchObject({ person: { name: "Priya" } });
  });

  it("offers every person a name could mean instead of guessing", () => {
    const actions = parseActions("text jo see you at 5", people);
    expect(actions.map((a) => (a.kind === "send" ? a.person.address : ""))).toEqual(["+15550000001", "+15550000002"]);
    // A full name settles it.
    expect(parseActions("text jo brown see you", people)).toHaveLength(1);
  });

  it("prefers the longest name that fits", () => {
    const [a] = parseActions("text mary ann lunch?", people);
    expect(a).toMatchObject({ person: { name: "Mary Ann" }, text: "lunch?" });
    const mary = parseActions("text mary lunch?", people);
    expect(mary.map((x) => (x.kind === "send" ? x.person.name : ""))).toEqual(["Mary Ann", "Mary"]);
  });

  it("opens the conversation when there's no message yet", () => {
    expect(parseActions("text zoe", people)[0]).toMatchObject({ kind: "open-chat", label: "Message zoe 💜" });
  });

  it("takes a phone number directly", () => {
    expect(parseActions("text 302 555 0100 on my way", people)[0]).toMatchObject({
      kind: "send",
      person: { address: "3025550100", name: "(302) 555-0100" },
      text: "on my way",
    });
  });

  it("knows the media and navigation verbs", () => {
    expect(parseActions("pause", people)).toEqual([{ kind: "media", command: "pause", label: "Pause" }]);
    expect(parseActions("Next", people)[0]).toMatchObject({ command: "nextTrack" });
    expect(parseActions("new message", people)[0]).toMatchObject({ kind: "open", target: "new-message" });
  });

  it("stays out of the way of ordinary searches", () => {
    expect(parseActions("dinner", people)).toEqual([]);
    expect(parseActions("text", people)).toEqual([]);
    expect(parseActions("text nobody hello", people)).toEqual([]);
    expect(parseActions("", people)).toEqual([]);
  });

  it("copies the newest one-time code", () => {
    expect(parseActions("copy code", people, ctx)[0]).toMatchObject({ kind: "copy-code", code: "591204" });
    expect(parseActions("code", people, ctx)[0]).toMatchObject({ kind: "copy-code", code: "591204", label: "Copy code 591204" });
    expect(parseActions("copy code", people, ctx)[0]).toMatchObject({ from: [{ id: 0 }] });
  });

  it("still offers copy code when there's none, but harmlessly", () => {
    const [a] = parseActions("code", people, { ...ctx, code: null });
    expect(a).toMatchObject({ kind: "copy-code", code: null, label: "No recent code to copy" });
  });

  it("doesn't mistake a longer search for the copy-code verb", () => {
    expect(parseActions("zip code 90210", people, ctx)).toEqual([]);
    expect(parseActions("promo code", people, ctx)).toEqual([]);
    expect(parseActions("decode this", people, ctx)).toEqual([]);
  });

  it("clears everything, saying exactly how many", () => {
    const [a] = parseActions("clear all", people, ctx);
    expect(a).toMatchObject({ kind: "clear-all", label: "Clear 7 notifications on your iPhone" });
    expect(a.kind === "clear-all" && a.items).toHaveLength(7);
    expect(parseActions("clear all", people, { ...ctx, clearable: noteIds(1) })[0].label).toBe(
      "Clear 1 notification on your iPhone",
    );
    expect(parseActions("clear all", people, { ...ctx, clearable: [] })[0].label).toBe("Nothing to clear on your iPhone");
  });

  it("never clears on a partial match", () => {
    expect(parseActions("clear", people, ctx)).toEqual([]);
    expect(parseActions("clear the table", people, ctx)).toEqual([]);
  });

  it("marks everything read, stating the count", () => {
    expect(parseActions("mark all read", people, ctx)[0]).toMatchObject({ kind: "mark-all-read", count: 3, label: "Mark 3 conversations read" });
    expect(parseActions("mark all as read", people, { ...ctx, unread: 1 })[0].label).toBe("Mark 1 conversation read");
    expect(parseActions("mark all read", people, { ...ctx, unread: 0 })[0].label).toBe("Everything's already read");
    expect(parseActions("mark", people, ctx)).toEqual([]);
  });

  it("toggles do not disturb and shows the resulting state", () => {
    expect(parseActions("dnd", people, ctx)[0]).toMatchObject({ kind: "dnd", enabled: true, label: "Turn on Do not disturb" });
    expect(parseActions("do not disturb", people, ctx)[0]).toMatchObject({ enabled: true });
    expect(parseActions("dnd", people, { ...ctx, doNotDisturb: true })[0]).toMatchObject({ enabled: false, label: "Turn off Do not disturb" });
    expect(parseActions("dnd on", people, { ...ctx, doNotDisturb: true })[0]).toMatchObject({ enabled: true });
    expect(parseActions("do not disturb off", people, ctx)[0]).toMatchObject({ enabled: false });
    expect(parseActions("dndx", people, ctx)).toEqual([]);
  });

  it("jumps the feed to an app's notifications, matched on its name", () => {
    expect(parseActions("gmail", people, ctx)[0]).toMatchObject({
      kind: "show-app",
      appId: "com.google.Gmail",
      focusId: 42,
      label: "Show Gmail notifications",
    });
    expect(parseActions("linked", people, ctx)[0]).toMatchObject({ appId: "com.linkedin.LinkedIn" });
  });

  it("only offers apps that actually have notifications, and not inside a longer search", () => {
    expect(parseActions("facebook", people, ctx)).toEqual([]);
    expect(parseActions("gmail is down", people, ctx)).toEqual([]);
    expect(parseActions("gmail", people, {})).toEqual([]);
    // A single letter would match too much to be a real choice.
    expect(parseActions("g", people, ctx)).toEqual([]);
  });
});

describe("call", () => {
  const dial = { canDial: true };

  it("calls a person by their whole name, once calling from tug works", () => {
    expect(parseActions("call priya", people, dial)).toEqual([
      { kind: "call", person: people[1], label: "Call Priya", detail: "(214) 555-0186" },
    ]);
    expect(parseActions("ring zoe", people, dial)[0]).toMatchObject({ kind: "call", label: "Call zoe 💜" });
  });

  it("offers each match for a shared name, so one is picked on purpose", () => {
    const rows = parseActions("call jo", people, dial);
    expect(rows.map((r) => r.kind)).toEqual(["call", "call"]);
  });

  it("dials a typed number", () => {
    expect(parseActions("call 302 555 0100", people, dial)[0]).toMatchObject({ kind: "call", person: { address: "3025550100" } });
  });

  it("says why instead of offering a call that can't work", () => {
    expect(parseActions("call priya", people, {})).toEqual([
      { kind: "call-setup", label: "Can't call Priya yet: only missed calls can be called back" },
    ]);
  });

  it("calls back from a missed call without hands-free", () => {
    const missed = {
      id: 9,
      category: "missedCall",
      title: "Priya",
      live: true,
      removedAt: null,
      receivedAt: 1,
      flags: { positiveAction: true },
    } as unknown as PhoneNotification;
    expect(parseActions("call priya", people, { notifications: [missed] })).toEqual([
      { kind: "call", person: people[1], label: "Call Priya back", detail: "From their missed call" },
    ]);
  });

  it("never fires on a near miss", () => {
    expect(parseActions("call priya later today", people, dial)).toEqual([]);
    expect(parseActions("call nobody", people, dial)).toEqual([]);
    expect(parseActions("callback", people, dial)).toEqual([]);
  });

  it("plays a Spotify playlist by fuzzy name, and 'play' alone is still media", () => {
    const playlists = [
      { uri: "spotify:playlist:1", id: "1", name: "Deep Focus", owner: "Spotify", trackCount: 120, imageUrl: null, owned: false },
      { uri: "spotify:playlist:2", id: "2", name: "Morning Run", owner: "Jordan", trackCount: 42, imageUrl: null, owned: true },
    ];
    // Not connected: a fuzzy playlist match plays the playlist; no song-search row.
    expect(parseActions("play deep", people, { playlists })).toEqual([
      { kind: "play-playlist", uri: "spotify:playlist:1", name: "Deep Focus", label: "Play Deep Focus", detail: "Spotify · 120 songs" },
    ]);
    // Bare "play" is the media play action, not a playlist search.
    expect(parseActions("play", people, { playlists })).toEqual([{ kind: "media", command: "play", label: "Play" }]);
    // No match, and no playlists loaded, yield nothing.
    expect(parseActions("play nothingmatches", people, { playlists })).toEqual([]);
    expect(parseActions("play deep", people, {})).toEqual([]);
  });
});

describe("spotify search actions", () => {
  const playlists = [
    { uri: "spotify:playlist:1", id: "1", name: "Deep Focus", owner: "Spotify", trackCount: 120, imageUrl: null, owned: false },
  ];
  const on = { spotifyConnected: true, playlists };

  it("searches and plays the best song when connected, playlists below", () => {
    const rows = parseActions("play teardrop", people, on);
    expect(rows[0]).toEqual({ kind: "play-search", query: "teardrop", label: "Play “teardrop”", detail: "Play the top song match on Spotify" });
  });

  it("offers both a song search and the fuzzy playlist for a partial name", () => {
    const rows = parseActions("play deep", people, on);
    expect(rows.map((r) => r.kind)).toEqual(["play-search", "play-playlist"]);
  });

  it("an exactly-named playlist still wins over a song search", () => {
    expect(parseActions("play deep focus", people, on)).toEqual([
      { kind: "play-playlist", uri: "spotify:playlist:1", name: "Deep Focus", label: "Play Deep Focus", detail: "Spotify · 120 songs" },
    ]);
  });

  it("queues the best song match, only when connected", () => {
    expect(parseActions("queue teardrop", people, on)[0]).toMatchObject({ kind: "queue-search", query: "teardrop" });
    expect(parseActions("queue teardrop", people, { playlists })).toEqual([]);
  });

  it("opens the Spotify panel, only when connected", () => {
    expect(parseActions("spotify", people, on)[0]).toMatchObject({ kind: "open", target: "spotify" });
    expect(parseActions("spotify", people, {})).toEqual([]);
  });
});
