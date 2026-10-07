// Ctrl+K actions: a few verbs typed into search, so common things become keystrokes.
// "text zoe running late", "pause", "copy code", "clear all", "new message". Pure, so it's tested.

import type { MediaCommand, PhoneNotification, SpotifyPlaylist } from "../types/protocol";
import { cleanName, formatAddress, missedCallFor } from "./format";
import { matchPlaylists, normalize, playlistDetail } from "./spotify";

export interface Person {
  name: string;
  address: string;
}

/** An app with notifications still in the feed, so "Show Gmail notifications" can scroll to it. */
export interface AppFeed {
  appId: string;
  label: string;
  /** The newest feed notification for the app, to scroll to and highlight. */
  focusId: number;
}

/**
 * Live context the one-off actions need to describe themselves exactly and run safely.
 * The parse stays pure: the store hands it what's waiting, so a row says what it will do.
 */
export interface ActionContext {
  /** The newest one-time code waiting, and the notifications it came in on. */
  code?: { code: string; from: PhoneNotification[] } | null;
  /** Notifications still on the phone that "clear all" would clear. */
  clearable?: PhoneNotification[];
  /** Whether Do not disturb is on right now. */
  doNotDisturb?: boolean;
  /** How many conversations have unread texts, for "mark all read". */
  unread?: number;
  /** Apps with notifications in the feed, to match a typed app name against. */
  apps?: AppFeed[];
  /** Calling from tug passed its hands-free check, so "call zoe" can dial. */
  canDial?: boolean;
  /** Notifications, to find a missed call "call zoe" can call back from without hands-free. */
  notifications?: PhoneNotification[];
  /** The user's Spotify playlists, for "play <name>" (empty/absent when not connected). */
  playlists?: SpotifyPlaylist[];
  /** Spotify is connected, so "play <song>", "queue <song>" and "spotify" are offered. */
  spotifyConnected?: boolean;
}

export type Action =
  | { kind: "send"; person: Person; text: string; label: string; detail: string }
  | { kind: "open-chat"; person: Person; label: string; detail: string }
  | { kind: "call"; person: Person; label: string; detail: string }
  /** "call zoe" before calling from tug has been checked: points to where to turn it on. */
  | { kind: "call-setup"; label: string }
  | { kind: "media"; command: MediaCommand; label: string }
  | { kind: "play-playlist"; uri: string; name: string; label: string; detail: string }
  /** "play <song>": search Spotify and play the best track match. */
  | { kind: "play-search"; query: string; label: string; detail: string }
  /** "queue <song>": search Spotify and queue the best track match. */
  | { kind: "queue-search"; query: string; label: string; detail: string }
  | { kind: "copy-code"; code: string | null; from: PhoneNotification[]; label: string }
  | { kind: "clear-all"; items: PhoneNotification[]; label: string }
  | { kind: "mark-all-read"; count: number; label: string }
  | { kind: "dnd"; enabled: boolean; label: string }
  | { kind: "show-app"; appId: string; focusId: number; label: string }
  | { kind: "open"; target: OpenTarget; label: string };

/** Panels and screens Ctrl+K can open by name. */
export type OpenTarget = "new-message" | "settings" | "spotify" | "tugboat";

const MEDIA: Array<[string[], MediaCommand, string]> = [
  [["play", "resume"], "play", "Play"],
  [["pause", "stop"], "pause", "Pause"],
  [["next", "skip", "next track"], "nextTrack", "Next track"],
  [["back", "previous", "prev", "previous track"], "previousTrack", "Previous track"],
  [["louder", "volume up"], "volumeUp", "Volume up"],
  [["quieter", "volume down"], "volumeDown", "Volume down"],
];

const OPEN: Array<[string[], OpenTarget, string]> = [
  [["new message", "new text", "compose"], "new-message", "New message"],
  [["settings", "preferences"], "settings", "Settings"],
  [["spotify", "music"], "spotify", "Open Spotify"],
  // Tugboat: files and text to and from the phone over Wi-Fi.
  [["tugboat", "tug boat", "drop", "send files", "send file", "send to phone", "send to iphone", "send to android"], "tugboat", "Tugboat"],
];

