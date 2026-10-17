// Layout thresholds, in CSS pixels of tug's own layout. tug zooms with CSS zoom on the page
// (lib/zoom), and neither window.innerHeight nor media queries follow it: at 150% a 768 px-tall
// window lays out only 512 px, but both still said 768, so the sidebar kept its roomy layout and its
// bottom row was cut off. These take sizes measured from the elements themselves
// (lib/useElementSize), which do follow the zoom.

/**
 * The sidebar's density for its height. Compact below 800: the roomy layout with the two-row Now
 * Playing card needs ~780 px, so a 1366×768 laptop must use compact. Tight below 640: even the
 * one-row card has no room for the volume bar's row.
 */
export function railDensity(height: number): { compact: boolean; tight: boolean } {
  return { compact: height < 800, tight: height < 640 };
}

/** Wide enough to show the Connection panel beside the Feed. */
export function isWide(width: number): boolean {
  return width >= 1240;
}
