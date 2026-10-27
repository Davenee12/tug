// Smart actions in texts: a link, an email address, a phone number, a street address or a parcel
// tracking number in a text or notification becomes one click (Open link, Copy, Open in Maps,
// Track parcel), so nobody has to pick up the phone to act on it.
//
// Conservative throughout: a wrong action is worse than none, so anything ambiguous stays plain
// text. A one-time code is never offered again here (lib/codes has its own Copy code chip).
//
// Pure module: no Tauri, no DOM. Message bubbles render `smartSegments` inline; Feed rows and
// notifications show `smartChips` as compact buttons. Every `url` is an https:// address with no
// whitespace, the only kind the backend's `open_url` will launch.

import { findCode } from "./codes";

export type SmartKind = "link" | "email" | "phone" | "address" | "tracking";

export interface SmartAction {
  kind: SmartKind;
  /** As written in the text: `body.slice(start, end)`. */
  text: string;
  start: number;
  end: number;
  /** Button text ("Open link", "Copy number", "Open in Maps", "Track parcel", "Copy email"). */
  label: string;
  /** What happens, to what: the tooltip and screen-reader name ("Copy +1 (555) 010-0123"). */
  title: string;
  /** An http(s) address to open in the browser (links, Maps, the carrier's tracking page). */
  url?: string;
  /** What to put on the clipboard (numbers, email addresses). */
  copy?: string;
}

/** A body split for rendering: plain text, and the pieces that do something. */
export type SmartSegment = { kind: "text"; text: string } | { kind: "action"; text: string; action: SmartAction };

// ---------------------------------------------------------------------------------------------
// Links

/**
 * What a link can't contain: spaces, quotes or <angle brackets> around it, emoji or CJK punctuation
 * after it, and control or invisible characters (which the backend won't open anyway).
 */
const URL_CHARS = "[^\\s<>\"'`\\p{Extended_Pictographic}\\p{C}\\uFE0F\\u3000-\\u303F\\uFF01-\\uFF0F]";
/** A link, up to the next one when two are joined by a comma ("…/a,https://…/b"). */
const SCHEME_URL = new RegExp(`(?<![\\p{L}\\p{N}])https?:\\/\\/(?:(?![,;]https?:\\/\\/)${URL_CHARS})+`, "giu");

/**
 * Endings that make a bare name a website on their own ("example.com"). Only common, unmistakable
 * ones, never short words (`.is`, `.in`, `.at`, `.it`, `.to`, `.so`, `.us`…), so "this.is" or
 * "log.in" stay words. Lower case or all capitals ("EXAMPLE.COM"), not "Mr.Me".
 */
const TLDS = [
  "com", "org", "net", "edu", "gov", "info", "biz", "app", "dev", "xyz",
  "uk", "ca", "au", "nz", "ie", "nl", "dk", "fi", "pl", "jp", "mx", "br",
];
/**
 * Endings that are also words somewhere ("eu", "io", "es", "de", "se" are words in Portuguese,
 * Italian, Spanish…; "me", "co"…): a website only with a path after it ("t.co/x", "wa.me/1555…"),
 * so "Hola.es tarde" or "lol.me too" stay words.
 */
const PATH_TLDS = [
  "io", "co", "ai", "me", "ly", "gl", "gg", "tv", "fm", "de", "fr", "es", "ch", "se", "pt", "eu",
  "shop", "store", "online", "site", "link", "page", "news", "blog",
];
const alternation = (list: string[]) => [...list, ...list.map((t) => t.toUpperCase())].sort((a, b) => b.length - a.length).join("|");
const LABEL = "[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?";
/** Not part of an email address, a longer word, or a path. */
const BARE_BEFORE = "(?<![\\p{L}\\p{N}@._%+\\-\\/:#])";
const BARE_END = "(?![\\p{L}\\p{N}@_\\-])(?::\\d{2,5})?";
/**
 * `www.example.com`, `example.com/path`, `t.co/abc`: a known ending (or `www.` in front), then an
 * optional port and a path, query or fragment.
 */
const BARE_URL = new RegExp(
  BARE_BEFORE +
    `(?:(?:www\\.(?:${LABEL}\\.)+[A-Za-z]{2,24}|(?:${LABEL}\\.)+(?:${alternation(TLDS)}))${BARE_END}(?:[/?#]${URL_CHARS}*)?` +
    `|(?:${LABEL}\\.)+(?:${alternation(PATH_TLDS)})${BARE_END}\\/${URL_CHARS}+)`,
  "gu",
);

