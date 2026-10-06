// Pure Spotify helpers used by the UI and the Ctrl+K palette: fuzzy-matching a typed name to a
// playlist ("play deep focus"), and the repeat-button cycle. No IPC here, so it's unit-tested.

import type { RepeatMode, SpotifyPlaylist, SpotifyTrack } from "../types/protocol";

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

/** A playlist's subtitle: owner and song count, leaving out a count Spotify didn't give. */
export function playlistDetail(p: Pick<SpotifyPlaylist, "owner" | "trackCount">): string {
  const songs = p.trackCount == null ? null : `${p.trackCount} song${p.trackCount === 1 ? "" : "s"}`;
  return [p.owner, songs].filter(Boolean).join(" · ");
}

/** The bare id in a `spotify:track:ID` / `spotify:album:ID` / `spotify:artist:ID` URI. */
export function idFromUri(uri: string | null | undefined): string | null {
  if (!uri) return null;
  const id = uri.split(":").pop();
  return id && id.length ? id : null;
}

/**
 * The best track for a typed "play …": an exact name match wins, then a name that starts with the
 * query, then Spotify's own top result. `null` when there are no tracks. Pure, so it's tested.
 */
export function bestTrack(query: string, tracks: SpotifyTrack[]): SpotifyTrack | null {
  if (tracks.length === 0) return null;
  const q = normalize(query);
  if (!q) return tracks[0];
  return (
    tracks.find((t) => normalize(t.name) === q) ??
    tracks.find((t) => normalize(`${t.name} ${t.artists}`) === q) ??
    tracks.find((t) => normalize(t.name).startsWith(q)) ??
    tracks[0]
  );
}

/** mm:ss from a duration in milliseconds (track lengths). */
export function trackLength(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

/**
 * The fraction (0–1) of the progress bar a pointer is at, for click/drag-to-seek. Clamped so a
 * drag past either end stays in range. Pure, so it's tested.
 */
export function seekFraction(clientX: number, rect: { left: number; width: number }): number {
  if (rect.width <= 0) return 0;
  return Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
}
