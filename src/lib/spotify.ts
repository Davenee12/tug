// Pure Spotify helpers used by the UI and the Ctrl+K palette: fuzzy-matching a typed name to a
// playlist ("play deep focus"), and the repeat-button cycle. No IPC here, so it's unit-tested.

import type { RepeatMode, SpotifyPlaylist } from "../types/protocol";

/** Lowercase, drop punctuation/emoji, collapse spaces: "Deep Focus 🎧" → "deep focus". */
export function normalize(s: string): string {
  return s
    .toLowerCase()
    .replace(/[^\p{L}\p{N} ]+/gu, " ")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * How well a typed query matches a playlist name (0 = no match, higher = better):
 * exact name, then a name that starts with the query, then a word that starts with it, then a
 * plain substring, then every query word matching some word's start (out of order).
 */
export function matchScore(query: string, name: string): number {
  const q = normalize(query);
  const n = normalize(name);
  if (!q || !n) return 0;
  if (n === q) return 100;
  if (n.startsWith(q)) return 80 - Math.min(20, n.length - q.length);
  const words = n.split(" ");
  if (words.some((w) => w.startsWith(q))) return 60;
  if (n.includes(q)) return 40;
  const qWords = q.split(" ");
  if (qWords.length > 1 && qWords.every((qw) => words.some((w) => w.startsWith(qw)))) return 30;
  return 0;
}

/** The playlists a typed name could mean, best first (ties keep input order). */
export function matchPlaylists(query: string, playlists: SpotifyPlaylist[], limit = 4): SpotifyPlaylist[] {
  return playlists
    .map((p, i) => ({ p, i, s: matchScore(query, p.name) }))
    .filter((x) => x.s > 0)
    .sort((a, b) => b.s - a.s || a.i - b.i)
    .slice(0, limit)
    .map((x) => x.p);
}

/** The next repeat mode when the repeat button is pressed (off → all → one → off). */
export function nextRepeat(mode: RepeatMode | null): RepeatMode {
  if (mode === "all") return "one";
  if (mode === "one") return "off";
  return "all";
}
