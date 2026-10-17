// When each Feed entry was last looked at (`ui.seen`), across the change that moved other chat apps
// (WhatsApp, Teams…) out of Messages: they used to be one conversation per person
// (`com.whatsapp\0sam`), and are now one Feed stack per app (`app\0com.whatsapp`). Without this, a
// stack had never been "seen", so every message in it showed as new after the upgrade.

import { MESSAGES_APP } from "./format";

const STACK_PREFIX = "app\u0000";

/**
 * Fold the old per-person keys of other chat apps into their app's stack key (the newest time
 * wins) and drop them. Texts' own keys (the Messages app) are kept. Returns null when there's
 * nothing to change, so the setting is only rewritten once.
 */
export function foldChatAppSeen(seen: Record<string, number>): Record<string, number> | null {
  const out: Record<string, number> = {};
  let changed = false;
  for (const [key, at] of Object.entries(seen)) {
    const sep = key.indexOf("\u0000");
    const appId = sep > 0 ? key.slice(0, sep) : "";
    if (!appId || key.startsWith(STACK_PREFIX) || appId === MESSAGES_APP) {
      out[key] = Math.max(out[key] ?? 0, at);
      continue;
    }
    const stack = `${STACK_PREFIX}${appId}`;
    out[stack] = Math.max(out[stack] ?? 0, at);
    changed = true;
  }
  return changed ? out : null;
}
