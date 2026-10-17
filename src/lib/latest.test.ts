import { describe, expect, it } from "vitest";
import { createLoader, latestOnly, pendingKey } from "./latest";

/** A promise the test settles by hand. */
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => ((resolve = res), (reject = rej)));
  return { promise, resolve, reject };
}

describe("latestOnly", () => {
  it("only the newest request counts", () => {
    const runs = latestOnly();
    const a = runs.begin();
    const b = runs.begin();
    expect(a()).toBe(false);
    expect(b()).toBe(true);
    runs.cancel();
    expect(b()).toBe(false);
  });
});

describe("createLoader (Spotify panel detail view)", () => {
  it("Back while it loads: the spinner goes at once and the late answer doesn't reopen the view", async () => {
    const errors: unknown[] = [];
    const view = createLoader<string>((e) => errors.push(e));
    const album = deferred<string>();
    const loading = view.load(() => album.promise);
    expect(view.loading.value).toBe(true);

    view.clear(); // Back (or another tab)
    expect(view.loading.value).toBe(false);
    album.resolve("Album A");
    await loading;
    expect(view.value.value).toBeNull();
    expect(view.loading.value).toBe(false);
  });

  it("album A then album B: B shows even when A answers last, and A doesn't end B's spinner", async () => {
    const view = createLoader<string>(() => {});
    const a = deferred<string>();
    const b = deferred<string>();
    const loadA = view.load(() => a.promise);
    const loadB = view.load(() => b.promise);

    a.resolve("Album A");
    await loadA;
    expect(view.loading.value).toBe(true);
    expect(view.value.value).toBeNull();

    b.resolve("Album B");
    await loadB;
    expect(view.value.value).toBe("Album B");
    expect(view.loading.value).toBe(false);
  });

  it("a failure that's been overtaken says nothing", async () => {
    const errors: unknown[] = [];
    const view = createLoader<string>((e) => errors.push(e));
    const a = deferred<string>();
    const loadA = view.load(() => a.promise);
    void view.load(async () => "Album B");
    a.reject("rate limited");
    await loadA;
    expect(errors).toEqual([]);
  });

  it("the newest failure is reported and the spinner stops", async () => {
    const errors: unknown[] = [];
    const view = createLoader<string>((e) => errors.push(e));
    await view.load(async () => Promise.reject("offline"));
    expect(errors).toEqual(["offline"]);
    expect(view.loading.value).toBe(false);
  });
});

describe("pendingKey", () => {
  it("an older request ending late never clears a newer one for the same key", () => {
    const pending = pendingKey();
    const endA = pending.begin("album a");
    expect(pending.has("album a")).toBe(true);
    pending.begin("album b");
    const endAgain = pending.begin("album a");
    // The first lookup for album a comes back now: the second is still out.
    endA();
    expect(pending.has("album a")).toBe(true);
    endAgain();
    expect(pending.has("album a")).toBe(false);
  });

  it("knows only the newest key", () => {
    const pending = pendingKey();
    const end = pending.begin("x");
    pending.begin("y");
    expect(pending.has("x")).toBe(false);
    expect(pending.has("y")).toBe(true);
    end();
    expect(pending.has("y")).toBe(true);
  });
});
