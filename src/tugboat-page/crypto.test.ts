import { describe, expect, it } from "vitest";
import { hexToBytes, bytesToHex } from "@noble/ciphers/utils.js";
import vector from "./testVector.json";
import guestVector from "./guestVector.json";
import aead010 from "./aead010Vector.json";
import { GUEST_LABEL, GUEST_PREFIX, ad, authHeader, b64, deriveKeys, fileId, guestSecret, open, requestMac, seal, unb64, OVERHEAD } from "./crypto";
import { keyFromHash } from "./secret";

const enc = (s: string) => new TextEncoder().encode(s);

describe("shared test vector (also asserted by cargo test in src-tauri/src/tugboat/crypto.rs)", () => {
  const keys = deriveKeys(hexToBytes(vector.secret));

  it("derives the same keys", () => {
    expect(bytesToHex(keys.enc)).toBe(vector.encKey);
    expect(bytesToHex(keys.auth)).toBe(vector.authKey);
  });

  it("seals to the same bytes and opens them", () => {
    const a = vector.ads;
    const up = ad.up(a.fileId, a.index);
    const sealed = seal(keys, up, enc(vector.plaintext), hexToBytes(vector.nonce));
    expect(bytesToHex(sealed)).toBe(vector.sealed);
    expect(new TextDecoder().decode(open(keys, up, sealed))).toBe(vector.plaintext);
  });

  it("builds every associated-data string byte for byte", () => {
    const a = vector.ads;
    const text = (b: Uint8Array) => new TextDecoder().decode(b);
    expect(text(ad.up(a.fileId, a.index))).toBe(a.up);
    expect(text(ad.down(a.offerId, a.offerIndex))).toBe(a.down);
    expect(text(ad.request(vector.mac.client, BigInt(vector.mac.seq)))).toBe(a.req);
    expect(text(ad.response(vector.mac.client, BigInt(vector.mac.seq)))).toBe(a.res);
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

describe("guest key vector (also asserted by cargo test in src-tauri/src/tugboat/crypto.rs)", () => {
  const secret = hexToBytes(guestVector.secret);
  const guest = guestSecret(secret);

  it("derives the same guest secret, with the same label", () => {
    expect(GUEST_LABEL).toBe(guestVector.label);
    expect(bytesToHex(guest)).toBe(guestVector.guestSecret);
    expect(`#${GUEST_PREFIX}${b64(guest)}`).toBe(guestVector.fragment);
  });

  it("derives the same guest keys and signs the same way", () => {
    const keys = deriveKeys(guest);
    expect(bytesToHex(keys.enc)).toBe(guestVector.encKey);
    expect(bytesToHex(keys.auth)).toBe(guestVector.authKey);
    const m = guestVector.mac;
    expect(b64(requestMac(keys, m.client, BigInt(m.seq), m.method, m.path))).toBe(m.mac);
  });

  it("shares nothing with the session's own keys", () => {
    const session = deriveKeys(secret);
    expect(bytesToHex(guest)).not.toBe(guestVector.secret);
    expect(bytesToHex(session.enc)).not.toBe(guestVector.encKey);
    expect(bytesToHex(session.auth)).not.toBe(guestVector.authKey);
  });

  it("is what the page reads out of the Player 2 code's link", () => {
    const key = keyFromHash(guestVector.fragment);
    expect(key?.guest).toBe(true);
    expect(bytesToHex(key!.secret)).toBe(guestVector.guestSecret);
  });
});

describe("RustCrypto chacha20poly1305 0.10 output (also asserted by cargo test in src-tauri/src/tugboat/crypto.rs)", () => {
  const keys = deriveKeys(hexToBytes(aead010.secret));

  it("seals to the same bytes the old Rust library did, and opens them", () => {
    for (const c of aead010.fixedNonce) {
      const sealed = seal(keys, hexToBytes(c.ad), hexToBytes(c.plaintext), hexToBytes(c.nonce));
      expect(bytesToHex(sealed)).toBe(c.sealed);
      expect(bytesToHex(open(keys, hexToBytes(c.ad), hexToBytes(c.sealed)))).toBe(c.plaintext);
    }
  });

  it("opens what the old Rust library sealed with a random nonce", () => {
    for (const c of aead010.randomNonce) {
      expect(bytesToHex(open(keys, hexToBytes(c.ad), hexToBytes(c.sealed)))).toBe(c.plaintext);
    }
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
