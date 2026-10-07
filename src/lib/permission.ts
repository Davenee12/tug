// Windows notification permission, asked without nagging but without getting stuck.
//
// Once granted it stays granted for the launch, so that answer is remembered. A "no" isn't: the
// person can turn tug's notifications on in Windows Settings at any time, and caching the refusal
// kept every pop-up off until tug was restarted.

/** Wraps a permission check so a granted answer is remembered and a refusal is asked again next time. */
export function grantedOnceCache(check: () => Promise<boolean>): () => Promise<boolean> {
  let granted = false;
  return async () => {
    if (granted) return true;
    granted = await check();
    return granted;
  };
}
