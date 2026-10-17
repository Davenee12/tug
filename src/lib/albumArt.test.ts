import { describe, expect, it } from "vitest";
import {
  AA,
  artChoice,
  cardTint,
  contrast,
  extractPalette,
  luminance,
  parseColor,
  withContrast,
  type ArtInput,
  type Rgb,
} from "./albumArt";

// The card's theme as style.css defines it (on-dark-soft text on surface-dark-elevated).
const theme = { text: parseColor("#a09d96")!, base: parseColor("#252320")! };
const rgb = (s: string) => parseColor(s)!;

const input = (over: Partial<ArtInput> = {}): ArtInput => ({
  settingOn: true,
  spotifyActive: false,
  spotifyVerified: false,
  spotifyArt: null,
  artist: "Massive Attack",
  album: "Mezzanine",
  ...over,
});

describe("where the art comes from", () => {
  it("looks the album up when Spotify isn't the player", () => {
    expect(artChoice(input())).toEqual({ kind: "lookup", artist: "Massive Attack", album: "Mezzanine", key: "massive attack\nmezzanine" });
  });

  it("Spotify's art takes precedence once it reports this song, and nothing is looked up meanwhile", () => {
    const spotify = { spotifyActive: true, spotifyArt: "data:image/jpeg;base64,AAAA" };
    expect(artChoice(input({ ...spotify, spotifyVerified: true }))).toEqual({ kind: "spotify", src: "data:image/jpeg;base64,AAAA" });
    // Right after a skip Spotify may still describe the last song: wait for it, don't look up.
    expect(artChoice(input({ ...spotify, spotifyVerified: false }))).toEqual({ kind: "none" });
    // Spotify's art is used even with the lookup switched off.
    expect(artChoice(input({ ...spotify, spotifyVerified: true, settingOn: false }))).toEqual({ kind: "spotify", src: "data:image/jpeg;base64,AAAA" });
  });

  it("falls back to the lookup when Spotify has no cover for its song", () => {
    expect(artChoice(input({ spotifyActive: true, spotifyVerified: true, spotifyArt: null })).kind).toBe("lookup");
  });

  it("switched off means no lookup: the placeholder unless Spotify has art", () => {
    expect(artChoice(input({ settingOn: false }))).toEqual({ kind: "none" });
    expect(artChoice(input({ settingOn: false, spotifyActive: true, spotifyVerified: true }))).toEqual({ kind: "none" });
  });

  it("needs both an artist and an album to look anything up", () => {
    expect(artChoice(input({ album: null }))).toEqual({ kind: "none" });
    expect(artChoice(input({ artist: "  " }))).toEqual({ kind: "none" });
  });
});

describe("colour maths", () => {
  it("reads the theme's colour formats", () => {
    expect(parseColor("#fff")).toEqual({ r: 255, g: 255, b: 255 });
    expect(parseColor(" #a09d96 ")).toEqual({ r: 160, g: 157, b: 150 });
    expect(parseColor("rgb(1 2 3)")).toEqual({ r: 1, g: 2, b: 3 });
    expect(parseColor("rgba(10, 20, 30, 0.5)")).toEqual({ r: 10, g: 20, b: 30 });
    expect(parseColor("oklch(0.5 0.1 20)")).toBeNull();
    expect(parseColor("")).toBeNull();
  });

  it("measures WCAG contrast", () => {
    expect(contrast(rgb("#000"), rgb("#fff"))).toBeCloseTo(21, 5);
    expect(contrast(rgb("#777"), rgb("#777"))).toBe(1);
    // #767676 is the well-known lightest grey with AA contrast on white.
    expect(contrast(rgb("#767676"), rgb("#fff"))).toBeGreaterThanOrEqual(4.5);
    expect(contrast(rgb("#777777"), rgb("#fff"))).toBeLessThan(4.5);
  });
});

