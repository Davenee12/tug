// What's new: the short, end-user release notes tug shows once after it updates, and the pure
// logic for deciding which entries to show. This is NOT the developer CHANGELOG.md (PR numbers,
// caveats, known limits) — it's a handful of friendly lines per release, written for the person
// using tug.
//
// ⚠️ EVERY RELEASE: add an entry here, newest first, and keep the top version in step with
//    package.json / tauri.conf.json. An entry whose version is newer than the installed app is
//    held back until the app actually reaches it, so it's fine to add 0.5.9's lines before 0.5.9
//    ships. See the comment in CLAUDE.md too.

/** One release's notes, as shown in the "What's new" card. */
export interface ReleaseNote {
  /** Dotted version, e.g. "0.5.8" — must match the app version it ships in. */
  version: string;
  /** One friendly line summarising the release. */
  title: string;
  /** 3–6 short bullets, each a thing the person can now do. */
  highlights: string[];
}

/** Newest first. The first entry's version is the one that ships next. */
export const RELEASE_NOTES: ReleaseNote[] = [
  {
    version: "0.5.10",
    title: "Steadier connection and a round of fixes",
    highlights: [
      "tug reconnects by itself after your PC wakes up, and shows when your iPhone needs unlocking.",
      "Notifications stay put through a quick Bluetooth hiccup.",
      "Spotify keeps the right song's cover and Like, and asks Spotify far less often.",
      "The sidebar fits every music control on laptop screens.",
      "Smaller fixes: the Calls list stays put, codes pop up once, contact names stay right.",
    ],
  },
  {
    version: "0.5.9",
    title: "Easier connecting, quiet hours, and a lot more Spotify",
    highlights: [
      "Connect your iPhone on one screen that waits for each switch to come on.",
      "Spotify: search and play, line up a queue, browse albums and artists, scrub the track, and pick where it plays.",
      "Quiet hours, mute noisy apps, and let favourite people always get through.",
      "Your contacts' photos show up in Messages, Calls and the Feed.",
      "Skip 15 seconds and like songs in Apple Music, right from Now Playing.",
      "The low-battery alert now pops up, and the sidebar never scrolls.",
    ],
  },
  {
    version: "0.5.8",
    title: "Smoother setup, instant texts, and Spotify",
    highlights: [
      "Setup finds your iPhone and pairs it, no extra apps needed.",
      "Texts arrive the moment your phone gets them.",
      "Verification codes from texts show right in the Feed, ready to copy.",
      "Connect Spotify under Settings › Connectors for your playlists on your iPhone.",
      "Tidying up your notifications never ends a call.",
    ],
  },
];

/** Parse a dotted version into numbers; non-numeric parts become 0. */
function parts(version: string): number[] {
  return version.split(".").map((p) => {
    const n = Number.parseInt(p, 10);
    return Number.isNaN(n) ? 0 : n;
  });
}

/** True for a string that looks like a dotted version (at least one number). */
export function isVersion(value: string | null | undefined): value is string {
  return typeof value === "string" && /^\d+(\.\d+)*$/.test(value.trim());
}

/**
 * Compare two dotted versions numerically. Returns a negative number when `a` is older than `b`,
 * 0 when they're equal, a positive number when `a` is newer. Missing trailing parts count as 0,
 * so "0.5" === "0.5.0" and "0.5.9" > "0.5.8".
 */
export function compareVersions(a: string, b: string): number {
  const pa = parts(a);
  const pb = parts(b);
  const len = Math.max(pa.length, pb.length);
  for (let i = 0; i < len; i++) {
    const diff = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

/** Release notes at or below `current`, newest first (what the About link reopens). */
export function notesUpTo(notes: ReleaseNote[], current: string): ReleaseNote[] {
  if (!isVersion(current)) return [...notes].sort((a, b) => compareVersions(b.version, a.version));
  return notes
    .filter((n) => compareVersions(n.version, current) <= 0)
    .sort((a, b) => compareVersions(b.version, a.version));
}

/**
 * Which notes to show on launch, newest first: entries at or below the running version and newer
 * than the last version the user saw. Empty when there's nothing new to show — the user has
 * already seen this version (or a newer one), the version can't be read, or `lastSeen` is null (a
 * fresh install, which the caller records silently rather than greeting with a card).
 *
 * When several versions were skipped it returns them all newest-first, so the card can show the
 * newest prominently and collapse the rest.
 */
export function whatsNewToShow(notes: ReleaseNote[], current: string | null, lastSeen: string | null): ReleaseNote[] {
  if (!isVersion(current) || lastSeen === null) return [];
  // A stored value that isn't a version (corruption) is treated as "nothing seen yet": show
  // everything up to the current version rather than silently skipping the card forever.
  const seen = isVersion(lastSeen) ? lastSeen : null;
  return notesUpTo(notes, current).filter((n) => seen === null || compareVersions(n.version, seen) > 0);
}
