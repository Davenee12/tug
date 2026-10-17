/**
 * The window's startup reads (status, Now Playing, the first notifications, texts, contacts,
 * calls). Each read applies its own result, so one failing can't hold back the others, and what
 * failed is read again on a gentle schedule instead of leaving tug half-started until a restart.
 */

/** One startup read: fetches and applies its result; rejects when the read failed. */
export type StartupRead = () => Promise<void>;

/** When something prevents tug from starting at all (its listeners couldn't be set up). */
export const START_FAILED = "tug didn't start properly. Quit it from the tray and open it again.";

/** When a startup read failed and its retry did too. tug keeps retrying. */
export const STILL_LOADING = "tug couldn't load everything yet. It keeps trying by itself.";

/** Run every read at once and wait for all of them; returns the ones that failed, in order. */
export async function settleAll(reads: readonly StartupRead[]): Promise<StartupRead[]> {
  // Through a promise, so a read that throws before its first await counts as failed too.
  const results = await Promise.allSettled(reads.map((read) => Promise.resolve().then(read)));
  return reads.filter((_, i) => results[i].status === "rejected");
}

const DELAYS_MS = [2_000, 5_000, 15_000];
/** The pace once the quick retries are used up: cheap, and a read that starts working is caught soon. */
const STEADY_MS = 30_000;

/** How long to wait before retry number `tries` (0 for the first retry). */
export function retryDelay(tries: number): number {
  return DELAYS_MS[tries] ?? STEADY_MS;
}
