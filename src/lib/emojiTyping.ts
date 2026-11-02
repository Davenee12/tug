// Emoji shortcuts wired to a message box (a textarea with v-model): lib/emoji decides, this reads
// the box after each keystroke and puts the change back. The box's `@input` calls `onInput`, its
// `@keydown` calls `onKeydown` first (Backspace straight after a change undoes it), and Send runs
// the text through `finishForSend`.

import { afterTyping, finishForSend as finish, undoOnBackspace, type Undo } from "./emoji";

export function useEmojiTyping(enabled: () => boolean) {
  let undo: Undo | null = null;

  /** Write `text` into the box with the caret at `caret`, and tell v-model. */
  function put(box: HTMLTextAreaElement, text: string, caret: number) {
    box.value = text;
    box.setSelectionRange(caret, caret);
    // A plain Event, not an InputEvent: v-model reads it, `onInput` below ignores it.
    box.dispatchEvent(new Event("input"));
  }

  function onInput(e: Event) {
    const box = e.target as HTMLTextAreaElement;
    // Only typing: pasted text, dictation and an IME word mid-composition stay as they are.
    if (!(e instanceof InputEvent) || e.isComposing) return;
    if (e.inputType !== "insertText" && e.inputType !== "insertLineBreak") {
      undo = null;
      return;
    }
    undo = null;
    if (!enabled() || box.selectionStart !== box.selectionEnd) return;
    const change = afterTyping(box.value, box.selectionStart);
    if (!change) return;
    put(box, change.text, change.caret);
    undo = change.undo;
  }

  /** True when it handled the key (a Backspace that undid a change). */
  function onKeydown(e: KeyboardEvent): boolean {
    if (e.key !== "Backspace") return false;
    const box = e.target as HTMLTextAreaElement;
    const back = box.selectionStart === box.selectionEnd ? undoOnBackspace(undo, box.value, box.selectionStart) : null;
    undo = null;
    if (!back) return false;
    e.preventDefault();
    put(box, back.text, back.caret);
    return true;
  }

  function finishForSend(text: string): string {
    return enabled() ? finish(text) : text;
  }

  return { onInput, onKeydown, finishForSend };
}
