// App-wide zoom with Ctrl +/−/0 and Ctrl+wheel, like browsers and Electron apps.
// CSS zoom on the root works in WebView2 and in the browser preview alike.

const STEPS = [0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2];

export function applyZoom(factor: number) {
  document.documentElement.style.zoom = String(factor);
}

export function stepZoom(current: number, dir: 1 | -1): number {
  const i = STEPS.findIndex((s) => s >= current - 0.001);
  const at = i < 0 ? STEPS.length - 1 : i;
  return STEPS[Math.min(STEPS.length - 1, Math.max(0, at + dir))];
}

/** Install the shortcuts; `onChange` persists and announces the new factor. Returns an uninstaller. */
export function installZoomShortcuts(get: () => number, onChange: (factor: number) => void): () => void {
  const set = (z: number) => {
    if (Math.abs(z - get()) < 0.001) return;
    applyZoom(z);
    onChange(z);
  };
  const onKey = (e: KeyboardEvent) => {
    if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
    if (e.key === "=" || e.key === "+") set(stepZoom(get(), 1));
    else if (e.key === "-" || e.key === "_") set(stepZoom(get(), -1));
    else if (e.key === "0") set(1);
    else return;
    e.preventDefault();
  };
  const onWheel = (e: WheelEvent) => {
    if (!e.ctrlKey) return;
    e.preventDefault();
    set(stepZoom(get(), e.deltaY < 0 ? 1 : -1));
  };
  window.addEventListener("keydown", onKey);
  window.addEventListener("wheel", onWheel, { passive: false });
  return () => {
    window.removeEventListener("keydown", onKey);
    window.removeEventListener("wheel", onWheel);
  };
}
