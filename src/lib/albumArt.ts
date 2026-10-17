// Album art and its colours on the Now Playing card. AMS never sends artwork, so the art comes
// from Spotify when it's the player (and reports this song), otherwise from Apple's public catalog
// (looked up in Rust, `album_art.rs`, while Show album art is on). The card is tinted from the
// art: a dominant colour for a soft background and a readable accent for the bars, both checked
// against the card's text for WCAG AA contrast, or the card keeps its usual colours.
//
// Everything here is pure: the store (stores/albumArt) feeds it plain values, and the one canvas
// pass that reads a cover's pixels lives in lib/artCanvas.

/** A colour, each channel 0–255. */
export interface Rgb {
  r: number;
  g: number;
  b: number;
}

/** Where this song's art comes from. */
export type ArtChoice =
  | { kind: "spotify"; src: string }
  | { kind: "lookup"; artist: string; album: string; key: string }
  | { kind: "none" };

export interface ArtInput {
  /** Show album art (Settings › General): may tug look the album up in Apple's catalog? */
  settingOn: boolean;
  /** Spotify is connected and is the phone's player. */
  spotifyActive: boolean;
  /** Spotify reports the song the phone is playing (right after a skip it can lag a moment). */
  spotifyVerified: boolean;
  /** Spotify's cover for its current song, as a data URI. */
  spotifyArt: string | null;
  /** The song as shown (never "Listening on <device>", see lib/playback). */
  artist: string | null;
  album: string | null;
}

const NONE: ArtChoice = { kind: "none" };

/**
 * Spotify's art takes precedence: while Spotify is the player, its cover is used once it reports
 * this song, and nothing is looked up while that's pending. Only when Spotify isn't the player, or
 * has no cover for the song, is the album looked up, and only with the setting on and both an
 * artist and an album to send. Off and no Spotify: the placeholder.
 */
export function artChoice(i: ArtInput): ArtChoice {
  if (i.spotifyActive) {
    if (!i.spotifyVerified) return NONE;
    if (i.spotifyArt) return { kind: "spotify", src: i.spotifyArt };
  }
  if (!i.settingOn) return NONE;
  const artist = i.artist?.trim();
  const album = i.album?.trim();
  if (!artist || !album) return NONE;
  return { kind: "lookup", artist, album, key: `${artist.toLowerCase()}\n${album.toLowerCase()}` };
}

// ---- Colour maths ----

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** `#rgb`, `#rrggbb` or `rgb(r g b)` / `rgb(r, g, b)`; null for anything else. */
export function parseColor(text: string): Rgb | null {
  const s = text.trim().toLowerCase();
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/.exec(s);
  if (hex) {
    const h = hex[1].length === 3 ? [...hex[1]].map((c) => c + c).join("") : hex[1];
    return { r: parseInt(h.slice(0, 2), 16), g: parseInt(h.slice(2, 4), 16), b: parseInt(h.slice(4, 6), 16) };
  }
  const rgb = /^rgba?\(\s*(\d+(?:\.\d+)?)[\s,]+(\d+(?:\.\d+)?)[\s,]+(\d+(?:\.\d+)?)/.exec(s);
  if (rgb) return { r: clamp(Number(rgb[1]), 0, 255), g: clamp(Number(rgb[2]), 0, 255), b: clamp(Number(rgb[3]), 0, 255) };
  return null;
}

/** For a style value: `rgb(r g b)`, whole numbers. */
export const css = (c: Rgb) => `rgb(${Math.round(c.r)} ${Math.round(c.g)} ${Math.round(c.b)})`;

/** WCAG 2 relative luminance (0 black – 1 white). */
export function luminance(c: Rgb): number {
  const lin = (v: number) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b);
}

/** WCAG 2 contrast ratio between two colours (1–21). */
export function contrast(a: Rgb, b: Rgb): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

interface Hsl {
  h: number;
  s: number;
  l: number;
}

export function toHsl(c: Rgb): Hsl {
  const r = c.r / 255;
  const g = c.g / 255;
  const b = c.b / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  const d = max - min;
  if (d === 0) return { h: 0, s: 0, l };
  const s = d / (1 - Math.abs(2 * l - 1));
  let h = max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  h *= 60;
  if (h < 0) h += 360;
  return { h, s, l };
}

export function fromHsl({ h, s, l }: Hsl): Rgb {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  const [r, g, b] =
    h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  return { r: (r + m) * 255, g: (g + m) * 255, b: (b + m) * 255 };
}

/** `a` blended toward `b` by `t` (0 = a, 1 = b). */
export function mix(a: Rgb, b: Rgb, t: number): Rgb {
  return { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t };
}

/**
 * The colour moved only in lightness (hue and saturation kept) until it has at least `min`
 * contrast with `against`: darker or lighter. Unchanged if it already does; null if even black or
 * white wouldn't (then the caller falls back to the default theme).
 */