/** Punctuation that ends a sentence rather than a link ("see example.com/a." → no "."). */
const TRAILING = /[.,;:!?'"’”…]$/u;
const CLOSERS: Record<string, string> = { ")": "(", "]": "[", "}": "{" };

/**
 * A link without the sentence's punctuation after it; a closing bracket stays if it's the link's
 * own. Brackets are counted once, not per character trimmed: a text ending in thousands of ")"
 * mustn't freeze the window.
 */
function trimLink(raw: string): string {
  const count: Record<string, number> = {};
  for (const ch of raw) if (ch in CLOSERS || Object.values(CLOSERS).includes(ch)) count[ch] = (count[ch] ?? 0) + 1;
  let end = raw.length;
  while (end > 0) {
    const last = raw[end - 1];
    if (TRAILING.test(last)) {
      end--;
      continue;
    }
    const open = CLOSERS[last];
    if (open && (count[open] ?? 0) < (count[last] ?? 0)) {
      count[last]--;
      end--;
      continue;
    }
    break;
  }
  return raw.slice(0, end);
}

/** A host a browser can go to: dotted names (any language) or an IPv4 address. */
const HOST = /^(?:(?:[\p{L}\p{N}](?:[\p{L}\p{N}-]*[\p{L}\p{N}])?\.)+\p{L}{2,}|\d{1,3}(?:\.\d{1,3}){3})$/u;

function hostOf(url: string): string | null {
  const m = /^https?:\/\/(?:[^@/?#]*@)?([^/?#:]+)/i.exec(url);
  return m && HOST.test(m[1]) ? m[1] : null;
}

function linkAction(text: string, start: number, url: string): SmartAction | null {
  const host = hostOf(url);
  if (!host) return null;
  return { kind: "link", text, start, end: start + text.length, label: "Open link", title: `Open ${text} in your browser`, url };
}

/**
 * The same page, for not offering it twice: scheme, `www.`, case of the host and a trailing slash
 * don't matter (`https://www.Example.com/` and `example.com`).
 */
export function sameUrlKey(url: string): string {
  return url
    .trim()
    .replace(/^https?:\/\//i, "")
    .replace(/^www\./i, "")
    .replace(/^[^/?#]+/, (host) => host.toLowerCase())
    .replace(/\/+$/, "");
}

// ---------------------------------------------------------------------------------------------
// Email addresses

const EMAIL =
  /(?<![\p{L}\p{N}._%+\-])[A-Za-z0-9](?:[A-Za-z0-9._%+\-]*[A-Za-z0-9_%+\-])?@(?:[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?\.)+[A-Za-z]{2,24}(?![\p{L}\p{N}_\-@]|\.[A-Za-z0-9])/gu;

// ---------------------------------------------------------------------------------------------
// Parcel tracking numbers. Each carrier's own shape, and nothing looser: a "Track parcel" that
// opens someone's order number on the wrong site is worse than none.

interface Carrier {
  name: string;
  url: (n: string) => string;
}
const UPS: Carrier = { name: "UPS", url: (n) => `https://www.ups.com/track?tracknum=${n}` };
const USPS: Carrier = { name: "USPS", url: (n) => `https://tools.usps.com/go/TrackConfirmAction?tLabels=${n}` };
const FEDEX: Carrier = { name: "FedEx", url: (n) => `https://www.fedex.com/fedextrack/?trknbr=${n}` };

/** UPS: 1Z and 16 letters and digits, written solid or as UPS prints it (1Z 999 AA1 01 2345 6784). */
const UPS_NUMBER = /(?<![A-Za-z0-9])1Z(?:[0-9A-Z]{16}| [0-9A-Z]{3} [0-9A-Z]{3} [0-9A-Z]{2} [0-9A-Z]{4} [0-9A-Z]{4})(?![A-Za-z0-9])/g;
/** USPS: 20 or 22 digits starting 91–95, solid or in fours (9400 1000 0000 0000 0000 00). */
const USPS_NUMBER = /(?<![\p{L}\p{N}+$£€.,-])9[1-5](?:\d{18}(?:\d{2})?|\d{2}(?: \d{4}){4}(?: \d{2})?)(?![\p{L}\p{N}]|[.,-]\d)/gu;
/** International mail handed to USPS (EC 123 456 785 US): S10 shape with its check digit. */
const S10_NUMBER = /(?<![A-Za-z0-9])([CELRUV][A-Z]) ?(\d{3}) ?(\d{3}) ?(\d{3}) ?US(?![A-Za-z0-9])/g;
/**
 * FedEx: 12 or 15 digits (or 1234 5678 9012), only in a text that says FedEx. Never part of a
 * longer run of digit groups: the first twelve digits of a card number aren't a parcel.
 */
const FEDEX_NUMBER = /(?<![\p{L}\p{N}+$£€.,-]|\d )(?:\d{15}|\d{12}|\d{4} \d{4} \d{4})(?![\p{L}\p{N}]|[.,-]\d| \d)/gu;
const MENTIONS_FEDEX = /(?<![\p{L}\p{N}])fed[ -]?ex(?![\p{L}\p{N}])/iu;

/** UPU S10 check digit: weights 8 6 4 2 3 5 9 7 over the first eight digits. */
function s10Valid(digits: string): boolean {
  const weights = [8, 6, 4, 2, 3, 5, 9, 7];
  const sum = weights.reduce((acc, w, i) => acc + w * Number(digits[i]), 0);
  const check = 11 - (sum % 11);
  const expected = check === 10 ? 0 : check === 11 ? 5 : check;
  return expected === Number(digits[8]);
}

function trackingAction(text: string, start: number, carrier: Carrier): SmartAction {
  const number = text.replace(/\s/g, "");
  return {
    kind: "tracking",
    text,
    start,
    end: start + text.length,
    label: "Track parcel",
    title: `Track ${number} on the ${carrier.name} website`,
    url: carrier.url(number),
  };
}

// ---------------------------------------------------------------------------------------------
// Street addresses: a house number, capitalised street name words, and a street type
// ("221B Baker St", "1600 Pennsylvania Ave NW, Washington, DC 20500"). Lower-case prose ("5 miles
// down the road") and title-case phrases led by small words ("Part 2 Down The Road") never match.

const STREET_TYPES = [
  "Street", "St", "Avenue", "Ave", "Road", "Rd", "Boulevard", "Blvd", "Lane", "Ln", "Drive", "Dr",
  "Way", "Court", "Ct", "Place", "Pl", "Parkway", "Pkwy",
];
const STREET_TYPE = [...STREET_TYPES, ...STREET_TYPES.map((t) => t.toUpperCase())].sort((a, b) => b.length - a.length).join("|");
// AM and PM too: "at 3 PM Dr Patel" is a time and a doctor, not "3 PM Dr".
const SMALL_WORDS = "The|A|An|And|Or|Of|To|In|On|At|For|By|With|From|Into|My|Your|Our|Their|His|Her|Its|This|That|These|Down|Up|Is|Are|Was|Be|I|We|You|It|AM|PM";
const NAME_WORD = `(?!(?:${SMALL_WORDS})(?![\\p{L}]))(?:[A-Z][A-Za-z'’-]*\\.?|\\d{1,3}(?:st|nd|rd|th|ST|ND|RD|TH))`;
const US_STATES =
  "AL|AK|AZ|AR|CA|CO|CT|DE|DC|FL|GA|HI|ID|IL|IN|IA|KS|KY|LA|ME|MD|MA|MI|MN|MS|MO|MT|NE|NV|NH|NJ|NM|NY|NC|ND|OH|OK|OR|PA|RI|SC|SD|TN|TX|UT|VT|VA|WA|WV|WI|WY";
const ADDRESS = new RegExp(
  `(?<![\\p{L}\\p{N}$£€#:./,-])` +
    // House number (221B), then one to four street-name words, then the street type.
    `\\d{1,6}[A-Z]?[ \\t]+(?:${NAME_WORD}[ \\t]+){1,4}(?:${STREET_TYPE})(?![\\p{L}\\p{N}-])` +
    // "St." only when more address follows ("Main St., Apt 4").
    `(?:\\.(?=[ \\t]*,))?` +
    // A direction after the street type (NW).
    `(?:[ \\t]+(?:NE|NW|SE|SW|N|S|E|W)(?![\\p{L}\\p{N}]))?` +
    // A unit: Apt 4B, Suite 200, #12.
    `(?:,?[ \\t]*(?:(?:Apt|Suite|Ste|Unit)\\.?[ \\t]*#?[ \\t]*|#[ \\t]?)[A-Za-z0-9-]{1,6}(?![\\p{L}\\p{N}]))?` +
    // City, ST and ZIP: only with a real state code, so ", Then we…" isn't taken for a city.
    `(?:,[ \\t]*[A-Z][a-z]+(?:[ \\t]+[A-Z][a-z]+){0,2},?[ \\t]+(?:${US_STATES})(?![\\p{L}\\p{N}])(?:[ \\t]+\\d{5}(?:-\\d{4})?(?![\\p{N}]))?)?`,
  "gu",
);

function addressAction(text: string, start: number): SmartAction {
  const query = text.replace(/\s+/g, " ").trim();
  return {
    kind: "address",
    text,
    start,
    end: start + text.length,
    label: "Open in Maps",
    title: `Open ${query} in Google Maps`,
    url: `https://www.google.com/maps/search/?api=1&query=${encodeURIComponent(query)}`,
  };
}

// ---------------------------------------------------------------------------------------------
// Phone numbers: written like phone numbers, never a bare run of digits (an order number, an
// account number). Not a price, a time, a date or part of a longer number.

/** Not after a letter, digit, currency, # or another number's punctuation or groups. */
const PHONE_BEFORE = "(?<![\\p{L}\\p{N}+$£€#.:/,_-])(?<!\\p{N} )";
/** Not before a letter, digit or %, nor before more of a longer number (12.5, 555-0123-4, 0123 4567). */
const PHONE_AFTER = "(?![\\p{L}\\p{N}%]|[.,:/-]\\d| \\d\\d)";
/** International: + country code and groups (+44 20 7946 0958, +44 (0)7700 900123, +1 (555) 010-0123). */
const INTL_PHONE = new RegExp(`${PHONE_BEFORE}\\+\\d{1,3}(?:[ .-]?\\(?\\d{1,5}\\)?){1,6}${PHONE_AFTER}`, "gu");
/** North American: (555) 010-0123, 555-010-0123, 555.010.0123, 1-555-010-0123. */
const NANP_PHONE = new RegExp(`${PHONE_BEFORE}(?:1[ .-]?)?(?:\\(\\d{3}\\)[ .-]?|\\d{3}[ .-])\\d{3}[ .-]\\d{4}${PHONE_AFTER}`, "gu");
/** National with a leading 0 and spaces or dashes: 07700 900123, 020 7946 0958, 0491 570 006. */
const ZERO_PHONE = new RegExp(`${PHONE_BEFORE}0\\d{2,4}[ -]\\d{3,4}(?:[ -]?\\d{3,4})?${PHONE_AFTER}`, "gu");

/** The number as a phone number, or null: a bracket from the sentence and a stray next word dropped. */
function cleanPhone(raw: string): string | null {
  let s = raw;
  const count = (c: string) => s.split(c).length - 1;
  if (s.endsWith(")") && count("(") < count(")")) s = s.slice(0, -1);
  // "+44 20 7946 0958 2 days": the lone 2 is the next word.
  while (/[ .-]\d$/.test(s)) s = s.slice(0, -2);
  if (count("(") !== count(")")) return null;
  const digits = s.replace(/\D/g, "");
  if (s.startsWith("+")) {
    // +1 is North America: exactly 11 digits. Elsewhere, 8 to 15 (the international maximum).
    if (digits.startsWith("1") ? digits.length !== 11 : digits.length < 8 || digits.length > 15) return null;
  } else if (digits.length < 10 || digits.length > 11) {
    return null;
  }
  return s;
}

function phoneAction(text: string, start: number): SmartAction {
  return { kind: "phone", text, start, end: start + text.length, label: "Copy number", title: `Copy ${text}`, copy: text };
}

// ---------------------------------------------------------------------------------------------

/**
 * Everything actionable in `body`, in reading order, never overlapping. Earlier kinds win a
 * contested stretch: a number inside a link is part of the link, and an email's domain is not
 * a separate website.
 */
export function findSmartActions(body: string | null | undefined): SmartAction[] {
  if (!body) return [];
  const taken: [number, number][] = [];
  const found: SmartAction[] = [];
  const free = (start: number, end: number) => !taken.some(([s, e]) => start < e && end > s);
  const claim = (a: SmartAction | null) => {
    if (!a || !free(a.start, a.end)) return;
    taken.push([a.start, a.end]);
    found.push(a);
  };

  for (const m of body.matchAll(SCHEME_URL)) {
    const text = trimLink(m[0]);
    // Always opened over https, whatever the text says ("HTTP://…", "http://…").
    claim(linkAction(text, m.index, `https://${text.replace(/^https?:\/\//i, "")}`));
  }
  for (const m of body.matchAll(EMAIL)) {
    claim({ kind: "email", text: m[0], start: m.index, end: m.index + m[0].length, label: "Copy email", title: `Copy ${m[0]}`, copy: m[0] });
  }
  for (const m of body.matchAll(BARE_URL)) {
    const text = trimLink(m[0]);
    claim(linkAction(text, m.index, `https://${text}`));
  }

  // A one-time code already has its Copy code chip: its digits are never a number or parcel here.
  const code = findCode(body);
  if (code) {
    for (let i = body.indexOf(code.shown); i >= 0; i = body.indexOf(code.shown, i + 1)) {
      const end = i + code.shown.length;
      if (!/[\p{L}\p{N}]/u.test(body[i - 1] ?? "") && !/[\p{L}\p{N}]/u.test(body[end] ?? "")) taken.push([i, end]);
    }
  }

  for (const m of body.matchAll(UPS_NUMBER)) {
    if ((m[0].match(/\d/g)?.length ?? 0) >= 8) claim(trackingAction(m[0], m.index, UPS));
  }
  for (const m of body.matchAll(USPS_NUMBER)) claim(trackingAction(m[0], m.index, USPS));
  for (const m of body.matchAll(S10_NUMBER)) {
    if (s10Valid(m[2] + m[3] + m[4])) claim(trackingAction(m[0], m.index, USPS));
  }
  if (MENTIONS_FEDEX.test(body)) {
    for (const m of body.matchAll(FEDEX_NUMBER)) claim(trackingAction(m[0], m.index, FEDEX));
  }

  for (const m of body.matchAll(ADDRESS)) claim(addressAction(m[0], m.index));

  for (const pattern of [INTL_PHONE, NANP_PHONE, ZERO_PHONE]) {
    for (const m of body.matchAll(pattern)) {
      const text = cleanPhone(m[0]);
      if (text) claim(phoneAction(text, m.index));
    }
  }

  return found.sort((a, b) => a.start - b.start);
}

/**
 * `body` as plain text and actions, in order; joined back together they're `body` exactly. With
 * `links: false` (a text from someone you don't know) its web links stay plain text, as iOS keeps
 * them: one-click links are how scam texts work. Addresses, numbers and parcel numbers still act,
 * because tug builds those pages itself.
 */
export function smartSegments(body: string | null | undefined, { links = true }: { links?: boolean } = {}): SmartSegment[] {
  if (!body) return [];
  const out: SmartSegment[] = [];
  let at = 0;
  for (const action of findSmartActions(body).filter((a) => links || a.kind !== "link")) {
    if (action.start > at) out.push({ kind: "text", text: body.slice(at, action.start) });
    out.push({ kind: "action", text: action.text, action });
    at = action.end;
  }
  if (at < body.length) out.push({ kind: "text", text: body.slice(at) });
  return out;
}

/** Which chip comes first when there's room for only a couple: the most specific action. */
const CHIP_ORDER: SmartKind[] = ["tracking", "address", "link", "phone", "email"];

export interface ChipOptions {
  /** At most this many chips (default 3). */
  limit?: number;
  /** Pages the item already has a button for (its app's Open), not offered again. */
  skipUrls?: (string | null | undefined)[];
  /** False for a text from someone you don't know: no web link chips (see smartSegments). */
  links?: boolean;
}

/**
 * The compact buttons for a Feed row or notification: one per kind (two "Copy number" chips
 * couldn't be told apart), nothing the item already offers, the most specific first.
 */
export function smartChips(body: string | null | undefined, options: ChipOptions = {}): SmartAction[] {
  const { limit = 3, skipUrls = [], links = true } = options;
  const kinds = new Set<SmartKind>();
  const targets = new Set(skipUrls.filter((u): u is string => !!u).map(sameUrlKey));
  const chips: SmartAction[] = [];
  for (const a of findSmartActions(body)) {
    const target = a.url ? sameUrlKey(a.url) : `copy:${a.copy}`;
    if (kinds.has(a.kind) || targets.has(target) || (!links && a.kind === "link")) continue;
    kinds.add(a.kind);
    targets.add(target);
    chips.push(a);
  }
  return chips.sort((a, b) => CHIP_ORDER.indexOf(a.kind) - CHIP_ORDER.indexOf(b.kind)).slice(0, Math.max(0, limit));
}
