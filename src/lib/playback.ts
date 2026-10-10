// Where music is really playing, and what the song really is, when Spotify plays on another
// device (Spotify Connect). Pure, so it's unit-tested; the store and components just call it.
//
// On Spotify Connect the iPhone's Spotify app reports the track oddly over AMS (seen on a real
// iPhone, 2026-10-07): the title is "Song • Artist" and the artist is "Listening on <device>".
// tug never shows that hint as an artist, and uses one device name everywhere: Spotify's own.

import type { SpotifyDevice, SpotifyPlayer } from "../types/protocol";

const LISTENING_ON = /^\s*listening on\s+(.+?)\s*$/i;
const BULLET = " • ";

/** The device in a "Listening on <device>" artist line (Spotify Connect), else null. */
export function connectHint(artist: string | null | undefined): string | null {
  const m = artist ? LISTENING_ON.exec(artist) : null;
  return m && m[1] ? m[1] : null;
}

/**
 * Spotify Connect on an iPhone in any language: the English "Listening on" is matched directly;
 * otherwise (the hint is localized, e.g. "Écoute sur Cuisine") it's recognised by its shape: the
 * phone names Spotify as the player, the title is "Song • Artist", and the artist line isn't part
 * of the title, as a real artist would be. Mirrors `is_connect_line` in `src-tauri/src/media_keys.rs`.
 */
export function isConnectLine(np: { title: string | null; artist: string | null; player?: string | null }): boolean {
  if (connectHint(np.artist)) return true;
  if (!np.player || !/spotify/i.test(np.player)) return false;
  const title = np.title?.trim() ?? "";
  const artist = np.artist?.trim() ?? "";
  const at = title.lastIndexOf(BULLET);
  if (!artist || at <= 0 || !title.slice(at + BULLET.length).trim()) return false;
  return !title.toLowerCase().includes(artist.toLowerCase());
}

export interface TrackLines {
  title: string | null;
  /** The real artist, or null to hide the line (never the "Listening on" hint). */
  artist: string | null;
  album: string | null;
  /** The phone's "Listening on <device>" hint, when it gave one. */
  hint: string | null;
}

/**
 * The title and artist to show. On Spotify Connect the phone's artist is a device hint and its
 * title is "Song • Artist": Spotify's own snapshot wins when it describes this song, otherwise
 * the title is split at its last " • " (artist lists use commas, so a bullet in a song name stays
 * in the song name). Anything else is shown exactly as the phone sent it.
 */
export function trackLines(
  np: { title: string | null; artist: string | null; album: string | null; player?: string | null },
  spotify?: Pick<SpotifyPlayer, "trackName" | "trackArtists"> | null,
): TrackLines {
  // The device is only read from the English hint; a localized one still splits the title.
  const hint = connectHint(np.artist);
  if (!isConnectLine(np)) return { title: np.title, artist: np.artist, album: np.album, hint: null };
  const raw = np.title?.trim() ?? "";
  const name = spotify?.trackName?.trim();
  const artists = spotify?.trackArtists?.trim();
  if (name && artists) {
    const joined = `${name}${BULLET}${artists}`.toLowerCase();
    if (raw.toLowerCase() === joined || raw.toLowerCase() === name.toLowerCase()) {
      return { title: name, artist: artists, album: np.album, hint };
    }
  }
  const at = raw.lastIndexOf(BULLET);
  if (at > 0) {
    const artist = raw.slice(at + BULLET.length).trim();
    return { title: raw.slice(0, at).trim(), artist: artist || null, album: np.album, hint };
  }
  return { title: np.title, artist: null, album: np.album, hint };
}

const same = (a: string | null | undefined, b: string | null | undefined) =>
  !!a && !!b && a.trim().toLowerCase() === b.trim().toLowerCase();

const isPhone = (kind: string | null | undefined) => (kind ?? "").toLowerCase() === "smartphone";

/** A playback device as tug shows it: always Spotify's name for it. */
export interface PlaybackDevice {
  /** Spotify's device id; null when only the phone's hint names it. */
  id: string | null;
  name: string;
  kind: string;
}

/**
 * The device Spotify's API last said is playing (`/me/player/devices` `is_active`, or the
 * `/me/player` `device`), whichever was read last. Null when the API says nothing is active.
 */
