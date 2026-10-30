// What's new: the short, end-user release notes tug shows once after it updates, and the pure
// logic for deciding which entries to show. This is NOT the developer CHANGELOG.md (PR numbers,
// caveats, known limits) — it's a handful of friendly lines per release, written for the person
// using tug.
//
// ⚠️ EVERY RELEASE: add an entry here, newest first, and keep the top version in step with
//    package.json / tauri.conf.json. An entry whose version is newer than the installed app is
//    held back until the app actually reaches it, so it's fine to add 0.5.9's lines before 0.5.9
//    ships. See CONTRIBUTING.md too.

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
    version: "0.5.23",
    title: "Quick text, Focus mode and calls that pause your music",
    highlights: [
      "Quick text: turn it on in Settings › Messages, press Ctrl+Shift+Y in any app, type, Enter, and you're back where you were.",
      "Focus mode holds pop-ups while you're presenting, gaming or in a meeting, then sums up what came.",
      "When your iPhone rings, music and videos on your PC pause, and carry on if you miss the call.",
      "Rain soon: a pop-up about 15 minutes before it rains where you are (Settings › Weather).",
      "Voice typing gets a Most accurate engine, your own words, and hints for the app you're in.",
    ],
  },
  {
    version: "0.5.22",
    title: "A live harbor, Send later and Remind me",
    highlights: [
      "The weather card opens on a live harbor: the real sun, moon and wind, rain to the quarter-hour, and your new texts floating by in bottles.",
      "With rain on the way and your phone low, the weather card tells you to charge it before you head out.",
      "Send later: write a text now and pick when tug sends it through your iPhone.",
      "Remind me: put a text or notification aside and have it come back later.",
      "Links, numbers, addresses and parcel numbers in texts are one click, and a Welcome back card sums up what you missed.",
      "Report a problem from Settings › About, and updates now roll out gradually.",
    ],
  },
  {
    version: "0.5.21",
    title: "Texts from the terminal, and voice typing fixes",
    highlights: [
      "Turn on Send tug text without asking (Settings › Developer tools) and tug text sends straight away, no click.",
      "Voice typing no longer types made-up words after a cough or a mumble.",
      "Fixes for voice typing's switch and speech model download.",
    ],
  },
  {
    version: "0.5.20",
    title: "Faster voice typing, and fixes",
    highlights: [
      "Voice typing's Accurate engine is about 12 times faster: a short phrase is typed in about a quarter of a second.",
      "Ctrl+Shift+V waits for you to let go of the keys before it types a code.",
      "App icons and a few other small fixes.",
    ],
  },
  {
    version: "0.5.19",
    title: "Accurate voice typing, still on your PC",
    highlights: [
      "Voice typing gets an Accurate engine: far better with code words and short commands.",
      "It's a one-time 78 MB download you start in Settings › General › Voice typing. Your voice never leaves your PC.",
      "Windows' own recognizer stays as Basic, for other languages or if you'd rather not download.",
      "Turning voice typing on no longer closes tug.",
    ],
  },
  {
    version: "0.5.18",
    title: "Your whole message history, album art and a new game",
    highlights: [
      "Bring in your full message history, texts you sent and photos included, from a backup of your iPhone on this PC.",
      "Album art and colours on the music card.",
      "Messages now holds only texts you can reply to; other chat apps stay in the Feed.",
      "Harbor Stack: a new stacking game, with your iPhone as a sideways controller.",
      "Send files to your iPhone from the command line or AI tools, always with your OK.",
    ],
  },
  {
    version: "0.5.17",
    title: "Faster texts, a shared clipboard and privacy while you share",
    highlights: [
      "Texts send right away, and pop-ups offer one-tap smart replies.",
      "While you share your screen or present, previews stay hidden until you say otherwise.",
      "Send your PC's clipboard to your iPhone, and paste from your phone to your PC.",
      "Voice typing (off until you turn it on), taskbar shortcuts, and Tugboat Run's daily course and two-player races.",
      "Texts you send from your iPhone itself still don't show in tug: that's iOS, not you.",
    ],
  },
  {
    version: "0.5.16",
    title: "Play, listen and sign in faster",
    highlights: [
      "Tugboat Run: a quick harbor game, and your iPhone can be the controller.",
      "Play your iPhone's audio through this PC's speakers.",
      "A volume bar for your phone on the music card.",
      "Verification codes copy themselves, and Ctrl+Shift+V types the latest one.",
      "Stronger security checks behind the scenes.",
    ],
  },
  {
    version: "0.5.14",
    title: "A little more life",
    highlights: [
      "A greeting at the top of your Feed that matches the time of day and the weather.",
      "The rope logo gives a little tug whenever your iPhone connects.",
      "A couple of small surprises for the curious.",
    ],
  },
  {
    version: "0.5.13",
    title: "Faster and steadier",
    highlights: [
      "tug is much lighter on your PC: almost no CPU while it sits there or hides in the tray.",
      "If your iPhone restarts, tug tells you to unlock it and reconnects by itself.",
      "Texts reconnect within seconds when your phone drops them.",
      "tug boat finds the file you meant anywhere on your PC.",
      "Works better with iPhones set to other languages and in other time zones.",
    ],
  },
  {
    version: "0.5.12",
    title: "tug for developers, and a steadier tug",
    highlights: [
      "tug for developers: let AI tools like Claude Code and Cursor use your phone, or type tug in a terminal. Off until you turn it on in Settings › Developer tools, and a text only sends when you click Send.",
      "Steadier connection: one calm status while your iPhone is away, and quicker recovery after a Bluetooth hiccup or waking your PC.",
      "Texts you can trust: replies always show in the conversation, Retry never sends twice, and nothing sits on “Sending…”.",
      "Texts that arrive while tug reconnects still pop up, and the tray icon shows when you have unread texts.",
      "Sync Contacts shows Off when it's off, and Spotify shows the real song, artist and speaker.",
    ],
  },
  {
    version: "0.5.11",
    title: "Meet Tugboat",
    highlights: [
      "Tugboat: scan a code with your phone and send photos, files and text to your PC, and back. No app needed.",
      "Your exact iPhone now shows in the sidebar.",
      "One conversation per person, even when their name arrives slightly different.",
      "Spotify's back and forward 15 seconds jump to the right spot.",
      "Weather keeps itself up to date.",
    ],
  },
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