// Exact phrases only, so "clear" or "code" inside a longer search never fire a one-off action.
const COPY_CODE = ["copy code", "code"];
const CLEAR_ALL = ["clear all", "clear everything", "clear all notifications"];
const MARK_READ = ["mark all read", "mark all as read", "mark everything read"];
const DND = /^(?:do not disturb|dnd)(?:\s+(on|off))?$/i;

const SEND = /^(?:text|msg|message|tell|send)\s+(.+)$/i;
const CALL = /^(?:call|ring|phone)\s+(.+)$/i;
const PLAY = /^play\s+(.+)$/i;
const QUEUE = /^queue\s+(.+)$/i;

/** Lowercase, drop emoji/symbols, collapse spaces: "zoe 💜" → "zoe". */
function key(s: string): string {
  return cleanName(s)
    .toLowerCase()
    .replace(/[^\p{L}\p{N}' ]+/gu, " ")
    .replace(/\s+/g, " ")
    .trim();
}

/** The ways someone might be typed: full name, and first name. */
function keys(p: Person): string[] {
  const full = key(p.name);
  const first = full.split(" ")[0];
  return [...new Set([full, first].filter(Boolean))];
}

export function parseActions(query: string, people: Person[], ctx: ActionContext = {}): Action[] {
  const q = query.trim().replace(/\s+/g, " ");
  if (!q) return [];
  const lower = q.toLowerCase();
  const out: Action[] = [];

  for (const [words, command, label] of MEDIA) {
    if (words.includes(lower)) out.push({ kind: "media", command, label });
  }
  for (const [words, target, label] of OPEN) {
    if (!words.includes(lower)) continue;
    if (target === "spotify" && !ctx.spotifyConnected) continue;
    out.push({ kind: "open", target, label });
  }

  if (COPY_CODE.includes(lower)) out.push(copyCodeAction(ctx));
  if (CLEAR_ALL.includes(lower)) out.push(clearAllAction(ctx));
  if (MARK_READ.includes(lower)) out.push(markReadAction(ctx));
  const dnd = DND.exec(lower);
  if (dnd) out.push(dndAction(dnd[1], ctx));
  out.push(...appActions(q, ctx));

  const m = SEND.exec(q);
  if (m) out.push(...sendActions(m[1], people));
  const c = CALL.exec(q);
  if (c) out.push(...callActions(c[1], people, ctx));
  const pl = PLAY.exec(q);
  if (pl) out.push(...playActions(pl[1], ctx));
  const qu = QUEUE.exec(q);
  if (qu && ctx.spotifyConnected) {
    out.push({ kind: "queue-search", query: qu[1], label: `Queue “${qu[1]}”`, detail: "Add the top Spotify result to your queue" });
  }
  return out;
}

/**
 * "play deep focus": an exactly-named playlist plays as a playlist; otherwise search Spotify for the
 * best song match (when connected), with any fuzzy playlist matches offered below it.
 */
function playActions(rest: string, ctx: ActionContext): Action[] {
  const playlists = ctx.playlists ?? [];
  const matches = matchPlaylists(rest, playlists);
  const toPlaylist = (p: SpotifyPlaylist): Action => ({
    kind: "play-playlist",
    uri: p.uri,
    name: p.name,
    label: `Play ${p.name}`,
    detail: playlistDetail(p),
  });
  // An exact playlist name wins: just play that playlist.
  const exact = matches.filter((p) => normalize(p.name) === normalize(rest));
  if (exact.length) return exact.map(toPlaylist);
  const out: Action[] = [];
  if (ctx.spotifyConnected) {
    out.push({ kind: "play-search", query: rest, label: `Play “${rest}”`, detail: "Play the top song match on Spotify" });
  }
  out.push(...matches.map(toPlaylist));
  return out;
}

/**
 * "call zoe": the iPhone calls a person (or a typed number). Only the whole name counts,
 * nothing after it. Back from their missed call when the phone has one (no hands-free needed),
 * else by dialing once the hands-free check has passed; otherwise a row says why it can't.
 */
function callActions(rest: string, people: Person[], ctx: ActionContext): Action[] {
  const found = findPeople(rest, people).filter((m) => m.text === "");
  return found.map(({ person }): Action => {
    const name = cleanName(person.name);
    if (missedCallFor(ctx.notifications ?? [], person)) {
      return { kind: "call", person, label: `Call ${name} back`, detail: "From their missed call" };
    }
    if (ctx.canDial) return { kind: "call", person, label: `Call ${name}`, detail: formatAddress(person.address) };
    return { kind: "call-setup", label: `Can't call ${name} yet: turn on Settings › iPhone › Call from tug` };
  });
}

/** Copy the newest one-time code; with none waiting the row says so and does nothing. */
function copyCodeAction(ctx: ActionContext): Action {
  const found = ctx.code ?? null;
  return found
    ? { kind: "copy-code", code: found.code, from: found.from, label: `Copy code ${found.code}` }
    : { kind: "copy-code", code: null, from: [], label: "No recent code to copy" };
}

/** Clear every clearable notification on the phone; the row names the exact count. */
function clearAllAction(ctx: ActionContext): Action {
  const items = ctx.clearable ?? [];
  const label = items.length
    ? `Clear ${items.length} notification${items.length === 1 ? "" : "s"} on your iPhone`
    : "Nothing to clear on your iPhone";
  return { kind: "clear-all", items, label };
}

/** Mark every conversation seen in tug and its texts read on the phone. */
function markReadAction(ctx: ActionContext): Action {
  const count = ctx.unread ?? 0;
  const label = count
    ? `Mark ${count} conversation${count === 1 ? "" : "s"} read`
    : "Everything's already read";
  return { kind: "mark-all-read", count, label };
}

/** Toggle Do not disturb, or set it outright with "on"/"off"; the row shows the result. */
function dndAction(explicit: string | undefined, ctx: ActionContext): Action {
  const enabled = explicit ? explicit.toLowerCase() === "on" : !(ctx.doNotDisturb ?? false);
  return { kind: "dnd", enabled, label: `Turn ${enabled ? "on" : "off"} Do not disturb` };
}

/** Typing an app's name offers to jump the feed to its notifications. Only apps that have some. */
function appActions(query: string, ctx: ActionContext): Action[] {
  const qk = key(query);
  if (qk.length < 2) return []; // a single letter would match too much to be a choice
  return (ctx.apps ?? [])
    .filter((app) => key(app.label).startsWith(qk))
    .slice(0, 4)
    .map((app) => ({ kind: "show-app", appId: app.appId, focusId: app.focusId, label: `Show ${app.label} notifications` }));
}

function sendActions(rest: string, people: Person[]): Action[] {
  return findPeople(rest, people).map(({ person, text }) => build(person, text));
}

/** Who "rest" starts with (and what follows the name), or a number typed directly. */
function findPeople(rest: string, people: Person[]): Array<{ person: Person; text: string }> {
  const lowerRest = rest.toLowerCase();
  // A number typed directly: "text 302 555 0100 on my way".
  const num = /^(\+?[\d\s().-]{7,}\d)(?:\s+(.*))?$/.exec(rest);
  if (num) {
    const address = num[1].replace(/[^\d+]/g, "");
    const person = { name: formatAddress(address.length === 10 ? `+1${address}` : address), address };
    return [{ person, text: (num[2] ?? "").trim() }];
  }
  // The longest name that starts what was typed wins; ties (two "Jo"s) all show, so
  // the right person is picked on purpose, never guessed.
  let best = 0;
  let matches: Array<{ person: Person; text: string }> = [];
  for (const p of people) {
    for (const k of keys(p)) {
      if (lowerRest !== k && !lowerRest.startsWith(`${k} `)) continue;
      if (k.length < best) continue;
      const text = rest.slice(k.length).trim();
      if (k.length > best) {
        best = k.length;
        matches = [];
      }
      if (!matches.some((x) => x.person.address === p.address)) matches.push({ person: p, text });
    }
  }
  return matches.slice(0, 4);
}

function build(person: Person, text: string): Action {
  const detail = formatAddress(person.address);
  return text
    ? { kind: "send", person, text, label: `Send “${text}” to ${cleanName(person.name)}`, detail }
    : { kind: "open-chat", person, label: `Message ${cleanName(person.name)}`, detail };
}
