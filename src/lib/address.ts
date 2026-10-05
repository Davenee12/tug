// Phone numbers and emails as tug compares them. Mirrors `src-tauri/src/map/address.rs`, so
// a number typed, shown by iOS in a notification title, or sent by MAP is one key.

/** Invisible direction marks iOS wraps around numbers in notification titles. */
const BIDI = /[‎‏‪-‮⁦-⁩]/g;

/**
 * `+1 (302) 669-8133`, `13026698133`, `3026698133` → `+13026698133`. Emails are lower-cased.
 * Short codes and other numbers keep their digits; anything else is returned trimmed.
 */
export function normalizeAddress(raw: string): string {
  const s = raw.replace(BIDI, "").trim();
  if (s.includes("@")) return s.toLowerCase();
  const digits = s.replace(/\D/g, "");
  if (!digits) return s;
  if (s.startsWith("+")) return `+${digits}`;
  if (digits.length === 10) return `+1${digits}`;
  if (digits.length === 11 && digits.startsWith("1")) return `+${digits}`;
  return digits;
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
