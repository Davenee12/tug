// Doing a smart action (lib/smartActions): open its page in the browser, or copy it and say so
// at the bottom of the window, the way copying a code does.

import { useTugStore } from "../stores/tug";
import { copyText } from "./clipboard";
import type { SmartAction } from "./smartActions";

export function useSmartAction() {
  const tug = useTugStore();

  /** True once it's done: the page handed to the browser, or the text on the clipboard. */
  async function run(action: SmartAction): Promise<boolean> {
    if (action.url) {
      await tug.openUrl(action.url);
      return true;
    }
    if (action.copy == null) return false;
    if (!(await copyText(action.copy))) {
      tug.notify("error", "Couldn't copy to the clipboard.");
      return false;
    }
    // While the screen is shared, the message is blurred: don't spell the number out underneath.
    tug.notify("info", tug.previewsHidden ? "Copied" : `Copied ${action.copy}`);
    return true;
  }

  return { run };
}
