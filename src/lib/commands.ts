// Ctrl+K actions: a few verbs typed into search, so common things become keystrokes.
// "text tay running late", "pause", "next", "new message". Pure, so it's tested.

import type { MediaCommand } from "../types/protocol";
import { cleanName, formatAddress } from "./format";

export interface Person {
  name: string;
  address: string;
}

export type Action =
  | { kind: "send"; person: Person; text: string; label: string; detail: string }
  | { kind: "open-chat"; person: Person; label: string; detail: string }
  | { kind: "media"; command: MediaCommand; label: string }
  | { kind: "open"; target: "new-message" | "settings"; label: string };

const MEDIA: Array<[string[], MediaCommand, string]> = [
  [["play", "resume"], "play", "Play"],
  [["pause", "stop"], "pause", "Pause"],
  [["next", "skip", "next track"], "nextTrack", "Next track"],
  [["back", "previous", "prev", "previous track"], "previousTrack", "Previous track"],
  [["louder", "volume up"], "volumeUp", "Volume up"],
  [["quieter", "volume down"], "volumeDown", "Volume down"],
];

const OPEN: Array<[string[], "new-message" | "settings", string]> = [
  [["new message", "new text", "compose"], "new-message", "New message"],
  [["settings", "preferences"], "settings", "Settings"],
];

const SEND = /^(?:text|msg|message|tell|send)\s+(.+)$/i;

/** Lowercase, drop emoji/symbols, collapse spaces: "tay 🤎" → "tay". */
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

export function parseActions(query: string, people: Person[]): Action[] {
  const q = query.trim().replace(/\s+/g, " ");
  if (!q) return [];
  const lower = q.toLowerCase();
  const out: Action[] = [];

  for (const [words, command, label] of MEDIA) {
    if (words.includes(lower)) out.push({ kind: "media", command, label });
  }
  for (const [words, target, label] of OPEN) {
    if (words.includes(lower)) out.push({ kind: "open", target, label });
  }

  const m = SEND.exec(q);
  if (m) out.push(...sendActions(m[1], people));
  return out;
}

function sendActions(rest: string, people: Person[]): Action[] {
  const lowerRest = rest.toLowerCase();
  // A number typed directly: "text 302 555 0100 on my way".
  const num = /^(\+?[\d\s().-]{7,}\d)(?:\s+(.*))?$/.exec(rest);
  if (num) {
    const address = num[1].replace(/[^\d+]/g, "");
    const person = { name: formatAddress(address.length === 10 ? `+1${address}` : address), address };
    return [build(person, (num[2] ?? "").trim())];
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
  return matches.slice(0, 4).map(({ person, text }) => build(person, text));
}

function build(person: Person, text: string): Action {
  const detail = formatAddress(person.address);
  return text
    ? { kind: "send", person, text, label: `Send “${text}” to ${cleanName(person.name)}`, detail }
    : { kind: "open-chat", person, label: `Message ${cleanName(person.name)}`, detail };
}
