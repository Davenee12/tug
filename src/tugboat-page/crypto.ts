// Tugboat's crypto on the phone, byte-for-byte the same as src-tauri/src/tugboat/crypto.rs.
//
// The page is plain HTTP, so `crypto.subtle` doesn't exist here; `crypto.getRandomValues` does.
// Everything else comes from @noble/ciphers and @noble/hashes (audited, pure JS), bundled by tug.
// Keys come from the secret after the `#` in the QR link, which the browser never sends.

import { xchacha20poly1305 } from "@noble/ciphers/chacha.js";
import { randomBytes } from "@noble/ciphers/utils.js";
import { hkdf } from "@noble/hashes/hkdf.js";
import { hmac } from "@noble/hashes/hmac.js";
import { sha256 } from "@noble/hashes/sha2.js";

export const VERSION = "tugboat/1";
export const NONCE_LEN = 24;
export const TAG_LEN = 16;
export const OVERHEAD = NONCE_LEN + TAG_LEN;
const MAC_LEN = 16;

const enc = new TextEncoder();
const utf8 = (s: string) => enc.encode(s);

export interface Keys {
  enc: Uint8Array;
  auth: Uint8Array;
}

/** The encryption and request-signing keys for one Tugboat session's secret. */
export function deriveKeys(secret: Uint8Array): Keys {
  const salt = utf8(VERSION);
  return {
    enc: hkdf(sha256, secret, salt, utf8("encrypt"), 32),
    auth: hkdf(sha256, secret, salt, utf8("authenticate"), 32),
  };
}

/** `nonce || ciphertext || tag`, with a fresh random nonce unless one is given (test vector only). */
export function seal(keys: Keys, ad: Uint8Array, plaintext: Uint8Array, nonce: Uint8Array = randomBytes(NONCE_LEN)): Uint8Array {
  const ct = xchacha20poly1305(keys.enc, nonce, ad).encrypt(plaintext);
  const out = new Uint8Array(NONCE_LEN + ct.length);
  out.set(nonce, 0);
  out.set(ct, NONCE_LEN);
  return out;
}

/** Open a sealed message; throws if it was tampered with or the associated data doesn't match. */
export function open(keys: Keys, ad: Uint8Array, sealed: Uint8Array): Uint8Array {
  if (sealed.length < OVERHEAD) throw new Error("sealed message too short");
  return xchacha20poly1305(keys.enc, sealed.subarray(0, NONCE_LEN), ad).decrypt(sealed.subarray(NONCE_LEN));
}

/** Associated data: the direction, the file and the chunk, so nothing can be moved or reflected. */
export const ad = {
  up: (fileId: string, index: number) => utf8(`${VERSION}|up|${fileId}|${index}`),
  down: (offerId: string, index: number) => utf8(`${VERSION}|down|${offerId}|${index}`),
  request: (client: string, seq: bigint) => utf8(`${VERSION}|req|${client}|${seq}`),
  response: (client: string, seq: bigint) => utf8(`${VERSION}|res|${client}|${seq}`),
};

/** The MAC of one request: who, which request, and what (method + path). */
export function requestMac(keys: Keys, client: string, seq: bigint, method: string, path: string): Uint8Array {
  const msg = utf8(`${VERSION}|auth|${client}|${seq}|${method}|${path}`);
  return hmac(sha256, keys.auth, msg).subarray(0, MAC_LEN);
}

export function authHeader(keys: Keys, client: string, seq: bigint, method: string, path: string): string {
  return `Tugboat ${client}.${seq}.${b64(requestMac(keys, client, seq, method, path))}`;
}

/**
 * A stable id for a picked file, so picking the same photo again after Safari reloaded the page
 * resumes it instead of starting over. Keyed, so it says nothing about the file to the network.
 */
export function fileId(keys: Keys, name: string, size: number, lastModified: number): string {
  const msg = utf8(`${VERSION}|file|${name}|${size}|${lastModified}`);
  return b64(hmac(sha256, keys.auth, msg).subarray(0, 16));
}

/** A random 128-bit id (the client id). */
export function randomId(): string {
  return b64(randomBytes(16));
}

export function b64(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function unb64(s: string): Uint8Array | null {
  if (!/^[A-Za-z0-9_-]*$/.test(s)) return null;
  try {
    const bin = atob(s.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - (s.length % 4)) % 4));
    return Uint8Array.from(bin, (c) => c.charCodeAt(0));
  } catch {
    return null;
  }
}
