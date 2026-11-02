// Emoji shortcuts wired to a message box (a textarea with v-model): lib/emoji decides, this reads
// the box after each keystroke and puts the change back. The box's `@input` calls `onInput`, its
// `@keydown` calls `onKeydown` first (Backspace straight after a change undoes it), and Send runs
// the text through `finishForSend`.

import { afterTyping, finishForSend as finish, undoOnBackspace, type Undo } from "./emoji";

/** The one stretch where `before` and `after` differ: `before[start, end)` became `insert`. */
export function differ(before: string, after: string): { start: number; end: number; insert: string } {
  let start = 0;
  while (start < before.length && start < after.length && before[start] === after[start]) start++;
  let tail = 0;
  while (
    tail < before.length - start &&
    tail < after.length - start &&
    before[before.length - 1 - tail] === after[after.length - 1 - tail]
  ) {
    tail++;
  }
  // Never split an emoji's surrogate pair at either edge.
  const high = (c: number) => c >= 0xd800 && c <= 0xdbff;
  if (start > 0 && high(before.charCodeAt(start - 1))) start--;
  if (tail > 0 && high(before.charCodeAt(before.length - tail - 1))) tail--;
  return { start, end: before.length - tail, insert: after.slice(start, after.length - tail) };
}

export function useEmojiTyping(enabled: () => boolean) {
  let undo: Undo | null = null;
  /** Set while this writes into the box, so its own input event isn't taken for typing. */
  let writing = false;

  /** Change the box from `before` to `text`, leaving the caret at `caret`. */
  function put(box: HTMLTextAreaElement, before: string, text: string, caret: number) {
    const { start, end, insert } = differ(before, text);
    writing = true;
    try {
      // As an edit, so Ctrl+Z still steps back through the typing (setting `value` would wipe the
      // box's undo history). It fires the input event v-model reads.
      box.setSelectionRange(start, end);
      const edited = document.execCommand?.("insertText", false, insert) === true && box.value === text;
      if (!edited) {
        box.value = text;
        // A plain Event: v-model reads it.
        box.dispatchEvent(new Event("input"));
      }
    } finally {
      writing = false;
    }
    box.setSelectionRange(caret, caret);
  }

  function onInput(e: Event) {
    if (writing) return;
    const box = e.target as HTMLTextAreaElement;
    undo = null;
    // Only typing: pasted text, dictation and an IME word mid-composition stay as they are.
    if (!(e instanceof InputEvent) || e.isComposing) return;
    if (e.inputType !== "insertText" && e.inputType !== "insertLineBreak") return;
    if (!enabled() || box.selectionStart !== box.selectionEnd) return;
    const change = afterTyping(box.value, box.selectionStart);
    if (!change) return;
    put(box, box.value, change.text, change.caret);
    undo = change.undo;
  }

  /** True when it handled the key (a Backspace that undid a change). */
  function onKeydown(e: KeyboardEvent): boolean {
    if (e.key !== "Backspace") return false;
    const box = e.target as HTMLTextAreaElement;
    const last = undo;
    undo = null;
    // A box that can't be edited right now (sending) stays as it is.
    if (box.readOnly || box.disabled || box.selectionStart !== box.selectionEnd) return false;
    const back = undoOnBackspace(last, box.value, box.selectionStart);
    if (!back) return false;
    e.preventDefault();
    put(box, box.value, back.text, back.caret);
    return true;
  }

  function finishForSend(text: string): string {
    return enabled() ? finish(text) : text;
  }

  return { onInput, onKeydown, finishForSend };
}
