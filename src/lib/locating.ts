// What tug says while it finds you for the weather ("Use my location"). Short, a little odd,
// never a progress bar pretending to be clever. Pure, so the sequence and the wording are tested.

/** Lines with the tug in them: one opens every search, then one roughly every third line. */
export const TUG_LINES = [
  "Giving the map a little tug…",
  "Tugging you into place…",
  "Pulling your location into view…",
  "Tugging on the map…",
  "Just a little tug…",
] as const;

export const LINES = [
  "Reading the room…",
  "Finding your corner of the map…",
  "Following your digital footprints…",
  "Putting a pin on you…",
  "Figuring out your side of town…",
  "Getting our bearings…",
  "Finding where you landed…",
  "Locating your neck of the woods…",
  "Finding your little dot…",
  "Putting you on the map…",
  "Finding your slice of Earth…",
  "Putting a name to your dot…",
] as const;

export const FOUND = "There you are. 📍";

/** Each line stays up this long: long enough to read, not so short it feels frantic. */
export const LINE_MS = 1600;
/** "There you are" stays this long before the forecast takes over. */
export const FOUND_MS = 1100;

function shuffled<T>(items: readonly T[], random: () => number): T[] {
  const out = [...items];
  for (let i = out.length - 1; i > 0; i--) {
    const j = Math.floor(random() * (i + 1));
    [out[i], out[j]] = [out[j], out[i]];
  }
  return out;
}

/**
 * The lines for one search, in order: a tug line first, then the rest shuffled with a tug line
 * every third, no line twice. Long enough for any search; it ends on the last line.
 */
export function locatingLines(random: () => number = Math.random): string[] {
  const tug: string[] = shuffled(TUG_LINES, random);
  const rest: string[] = shuffled(LINES, random);
  const out: string[] = [tug.shift()!];
  while (rest.length || tug.length) {
    for (let k = 0; k < 2 && rest.length; k++) out.push(rest.shift()!);
    if (tug.length) out.push(tug.shift()!);
  }
  return out;
}

/** Turn what went wrong into something a person would say, with what to do next. */
export function friendlyLocateError(raw: string): string {
  const s = raw.toLowerCase();
  if (s.includes("isn't sharing") || s.includes("denied") || s.includes("privacy")) {
    return "Your PC is keeping your location to itself. Turn on Location in Windows settings, or just type a city.";
  }
  if (s.includes("too long") || s.includes("timed out") || s.includes("timeout")) {
    return "Couldn't find your dot this time. Try again, or type a city.";
  }
  return "The map wouldn't budge. Try again, or type a city.";
}
