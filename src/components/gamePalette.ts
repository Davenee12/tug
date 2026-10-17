// The games draw on a canvas in tug's own colours, read from theme classes on hidden swatches
// (`<span data-swatch="day:water" class="bg-accent-teal/20" />`), so no colour is written in code.
// A night swatch replaces the day one it names; anything without a night swatch is shared.

export function readSwatches<K extends string>(el: HTMLElement | null, set: "day" | "night", names: readonly K[]): Record<K, string> | null {
  if (!el) return null;
  const pick = (name: string) => {
    const s = el.querySelector<HTMLElement>(`[data-swatch="${set}:${name}"]`) ?? el.querySelector<HTMLElement>(`[data-swatch="day:${name}"]`);
    return s ? getComputedStyle(s).backgroundColor : "";
  };
  return Object.fromEntries(names.map((n) => [n, pick(n)])) as Record<K, string>;
}
