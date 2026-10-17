/**
 * Keeping the window's copy of backend state from going stale.
 *
 * The backend sends a state (device status, Now Playing, PC audio) as an event every time it
 * changes, and the window also reads it (at startup, or as a command's answer). Those can arrive
 * out of order: a read that started before a change can land after the change's event. The
 * backend numbers every change (`rev`), so the window keeps whichever copy is newest.
 *
 * Notifications are a list, not one value, so a list read taken before an event (a removal, an
 * update) must not undo that event for the rows it touched: `SnapshotGuard`.
 */

/** A backend state that counts its changes (`rev`; see `src-tauri/src/state.rs`). */
export interface Revisioned {
  rev?: number;
}

/**
 * Whether `next` is at least as new as `current` and so may replace it. A missing `rev` counts
 * as 0 (the store's placeholder before anything arrived, and the dev mock). Equal revisions carry
 * the same state, so taking one again is harmless.
 */
export function isCurrent(next: Revisioned, current: Revisioned | null | undefined): boolean {
  return (next.rev ?? 0) >= (current?.rev ?? 0);
}

/**
 * Tracks the newest `rev` seen for a state whose events are applied later (Now Playing waits for
 * the next frame), so an older event can't replace a newer one that is still waiting.
 */
export class NewestRev {
  private newest = 0;
  /** Whether `next` is the newest so far (recording it if so). */
  accept(next: Revisioned): boolean {
    const rev = next.rev ?? 0;
    if (rev < this.newest) return false;
    this.newest = rev;
    return true;
  }
  reset(): void {
    this.newest = 0;
  }
}

/** The parts of a notification row the guard needs. */
export interface GuardedRow {
  id: number;
  removedAt: number | null;
  live: boolean;
}

/**
 * Stops a list read (a snapshot) from undoing events that arrived while it was in flight.
 *
 * `begin()` before the read, `merge()` its rows, `end()` afterwards (also when it failed). Events
 * that change rows call `touched`, `removed` or `allDead`; only while a read is in flight is
 * anything remembered, so nothing grows while tug sits idle.
 *
 * - A row an event changed after the read began keeps the event's version.
 * - A row removed after the read began stays removed, even if the window hadn't loaded it.
 * - A row hidden from the list (a deleted conversation) after the read began isn't put back.
 * - After the link dropped (every row stopped being live), an older read can't make rows live.
 */
export class SnapshotGuard {
  private seq = 0;
  private inFlight = new Set<number>();
  /** id → sequence number of the last event that changed (or hid) it. */
  private changedAt = new Map<number, number>();
  /** id → when it was removed, for removals the window had no row for. */
  private removedAtById = new Map<number, number>();
  private allDeadAt = 0;

  /** A read is about to start: returns its token. */
  begin(): number {
    const token = ++this.seq;
    this.inFlight.add(token);
    return token;
  }

  /** The read with this token is finished (merged, or failed). */
  end(token: number): void {
    this.inFlight.delete(token);
    if (this.inFlight.size === 0) {
      this.changedAt.clear();
      this.removedAtById.clear();
    }
  }

  /** An event changed (or hid, or put back) row `id`: the window's copy is the newest. */
  touched(id: number): void {
    if (this.inFlight.size) this.changedAt.set(id, ++this.seq);
  }

  /** The phone removed row `id` at `at` (Unix ms). */
  removed(id: number, at: number): void {
    if (!this.inFlight.size) return;
    this.touched(id);
    this.removedAtById.set(id, at);
  }

  /** The link dropped: no row is live any more. */
  allDead(): void {
    if (this.inFlight.size) this.allDeadAt = ++this.seq;
  }

  /**
   * The rows of the read with `token` that may be written into the list, adjusted for what
   * happened since it began. `has(id)` says whether the window already holds that row.
   */
  merge<T extends GuardedRow>(rows: readonly T[], token: number, has: (id: number) => boolean): T[] {
    const out: T[] = [];
    for (const row of rows) {
      let next = row;
      const changed = this.changedAt.get(row.id);
      if (changed !== undefined && changed > token) {
        // The event's copy is newer than the read's.
        if (has(row.id)) continue;
        const removedAt = this.removedAtById.get(row.id);
        // Not in the window and not a removal: hidden since (a deleted conversation).
        if (removedAt === undefined) continue;
        next = { ...next, removedAt: next.removedAt ?? removedAt, live: false };
      }
      if (this.allDeadAt > token && next.live) next = { ...next, live: false };
      out.push(next);
    }
    return out;
  }
}