export function withContrast(color: Rgb, against: Rgb, min: number, direction: "darker" | "lighter"): Rgb | null {
  if (contrast(color, against) >= min) return color;
  const hsl = toHsl(color);
  const step = direction === "darker" ? -0.01 : 0.01;
  for (let l = hsl.l + step; l >= 0 && l <= 1; l += step) {
    const next = fromHsl({ ...hsl, l: clamp(l, 0, 1) });
    if (contrast(next, against) >= min) return next;
  }
  return null;
}

// ---- Palette ----

/** The colours a cover gives the card: its most common colour and its most vivid one. */
export interface Palette {
  dominant: Rgb;
  /** Null for a cover with no real colour (greys), which keeps the default bars. */
  accent: Rgb | null;
}

/**
 * A small palette from a cover's pixels (RGBA, as `getImageData` gives them; a 24×24 thumbnail is
 * plenty). Pixels are grouped into 512 boxes of similar colour: the fullest box is the dominant
 * colour, and the box with the best mix of vividness and size is the accent. Null for an image
 * that is mostly transparent.
 */
export function extractPalette(rgba: ArrayLike<number>): Palette | null {
  const boxes = new Map<number, { n: number; r: number; g: number; b: number }>();
  let total = 0;
  for (let i = 0; i + 3 < rgba.length; i += 4) {
    if (rgba[i + 3] < 128) continue;
    const r = rgba[i];
    const g = rgba[i + 1];
    const b = rgba[i + 2];
    const key = ((r >> 5) << 6) | ((g >> 5) << 3) | (b >> 5);
    const box = boxes.get(key) ?? { n: 0, r: 0, g: 0, b: 0 };
    box.n++;
    box.r += r;
    box.g += g;
    box.b += b;
    boxes.set(key, box);
    total++;
  }
  if (total < 16) return null;
  const mean = (x: { n: number; r: number; g: number; b: number }): Rgb => ({ r: x.r / x.n, g: x.g / x.n, b: x.b / x.n });
  const all = [...boxes.entries()].sort((a, b) => b[1].n - a[1].n || a[0] - b[0]).map(([, box]) => box);
  const dominant = mean(all[0]);
  let accent: Rgb | null = null;
  let best = 0;
  for (const box of all) {
    if (box.n < Math.max(2, total * 0.02)) continue;
    const c = mean(box);
    const { s, l } = toHsl(c);
    // Vivid: saturated, and neither near-black nor near-white (where saturation means little).
    const vivid = s * (1 - Math.abs(l - 0.5) * 1.6);
    if (vivid < 0.15) continue;
    const score = vivid * vivid * Math.sqrt(box.n / total);
    if (score > best) {
      best = score;
      accent = c;
    }
  }
  return { dominant, accent };
}

// ---- The card's tint ----

/** WCAG AA for body text. The accent bars are held to it too, so they're always easy to see. */
export const AA = 4.5;
/** The background tint's saturation is capped so it stays soft. */
const MAX_TINT_SATURATION = 0.55;
/** How far the gradient's far end blends back to the card's usual surface. */
const FADE_TO_BASE = 0.6;

/** The card's colours from a cover, as style values; null keeps the default theme. */
export interface CardTint {
  /** The gradient's start (top left): the cover's colour, darkened for the text. */
  from: string;
  /** The gradient's end: blended back toward the card's usual surface. */
  to: string;
  /** The progress and volume bars; null keeps the default bars. */
  accent: string | null;
}

/**
 * The card's tint from a palette. `text` is the dimmest text on the card (the artist line and
 * times) and `base` its usual surface, both read from the theme. The background is darkened until
 * that text has AA contrast on both ends of the gradient; the accent is lightened until it has AA
 * contrast on both. If the background can't get there, the default theme stays; if the accent
 * can't, the default bars stay.
 */
export function cardTint(p: Palette, theme: { text: Rgb; base: Rgb }): CardTint | null {
  const hsl = toHsl(p.dominant);
  const soft = fromHsl({ ...hsl, s: Math.min(hsl.s, MAX_TINT_SATURATION) });
  const from = withContrast(soft, theme.text, AA, "darker");
  if (!from) return null;
  const to = mix(from, theme.base, FADE_TO_BASE);
  if (contrast(to, theme.text) < AA || contrast(theme.base, theme.text) < AA) return null;
  let accent: Rgb | null = null;
  if (p.accent) {
    // Against whichever end of the gradient is lighter (the harder one to stand out on).
    const lighter = luminance(from) >= luminance(to) ? from : to;
    const candidate = withContrast(p.accent, lighter, AA, "lighter");
    if (candidate && contrast(candidate, from) >= AA && contrast(candidate, to) >= AA) accent = candidate;
  }
  return { from: css(from), to: css(to), accent: accent ? css(accent) : null };
}
