import { GUEST_PREFIX, unb64 } from "./crypto";

/**
 * The session secret rides in the link's `#` part, which the browser never sends over the
 * network. The page takes it out of the address bar straight away and keeps it only in memory:
 * never in web storage, where it would outlive its use. So a reload (or Safari dropping the tab
 * in the background) needs a fresh scan of the code in tug; the client id is remembered, so the
 * PC still knows it's the same phone and an interrupted file picks up where it left off.
 *
 * Tugboat's own code carries the session secret. A two-player race's Player 2 code carries
 * `g.` and a guest secret instead (derived from the session's on the PC, and good only for
 * steering): the page then signs and seals with the guest secret's keys and is only a controller.
 */

/** Where the page was opened from, and the bits of the browser it touches (fakes in tests). */
export interface SecretEnv {
  location: Pick<Location, "hash" | "pathname">;
  history: Pick<History, "replaceState">;
  /** The tab's session storage, or null where it's blocked. */
  sessionStorage: Pick<Storage, "removeItem"> | null;
}

/** Earlier versions of the page kept the secret here; it's removed wherever it's found. */
export const LEGACY_SECRET_KEY = "tugboat.secret";

/** The secret a link carried, and whether it's the Player 2 code's guest secret. */
export interface PageKey {
  secret: Uint8Array;
  guest: boolean;
}

/** The 16-byte secret in a `#…` fragment (`#g.…` for the Player 2 code), or null if there isn't a valid one. */
export function keyFromHash(hash: string): PageKey | null {
  const fragment = hash.replace(/^#/, "");
  const guest = fragment.startsWith(GUEST_PREFIX);
  const bytes = unb64(guest ? fragment.slice(GUEST_PREFIX.length) : fragment);
  return bytes && bytes.length === 16 ? { secret: bytes, guest } : null;
}

/** Take the secret out of the address bar (whatever the fragment held) and return it, if valid. */
export function takeKey(env: SecretEnv): PageKey | null {
  try {
    env.sessionStorage?.removeItem(LEGACY_SECRET_KEY);
  } catch {
    /* storage blocked: nothing was kept there either */
  }
  const { hash, pathname } = env.location;
  if (hash.length <= 1) return null;
  const key = keyFromHash(hash);
  env.history.replaceState(null, "", pathname);
  return key;
}
