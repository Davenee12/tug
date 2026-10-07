import { describe, expect, it } from "vitest";
import { hexToBytes, bytesToHex } from "@noble/ciphers/utils.js";
import vector from "./testVector.json";
import { ad, authHeader, b64, deriveKeys, fileId, open, requestMac, seal, unb64, OVERHEAD } from "./crypto";

const enc = (s: string) => new TextEncoder().encode(s);

describe("shared test vector (also asserted by cargo test in src-tauri/src/tugboat/crypto.rs)", () => {
  const keys = deriveKeys(hexToBytes(vector.secret));

  it("derives the same keys", () => {
    expect(bytesToHex(keys.enc)).toBe(vector.encKey);
    expect(bytesToHex(keys.auth)).toBe(vector.authKey);
  });

  it("seals to the same bytes and opens them", () => {
    const sealed = seal(keys, enc(vector.ad), enc(vector.plaintext), hexToBytes(vector.nonce));
    expect(bytesToHex(sealed)).toBe(vector.sealed);
    expect(new TextDecoder().decode(open(keys, enc(vector.ad), sealed))).toBe(vector.plaintext);
  });

  it("signs requests the same way", () => {
    const m = vector.mac;
    expect(b64(requestMac(keys, m.client, BigInt(m.seq), m.method, m.path))).toBe(m.mac);
    expect(authHeader(keys, m.client, BigInt(m.seq), m.method, m.path)).toBe(`Tugboat ${m.client}.${m.seq}.${m.mac}`);
  });

  it("names files stably", () => {
    const f = vector.fileId;
    expect(fileId(keys, f.name, f.size, f.lastModified)).toBe(f.id);
    expect(fileId(keys, f.name, f.size + 1, f.lastModified)).not.toBe(f.id);
  });
});

describe("sealing", () => {
  const keys = deriveKeys(new Uint8Array(16).fill(7));

  it("uses a fresh nonce every time", () => {
    const a = seal(keys, ad.up("x", 0), enc("same"));
    const b = seal(keys, ad.up("x", 0), enc("same"));
    expect(bytesToHex(a)).not.toBe(bytesToHex(b));
    expect(a.length).toBe(4 + OVERHEAD);
  });

  it("refuses moved, reflected or tampered chunks", () => {
    const sealed = seal(keys, ad.down("offer", 2), enc("chunk"));
    expect(() => open(keys, ad.down("offer", 3), sealed)).toThrow();
    expect(() => open(keys, ad.down("other", 2), sealed)).toThrow();
    expect(() => open(keys, ad.up("offer", 2), sealed)).toThrow();
    const bad = sealed.slice();
    bad[bad.length - 1] ^= 1;
    expect(() => open(keys, ad.down("offer", 2), bad)).toThrow();
    expect(() => open(keys, ad.down("offer", 2), sealed.subarray(0, OVERHEAD - 1))).toThrow();
  });

  it("binds request and response bodies to their sequence number", () => {
    const sealed = seal(keys, ad.response("client", 10n), enc("{}"));
    expect(() => open(keys, ad.response("client", 11n), sealed)).toThrow();
    expect(() => open(keys, ad.request("client", 10n), sealed)).toThrow();
  });
});

describe("base64url", () => {
  it("round-trips without padding", () => {
    const bytes = Uint8Array.from({ length: 16 }, (_, i) => i * 17);
    const s = b64(bytes);
    expect(s).toMatch(/^[A-Za-z0-9_-]{22}$/);
    expect(Array.from(unb64(s)!)).toEqual(Array.from(bytes));
  });

  it("rejects junk", () => {
    expect(unb64("not base64!")).toBeNull();
    expect(unb64("a+b/")).toBeNull();
  });
});
