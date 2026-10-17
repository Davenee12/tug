// Settings › Data & privacy › Clear history. tug says "History cleared" and empties its lists only
// once the backend says it worked: a failure leaves everything shown as it is (it's all still
// there) and says why, rather than a window that looks empty over a history that isn't.

export interface ClearHistoryUi {
  /** Empty the notifications, texts and recent calls tug is showing. */
  emptyLists(): void;
  say(kind: "info" | "error", text: string): void;
}

/** Run the clear; true only if it worked (and then the lists are emptied and it says so). */
export async function clearHistory(
  clear: () => Promise<unknown>,
  ui: ClearHistoryUi,
  describe: (e: unknown) => string,
): Promise<boolean> {
  try {
    await clear();
  } catch (e) {
    ui.say("error", `Couldn't clear history: ${describe(e)}`);
    return false;
  }
  ui.emptyLists();
  ui.say("info", "History cleared");
  return true;
}
