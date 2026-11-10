/**
 * Who is asking for the iPhone scan right now. Each panel that shows the find step holds the scan
 * under its own token, and the scan only stops when the last one lets go.
 *
 * Two panels are briefly alive together whenever one replaces the other (closing Settings › iPhone
 * back to the Feed's Connect panel, or Forget in the side panel): Vue sets up the new one before
 * the old one's unmount runs. With a single shared on/off, the old panel's "stop" landed after the
 * new panel's "start" and switched the scan off under a panel still saying "Looking for your
 * iPhone…", and the periodic re-scan can't restart a stopped scan. Pure, so the order is unit-tested.
 */
export function scanOwners() {
  const owners = new Set<symbol>();
  return {
    /** `owner` wants the scan running. */
    hold(owner: symbol): void {
      owners.add(owner);
    },
    /** `owner` no longer needs the scan. True when nobody does, so it should stop now. */
    release(owner: symbol): boolean {
      owners.delete(owner);
      return owners.size === 0;
    },
  };
}
