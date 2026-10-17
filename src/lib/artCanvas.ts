// The DOM side of album art colours (rules in lib/albumArt): one cheap canvas pass per new cover,
// and the theme colours the card's tint is checked against. Nothing here runs per frame.

import { extractPalette, parseColor, type Palette, type Rgb } from "./albumArt";

/** The cover is shrunk to this many pixels a side before its colours are read: plenty for a palette. */
const SAMPLE_PX = 24;

/**
 * The palette of a cover (a `data:` URI, so the canvas is never tainted). Null if the image can't
 * be decoded or read, which keeps the card's default colours.
 */
export async function paletteFromImage(src: string): Promise<Palette | null> {
  try {
    const img = new Image();
    img.decoding = "async";
    img.src = src;
    await img.decode();
    const canvas = document.createElement("canvas");
    canvas.width = SAMPLE_PX;
    canvas.height = SAMPLE_PX;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) return null;
    ctx.drawImage(img, 0, 0, SAMPLE_PX, SAMPLE_PX);
    return extractPalette(ctx.getImageData(0, 0, SAMPLE_PX, SAMPLE_PX).data);
  } catch {
    return null;
  }
}

/**
 * The card's dimmest text colour and usual surface, from the theme tokens in style.css (so no
 * colour is hard-coded here). Null if they can't be read, which keeps the default colours.
 */
export function themeColors(): { text: Rgb; base: Rgb } | null {
  const style = getComputedStyle(document.documentElement);
  const text = parseColor(style.getPropertyValue("--color-on-dark-soft"));
  const base = parseColor(style.getPropertyValue("--color-surface-dark-elevated"));
  return text && base ? { text, base } : null;
}
