import { unb64 } from "./crypto";

/**
 * The session secret rides in the link's `#` part, which the browser never sends over the
 * network. The page takes it out of the address bar straight away and keeps it only in memory:
 * never in web storage, where it would outlive its use. So a reload (or Safari dropping the tab
 * in the background) needs a fresh scan of the code in tug; the client id is remembered, so the
 * PC still knows it's the same phone and an interrupted file picks up where it left off.
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

/** The 16-byte secret in a `#…` fragment, or null if there isn't a valid one. */
export function secretFromHash(hash: string): Uint8Array | null {
  const bytes = unb64(hash.replace(/^#/, ""));
  return bytes && bytes.length === 16 ? bytes : null;
}

/** Take the secret out of the address bar (whatever the fragment held) and return it, if valid. */
export function takeSecret(env: SecretEnv): Uint8Array | null {
  try {
    env.sessionStorage?.removeItem(LEGACY_SECRET_KEY);
  } catch {
    /* storage blocked: nothing was kept there either */
  }
  const { hash, pathname } = env.location;
  if (hash.length <= 1) return null;
  const secret = secretFromHash(hash);
  env.history.replaceState(null, "", pathname);
  return secret;
}
