// Phone numbers and emails as tug compares them. Mirrors `src-tauri/src/map/address.rs`, so
// a number typed, shown by iOS in a notification title, or sent by MAP is one key.

/** Invisible direction marks iOS wraps around numbers in notification titles. */
const BIDI = /[‎‏‪-‮⁦-⁩]/g;

/**
 * Regions that share the North American Numbering Plan (country code +1), as ISO 3166-1 alpha-2
 * codes. Only on a PC set to one of these is a bare 10-digit number taken as `+1…`. Mirrors
 * `NANP_REGIONS` in `src-tauri/src/map/address.rs`.
 */
const NANP_REGIONS = new Set([
  "US", "CA", "PR", "VI", "GU", "AS", "MP", "UM", "AG", "AI", "BB", "BM", "BS", "DM", "DO", "GD", "JM",
  "KN", "KY", "LC", "MS", "SX", "TC", "TT", "VC", "VG",
]);

/** Whether `region` (an ISO 3166-1 alpha-2 code, any case) dials with +1. */
export function isNanpRegion(region: string | null | undefined): boolean {
  return !!region && NANP_REGIONS.has(region.trim().toUpperCase());
}

/** The region part of a language tag: `en-US` → `US`, `fr` → null. */
export function regionFromLocale(tag: string | null | undefined): string | null {
  if (!tag) return null;
  try {
    return new Intl.Locale(tag).region ?? null;
  } catch {
    return null;
  }
}

// The PC's region. Until Rust says (`setPcRegion` at startup, from Windows' own Region setting, so
// both sides read numbers alike), the WebView's language is the best guess. Unit tests run as if on
// a US PC (like the Rust side), so they don't depend on the machine's settings.
let pcRegion: string | null =
  import.meta.env.MODE === "test" ? "US" : regionFromLocale(typeof navigator === "undefined" ? null : navigator.language);

/** Use Windows' Region setting (from the `pc_region` command); null keeps the current guess. */
export function setPcRegion(region: string | null | undefined): void {
  if (region) pcRegion = region.toUpperCase();
}

/**
 * `+1 (302) 555-0173`, `13025550173` → `+13025550173`, and on a PC in a +1 region a bare
 * `3025550173` too. Emails are lower-cased. Anything else keeps its digits (and a leading `+`) as
 * typed and the iPhone resolves it as it would a number typed there: a local `07700 900123` or
 * `0491 570 006` is never turned into a US number. Non-numbers are returned trimmed.
 *
 * Mirrors `normalize_in` in `src-tauri/src/map/address.rs`; change both together.
 */
export function normalizeAddressIn(raw: string, region: string | null): string {
  const s = raw.replace(BIDI, "").trim();
  if (s.includes("@")) return s.toLowerCase();
  const digits = s.replace(/\D/g, "");
  if (!digits) return s;
  if (s.startsWith("+")) return `+${digits}`;
  // A leading 0 is a trunk prefix (UK, AU, FR, IN, ...): never a North American number.
  if (digits.startsWith("0")) return digits;
  if (isNanpRegion(region)) {
    if (digits.length === 10) return `+1${digits}`;
    if (digits.length === 11 && digits.startsWith("1")) return `+${digits}`;
  }
  return digits;
}

/** {@link normalizeAddressIn} for this PC's region (one argument, so it's safe in `.map`). */
export function normalizeAddress(raw: string): string {
  return normalizeAddressIn(raw, pcRegion);
}

/**
 * A sender shown as a number, short code or email rather than a name. iOS titles a text with
 * the sender's name only when they're in the iPhone's contacts; everyone else is their number.
 */
export function isAddressLike(s: string): boolean {
  const t = s.replace(BIDI, "").trim();
  if (/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(t)) return true;
  return /^\+?[\d\s().-]+$/.test(t) && t.replace(/\D/g, "").length >= 3;
}

/** The last 10 digits of a full phone number (to match `+44 7700…` with `07700…`), else null. */
export function numberTail(address: string): string | null {
  if (address.includes("@")) return null;
  const digits = address.replace(/\D/g, "");
  return digits.length >= 10 ? digits.slice(-10) : null;
}
