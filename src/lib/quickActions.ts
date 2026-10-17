// Taskbar quick actions: the Tasks in tug's jump list (right-click tug on the taskbar or in
// Start). Rust queues each click (src-tauri/src/quick_actions) and nudges with "quick-action";
// the UI takes what's waiting once it has loaded and on every nudge. Taking clears the queue in
// Rust, so a click runs once even if nudges pile up or a take races another. Pure: the app hands
// in how to take and what each action does.

import type { QuickAction } from "../types/protocol";

/** What each task does; App.vue wires these to the same paths as Ctrl+K and the shortcuts. */
export interface QuickActionHandlers {
  newMessage: () => void;
  tugboat: () => void;
  copyCode: () => void;
  game: () => void;
  settings: () => void;
}

const ROUTES: Record<QuickAction, keyof QuickActionHandlers> = {
  "new-message": "newMessage",
  tugboat: "tugboat",
  "copy-code": "copyCode",
  game: "game",
  settings: "settings",
};

/** Carry out one action; false (and nothing done) for one this build doesn't know. */
export function runQuickAction(action: string, handlers: QuickActionHandlers): boolean {
  if (!Object.hasOwn(ROUTES, action)) return false;
  handlers[ROUTES[action as QuickAction]]();
  return true;
}

/**
 * Drain the queue: take what's waiting and run it in order. One take at a time; a nudge that
 * comes while a take is in flight takes again afterwards, so nothing waits for the next click.
 * A failed take (the backend away for a moment) leaves the actions queued for the next one.
 */
export function quickActionRunner(take: () => Promise<string[]>, run: (action: string) => void) {
  let draining: Promise<void> | null = null;
  let again = false;

  async function loop() {
    do {
      again = false;
      let actions: string[];
      try {
        actions = await take();
      } catch {
        return;
      }
      for (const action of actions) run(action);
    } while (again);
  }

  return {
    drain(): Promise<void> {
      if (draining) {
        again = true;
        return draining;
      }
      draining = loop().finally(() => (draining = null));
      return draining;
    },
  };
}