describe("contrast adjustment", () => {
  it("leaves a colour alone when it already passes", () => {
    const navy = rgb("#101830");
    expect(withContrast(navy, theme.text, AA, "darker")).toBe(navy);
  });

  it("darkens a background until the text is readable, keeping its hue", () => {
    const cream = rgb("#f2e6c4");
    const out = withContrast(cream, theme.text, AA, "darker")!;
    expect(contrast(out, theme.text)).toBeGreaterThanOrEqual(AA);
    expect(luminance(out)).toBeLessThan(luminance(cream));
    // Still warm: red above blue.
    expect(out.r).toBeGreaterThan(out.b);
  });

  it("lightens an accent until it stands out on a dark background", () => {
    const bg = rgb("#1a1a2a");
    const out = withContrast(rgb("#202060"), bg, AA, "lighter")!;
    expect(contrast(out, bg)).toBeGreaterThanOrEqual(AA);
  });

  it("gives up when no lightness could pass", () => {
    expect(withContrast(rgb("#808080"), rgb("#808080"), 22, "darker")).toBeNull();
  });
});

/** A synthetic square image (RGBA) of `size` pixels a side, painted by `paint`. */
function image(size: number, paint: (x: number, y: number) => [number, number, number, number]): Uint8ClampedArray {
  const data = new Uint8ClampedArray(size * size * 4);
  for (let y = 0; y < size; y++)
    for (let x = 0; x < size; x++) data.set(paint(x, y), (y * size + x) * 4);
  return data;
}

const near = (a: Rgb, b: Rgb, tolerance = 12) =>
  Math.abs(a.r - b.r) <= tolerance && Math.abs(a.g - b.g) <= tolerance && Math.abs(a.b - b.b) <= tolerance;

describe("palette extraction", () => {
  it("a solid cover is its own dominant colour and accent", () => {
    const p = extractPalette(image(24, () => [200, 30, 40, 255]))!;
    expect(near(p.dominant, rgb("#c81e28"))).toBe(true);
    expect(near(p.accent!, rgb("#c81e28"))).toBe(true);
  });

  it("a mostly navy cover with a yellow disc: navy dominates, yellow is the accent", () => {
    const p = extractPalette(image(24, (x, y) => ((x - 16) ** 2 + (y - 16) ** 2 < 25 ? [250, 210, 60, 255] : [15, 25, 50, 255])))!;
    expect(near(p.dominant, rgb("#0f1932"))).toBe(true);
    expect(near(p.accent!, rgb("#fad23c"))).toBe(true);
  });

  it("a grey cover has no accent", () => {
    const p = extractPalette(image(24, (x) => (x < 12 ? [90, 90, 90, 255] : [200, 200, 200, 255])))!;
    expect(p.accent).toBeNull();
  });

  it("ignores transparent pixels, and gives up on an empty image", () => {
    const p = extractPalette(image(24, (x) => (x < 20 ? [0, 0, 0, 0] : [30, 120, 200, 255])))!;
    expect(near(p.dominant, rgb("#1e78c8"))).toBe(true);
    expect(extractPalette(image(24, () => [0, 0, 0, 0]))).toBeNull();
    expect(extractPalette([])).toBeNull();
  });
});

describe("the card's tint", () => {
  const passes = (t: { from: string; to: string; accent: string | null }) => {
    for (const bg of [t.from, t.to]) {
      expect(contrast(rgb(bg), theme.text)).toBeGreaterThanOrEqual(AA);
      if (t.accent) expect(contrast(rgb(t.accent), rgb(bg))).toBeGreaterThanOrEqual(AA);
    }
  };

  it("a light cover is darkened until the card's text is readable", () => {
    const t = cardTint({ dominant: rgb("#f6efd9"), accent: rgb("#e0a83a") }, theme)!;
    expect(t).not.toBeNull();
    passes(t);
    expect(t.accent).not.toBeNull();
  });

  it("a dark cover keeps its colour, with a lightened accent", () => {
    const t = cardTint({ dominant: rgb("#0d1b2e"), accent: rgb("#1d4a8a") }, theme)!;
    passes(t);
    // Already dark enough: only its saturation is softened a touch, never its lightness.
    expect(near(rgb(t.from), rgb("#0d1b2e"), 3)).toBe(true);
    expect(t.accent).not.toBeNull();
  });

  it("a grey cover tints the background and keeps the default bars", () => {
    const t = cardTint({ dominant: rgb("#8a8a8a"), accent: null }, theme)!;
    passes(t);
    expect(t.accent).toBeNull();
  });

  it("falls back to the default theme when the text could never be readable", () => {
    // Text so dark that no background could give it AA contrast as a dark card.
    expect(cardTint({ dominant: rgb("#c0261b"), accent: null }, { text: rgb("#202020"), base: theme.base })).toBeNull();
  });
});