export function activeFromDevices(devices: readonly SpotifyDevice[]): PlaybackDevice | null {
  const d = devices.find((x) => x.isActive);
  return d ? { id: d.id, name: d.name, kind: d.kind } : null;
}
export function activeFromPlayer(p: Pick<SpotifyPlayer, "deviceId" | "deviceName" | "deviceKind"> | null): PlaybackDevice | null {
  if (!p?.deviceName) return null;
  return { id: p.deviceId ?? null, name: p.deviceName, kind: p.deviceKind ?? "" };
}

/**
 * Where music is playing now. Spotify's API is the truth; when it hasn't said (not connected,
 * not read yet, or nothing active), the phone's "Listening on <device>" hint is the fallback,
 * matched to a listed device so it carries Spotify's id and spelling. Null: unknown.
 */
export function currentDevice(
  api: PlaybackDevice | null,
  hint: string | null,
  devices: readonly SpotifyDevice[] = [],
): PlaybackDevice | null {
  if (api) return api;
  if (!hint) return null;
  const listed = devices.find((d) => same(d.name, hint));
  return listed ? { id: listed.id, name: listed.name, kind: listed.kind } : { id: null, name: hint, kind: "" };
}

/** The phone among Spotify's devices: the smartphone named like the paired iPhone, else the only/first one. */
export function phoneDevice(devices: readonly SpotifyDevice[], bluetoothName: string | null | undefined): SpotifyDevice | null {
  const phones = devices.filter((d) => isPhone(d.kind));
  return phones.find((d) => same(d.name, bluetoothName)) ?? phones[0] ?? null;
}

export interface PickerRow {
  /** Null for the phone when Spotify doesn't list it (its app is closed): choosing it still moves music there. */
  id: string | null;
  name: string;
  kind: string;
  phone: boolean;
  current: boolean;
}

/** True when `d` is the current device (by id, or by name when the current one has no id). */
function isCurrent(d: { id: string | null; name: string }, current: PlaybackDevice | null): boolean {
  if (!current) return false;
  if (current.id && d.id) return current.id === d.id;
  return same(current.name, d.name);
}

/**
 * The "Play on" list: the iPhone first (under Spotify's name for it), then the other devices,
 * with "Current" on exactly the device that is playing. A current device Spotify didn't list
 * (known only from the phone's hint) is still shown, so the list never disagrees with the header.
 */
export function pickerRows(
  devices: readonly SpotifyDevice[],
  current: PlaybackDevice | null,
  bluetoothName: string | null | undefined,
): PickerRow[] {
  const phone = phoneDevice(devices, bluetoothName);
  const rows: PickerRow[] = [
    phone
      ? { id: phone.id, name: phone.name, kind: phone.kind, phone: true, current: false }
      : { id: null, name: "iPhone", kind: "Smartphone", phone: true, current: false },
  ];
  for (const d of devices) {
    if (isPhone(d.kind)) continue;
    rows.push({ id: d.id, name: d.name, kind: d.kind, phone: false, current: false });
  }
  const at = rows.findIndex((r) => isCurrent(r, current));
  if (at >= 0) rows[at].current = true;
  else if (current && isPhone(current.kind) && !phone) {
    // Spotify says a phone is playing but didn't list it: that's the iPhone row, under Spotify's name.
    rows[0] = { ...rows[0], id: current.id, name: current.name, current: true };
  } else if (current) {
    rows.push({ id: current.id, name: current.name, kind: current.kind, phone: false, current: true });
  }
  return rows;
}

/**
 * The header button's label: the device that is playing (the same row marked "Current"), else
 * where the next play will go (a chosen device, else the iPhone under Spotify's name for it).
 */
export function pickerLabel(rows: readonly PickerRow[], chosen: { name: string } | null): string {
  return rows.find((r) => r.current)?.name ?? chosen?.name ?? rows.find((r) => r.phone)?.name ?? "iPhone";
}

/** The "on <device>" line for Now Playing: shown when music plays somewhere other than the iPhone. */
export function playingOnLine(current: PlaybackDevice | null): string | null {
  if (!current || isPhone(current.kind)) return null;
  return `on ${current.name}`;
}
