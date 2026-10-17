import { describe, expect, it } from "vitest";
import { coalesce } from "./coalesce";
import { isCurrent, NewestRev, SnapshotGuard, type GuardedRow } from "./ordering";

describe("isCurrent", () => {
  it("keeps a newer status over an older one that arrives later", () => {
    // The audit's race: the actor sends "connected" (rev 5) and a command thread's older
    // "connecting" (rev 4) reaches the window after it.
    let shown = { rev: 0, connection: "noDevice" };
    for (const next of [
      { rev: 5, connection: "connected" },
      { rev: 4, connection: "connecting" },
    ]) {
      if (isCurrent(next, shown)) shown = next;
    }
    expect(shown.connection).toBe("connected");
  });

  it("drops a startup read that an event overtook", () => {
    // Listeners go in, the read is sent, an event (rev 8) lands, then the read's answer (rev 7).
    const event = { rev: 8 };
    const read = { rev: 7 };
    expect(isCurrent(read, event)).toBe(false);
    // A read that's newer than every event so far is taken.
    expect(isCurrent({ rev: 9 }, event)).toBe(true);
  });

  it("takes an equal revision (the same state) and treats a missing rev as 0", () => {
    expect(isCurrent({ rev: 3 }, { rev: 3 })).toBe(true);
    expect(isCurrent({ rev: 1 }, null)).toBe(true);
    expect(isCurrent({}, {})).toBe(true); // the dev mock and the placeholder: always taken
    expect(isCurrent({}, { rev: 2 })).toBe(false);
  });
});

describe("NewestRev with the frame wait", () => {
  it("an older Now Playing event can't take the place of a newer one waiting for the frame", () => {
    const applied: number[] = [];
    let run: (() => void) | null = null;
    const c = coalesce<{ rev: number }>(
      (np) => applied.push(np.rev),
      (fn) => {
        run = fn;
        return () => (run = null);
      },
    );
    const newest = new NewestRev();
    for (const np of [{ rev: 3 }, { rev: 2 }]) if (newest.accept(np)) c.push(np);
    (run as (() => void) | null)?.();
    expect(applied).toEqual([3]);
  });

  it("reset starts over", () => {
    const n = new NewestRev();
    expect(n.accept({ rev: 5 })).toBe(true);
    expect(n.accept({ rev: 4 })).toBe(false);
    n.reset();
    expect(n.accept({ rev: 4 })).toBe(true);
  });
});

const row = (id: number, extra: Partial<GuardedRow> = {}): GuardedRow => ({ id, removedAt: null, live: true, ...extra });

describe("SnapshotGuard", () => {
  it("a removal that lands while a refresh is in flight isn't undone (the audit's case)", () => {
    const g = new SnapshotGuard();
    const list = [row(1)];
    const token = g.begin(); // refreshLoadedNotifications sends its read
    // notification-removed arrives before the read answers.
    g.removed(1, 1_000);
    list[0] = { ...list[0], removedAt: 1_000, live: false };
    // The read was taken before the removal: it still says the row is there and live.
    const merged = g.merge([row(1)], token, (id) => list.some((x) => x.id === id));
    g.end(token);
    expect(merged).toEqual([]);
    expect(list[0]).toMatchObject({ removedAt: 1_000, live: false });
  });

  it("a removal of a row the window hadn't loaded is applied to the read's copy", () => {
    const g = new SnapshotGuard();
    const token = g.begin();
    g.removed(7, 2_000);
    expect(g.merge([row(7)], token, () => false)).toEqual([row(7, { removedAt: 2_000, live: false })]);
  });

  it("an update event that lands meanwhile keeps the event's copy", () => {
    const g = new SnapshotGuard();
    const token = g.begin();
    g.touched(3);
    expect(g.merge([row(3), row(4)], token, (id) => id === 3)).toEqual([row(4)]);
  });

  it("a conversation deleted meanwhile isn't brought back", () => {
    const g = new SnapshotGuard();
    const token = g.begin();
    g.touched(5); // hidden from the list
    expect(g.merge([row(5)], token, () => false)).toEqual([]);
  });

  it("after the link dropped, an older read can't make rows live again", () => {
    const g = new SnapshotGuard();
    const token = g.begin();
    g.allDead();
    expect(g.merge([row(1), row(2, { live: false })], token, () => true)).toEqual([
      row(1, { live: false }),
      row(2, { live: false }),
    ]);
    g.end(token);
    // A read that starts after the drop takes the backend's word for it.
    const later = g.begin();
    expect(g.merge([row(1)], later, () => true)).toEqual([row(1)]);
  });

  it("events before a read began don't hold it back", () => {
    const g = new SnapshotGuard();
    const first = g.begin();
    g.removed(1, 500);
    const second = g.begin(); // started after the removal: it already knows
    g.end(first);
    expect(g.merge([row(1, { removedAt: 500, live: false })], second, () => true)).toEqual([
      row(1, { removedAt: 500, live: false }),
    ]);
  });

  it("remembers nothing while no read is in flight", () => {
    const g = new SnapshotGuard();
    g.removed(1, 100);
    g.touched(2);
    g.allDead();
    const token = g.begin();
    expect(g.merge([row(1), row(2)], token, () => true)).toEqual([row(1), row(2)]);
  });

  it("forgets what it tracked once the last read ends", () => {
    const g = new SnapshotGuard();
    const a = g.begin();
    g.touched(1);
    g.end(a);
    // Internals cleared: a stale entry can't affect a later read's row.
    const b = g.begin();
    expect(g.merge([row(1)], b, () => true)).toEqual([row(1)]);
  });
});
