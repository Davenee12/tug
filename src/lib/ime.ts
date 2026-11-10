/**
 * Whether a key press belongs to an input method that's still composing a word (Japanese, Chinese,
 * Korean and others): the Enter that picks a candidate, or the arrows that move between them.
 * Those keys are the input method's, never tug's: taking Enter as "send" sent the half-typed text.
 * `keyCode` 229 is what browsers report for such a key when `isComposing` isn't set yet.
 */
export function imeComposing(e: { isComposing?: boolean; keyCode?: number }): boolean {
  return e.isComposing === true || e.keyCode === 229;
}
