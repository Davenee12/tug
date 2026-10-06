// Copy text on a plain-HTTP page: the Clipboard API needs a secure context, so this uses the
// older execCommand("copy") on a temporary, selected textarea (the way that works on iOS Safari).
// Must run inside a tap handler.

export function copyText(text: string): boolean {
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.readOnly = true; // no keyboard popping up
  ta.contentEditable = "true";
  Object.assign(ta.style, { position: "fixed", top: "0", left: "0", opacity: "0", fontSize: "16px", pointerEvents: "none" });
  document.body.appendChild(ta);
  const range = document.createRange();
  range.selectNodeContents(ta);
  const sel = window.getSelection();
  sel?.removeAllRanges();
  sel?.addRange(range);
  ta.setSelectionRange(0, text.length);
  let ok = false;
  try {
    ok = document.execCommand("copy");
  } catch {
    ok = false;
  }
  sel?.removeAllRanges();
  document.body.removeChild(ta);
  return ok;
}
