import { describe, expect, it } from "vitest";
import { b64 } from "./crypto";
import { LEGACY_SECRET_KEY, keyFromHash, takeKey, type SecretEnv } from "./secret";

const SECRET = Uint8Array.from({ length: 16 }, (_, i) => i + 1);

/** A fake browser: records what the page did to the address bar and to storage. */
function fakeEnv(hash: string, storage: "ok" | "blocked" | "throws" = "ok") {
  const urls: string[] = [];
  const removed: string[] = [];
  const writes: string[] = [];
  const store = {
    removeItem: (key: string) => {
      if (storage === "throws") throw new Error("SecurityError");
      removed.push(key);
    },
    // Present only to catch a write; the page must never call it.
    setItem: (key: string) => writes.push(key),
  };
  const env: SecretEnv = {
    location: { hash, pathname: "/" },
    history: { replaceState: (_d: unknown, _t: string, url?: string | URL | null) => void urls.push(String(url)) },
    sessionStorage: storage === "blocked" ? null : store,
  };
  return { env, urls, removed, writes };
}

describe("the Tugboat secret", () => {
  it("is read from the link's fragment", () => {
    expect(keyFromHash(`#${b64(SECRET)}`)).toEqual({ secret: SECRET, guest: false });
    expect(keyFromHash(b64(SECRET))).toEqual({ secret: SECRET, guest: false });
  });

  it("must be exactly 16 bytes of base64url", () => {
    expect(keyFromHash("")).toBeNull();
    expect(keyFromHash("#")).toBeNull();
    expect(keyFromHash(`#${b64(SECRET.subarray(0, 15))}`)).toBeNull();
    expect(keyFromHash("#not base64!")).toBeNull();
    expect(keyFromHash("#g.")).toBeNull();
    expect(keyFromHash(`#g.${b64(SECRET.subarray(0, 15))}`)).toBeNull();
  });

  it("from a race's Player 2 code is the guest secret, and the page knows it's a guest", () => {
    expect(keyFromHash(`#g.${b64(SECRET)}`)).toEqual({ secret: SECRET, guest: true });
    const { env, urls } = fakeEnv(`#g.${b64(SECRET)}`);
    expect(takeKey(env)).toEqual({ secret: SECRET, guest: true });
    expect(urls, "taken out of the address bar like any secret").toEqual(["/"]);
  });

  it("is taken out of the address bar and kept only in memory", () => {
    const { env, urls, removed, writes } = fakeEnv(`#${b64(SECRET)}`);
    expect(takeKey(env)?.secret ?? null).toEqual(SECRET);
    expect(urls).toEqual(["/"]);
    expect(writes).toEqual([]);
    // A secret an earlier version of the page stored is cleared.
    expect(removed).toEqual([LEGACY_SECRET_KEY]);
  });

  it("isn't recovered after a reload: the page asks for a fresh scan", () => {
    const { env, urls } = fakeEnv("");
    expect(takeKey(env)?.secret ?? null).toBeNull();
    expect(urls, "nothing to strip").toEqual([]);
  });

  it("strips a malformed fragment too, without accepting it", () => {
    const { env, urls } = fakeEnv("#garbage");
    expect(takeKey(env)?.secret ?? null).toBeNull();
    expect(urls).toEqual(["/"]);
  });

  it("works when storage is blocked or throws", () => {
    for (const mode of ["blocked", "throws"] as const) {
      const { env } = fakeEnv(`#${b64(SECRET)}`, mode);
      expect(takeKey(env)?.secret ?? null).toEqual(SECRET);
    }
  });
});
