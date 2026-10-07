// Talks to tug on the PC: every request signed, every body sealed (see crypto.ts). Uploads go
// chunk by chunk straight from the picked File (never the whole file in memory) and pick up
// where they left off after Safari pauses the page.

import { ad, authHeader, fileId, open, seal, type Keys } from "./crypto";
import { CHUNK, MAX_DOWNLOAD, PARALLEL, chunkBounds, isFatal, missing, nextSeq, receivedBytes, retryDelay } from "./chunks";

export interface PageOffer {
  id: string;
  name: string;
  size: number;
  chunkSize: number;
  chunks: number;
  type: string;
}

export interface PageState {
  offers: PageOffer[];
  text: { id: number; text: string } | null;
}

interface UploadReply {
  received: Array<[number, number]>;
  chunks: number;
  savedAs: string | null;
}

/** A failed request: an error code from the PC, or "network" when it couldn't be reached. */
export class DropError extends Error {
  constructor(public code: string) {
    super(code);
  }
}

/** What the page needs from the PC (a real client, or the dev preview's stand-in). */
export interface DropApi {
  state(): Promise<PageState>;
  sendText(text: string): Promise<void>;
  /** Send a file; resolves with the name it was saved under on the PC. */
  upload(file: File, onProgress: (sent: number) => void, signal: AbortSignal): Promise<string>;
  /** Tell the PC to drop a half-sent file. */
  cancel(file: File): Promise<void>;
  /** Fetch and decrypt a file on offer. */
  download(offer: PageOffer, onProgress: (got: number) => void): Promise<Blob>;
}

/** Small persistent values (client id, last sequence number); in memory if storage is blocked. */
const memory = new Map<string, string>();
function load(key: string): string | null {
  try {
    return localStorage.getItem(key) ?? memory.get(key) ?? null;
  } catch {
    return memory.get(key) ?? null;
  }
}
function save(key: string, value: string) {
  memory.set(key, value);
  try {
    localStorage.setItem(key, value);
  } catch {
    /* private mode: memory only */
  }
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Resolve once the page is on screen again (Safari pauses hidden pages anyway). */
function whenVisible(): Promise<void> {
  if (document.visibilityState === "visible") return Promise.resolve();
  return new Promise((resolve) => {
    const on = () => {
      if (document.visibilityState !== "visible") return;
      document.removeEventListener("visibilitychange", on);
      resolve();
    };
    document.addEventListener("visibilitychange", on);
  });
}

export class DropClient implements DropApi {
  private last: bigint;

  constructor(
    private keys: Keys,
    private client: string,
  ) {
    const stored = load("tugdrop.seq");
    this.last = stored && /^\d+$/.test(stored) ? BigInt(stored) : 0n;
  }

  private seq(): bigint {
    this.last = nextSeq(this.last, Date.now());
    save("tugdrop.seq", this.last.toString());
    return this.last;
  }

  /** One signed request. Throws DropError("network") if the PC can't be reached. */
  private async request(method: string, path: string, body?: Uint8Array, seq = this.seq()): Promise<Uint8Array> {
    let res: Response;
    try {
      res = await fetch(path, {
        method,
        headers: { Authorization: authHeader(this.keys, this.client, seq, method, path) },
        body: body as BodyInit | undefined,
        cache: "no-store",
      });
    } catch {
      throw new DropError("network");
    }
    if (!res.ok) {
      let code = "error";
      try {
        code = ((await res.json()) as { error?: string }).error ?? code;
      } catch {
        /* no body */
      }
      throw new DropError(code);
    }
    return new Uint8Array(await res.arrayBuffer());
  }

  /** A JSON call: the body sealed to this request, the reply opened against it. */
  private async call<T>(method: string, path: string, json?: unknown): Promise<T> {
    const seq = this.seq();
    const body = json === undefined ? undefined : seal(this.keys, ad.request(this.client, seq), new TextEncoder().encode(JSON.stringify(json)));
    const sealed = await this.request(method, path, body, seq);
    const plain = open(this.keys, ad.response(this.client, seq), sealed);
    return JSON.parse(new TextDecoder().decode(plain)) as T;
  }

  state(): Promise<PageState> {
    return this.call<PageState>("GET", "/api/state");
  }

  async sendText(text: string): Promise<void> {
    await this.call("POST", "/api/text", { text });
  }

  private idFor(file: File): string {
    return fileId(this.keys, file.name, file.size, file.lastModified);
  }

  async upload(file: File, onProgress: (sent: number) => void, signal: AbortSignal): Promise<string> {
    const id = this.idFor(file);
    let attempt = 0;
    // Failures that aren't the connection dropping (the PC couldn't write the file, say) get a
    // few tries, then the file is marked failed instead of retrying silently forever.
    let otherFailures = 0;
    for (;;) {
      if (signal.aborted) throw new DropError("cancelled");
      try {
        // Ask what's already there (nothing, the first time) and send the rest.
        const reply = await this.call<UploadReply>("POST", `/api/up/${id}`, { name: file.name, size: file.size, chunkSize: CHUNK });
        if (reply.savedAs) return reply.savedAs;
        let sent = receivedBytes(file.size, reply.received);
        onProgress(sent);
        const todo = missing(reply.chunks, reply.received);
        await pool(todo, PARALLEL, signal, async (i) => {
          const [start, end] = chunkBounds(i, file.size);
          const plain = new Uint8Array(await file.slice(start, end).arrayBuffer());
          await this.request("PUT", `/api/up/${id}/${i}`, seal(this.keys, ad.up(id, i), plain));
          sent += end - start;
          onProgress(sent);
          attempt = 0;
        });
        const done = await this.call<{ savedAs: string }>("POST", `/api/finish/${id}`);
        return done.savedAs;
      } catch (e) {
        const code = e instanceof DropError ? e.code : "error";
        if (signal.aborted || isFatal(code)) throw e instanceof DropError ? e : new DropError(code);
        if (code !== "network" && code !== "stale" && ++otherFailures > 5) throw new DropError(code);
        // A dropped connection (the phone locked, Safari paused us): wait, then resume.
        await whenVisible();
        await sleep(retryDelay(attempt++));
      }
    }
  }

  async cancel(file: File): Promise<void> {
    try {
      await this.request("DELETE", `/api/up/${this.idFor(file)}`);
    } catch {
      /* best effort: the PC cleans up when Drop closes anyway */
    }
  }

  async download(offer: PageOffer, onProgress: (got: number) => void): Promise<Blob> {
    if (offer.size > MAX_DOWNLOAD) throw new DropError("too-big");
    // One small Blob per chunk, so each decrypted chunk can be let go as soon as it's wrapped
    // (and the final Blob just stitches them) instead of holding every chunk plus a full copy.
    const parts: Blob[] = [];
    let got = 0;
    for (let i = 0; i < offer.chunks; i++) {
      let attempt = 0;
      for (;;) {
        try {
          const sealed = await this.request("GET", `/api/down/${offer.id}/${i}`);
          const plain = open(this.keys, ad.down(offer.id, i), sealed);
          parts.push(new Blob([plain as BlobPart]));
          got += plain.length;
          onProgress(got);
          break;
        } catch (e) {
          const code = e instanceof DropError ? e.code : "error";
          if (isFatal(code) || attempt >= 6) throw e instanceof DropError ? e : new DropError(code);
          await whenVisible();
          await sleep(retryDelay(attempt++));
        }
      }
    }
    return new Blob(parts, { type: offer.type || "application/octet-stream" });
  }
}

/** Run `work` over `items` with at most `limit` at once; the first failure stops new work. */
async function pool<T>(items: T[], limit: number, signal: AbortSignal, work: (item: T) => Promise<void>): Promise<void> {
  let next = 0;
  let failed: unknown = null;
  const lane = async () => {
    while (next < items.length && failed === null) {
      if (signal.aborted) throw new DropError("cancelled");
      const item = items[next++];
      try {
        await work(item);
      } catch (e) {
        failed ??= e;
      }
    }
  };
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, lane));
  if (failed !== null) throw failed;
}

const CLIENT_ID = /^[A-Za-z0-9_-]{16,43}$/;

/** The client id this browser uses for Drop on this PC address (shared by tabs, so a re-scan works). */
export function clientId(make: () => string): string {
  const stored = load("tugdrop.client");
  if (stored && CLIENT_ID.test(stored)) return stored;
  const id = make();
  save("tugdrop.client", id);
  return id;
}

/** Use the client id the PC put in the link (this phone's, carried across a network change). */
export function adoptClientId(id: string) {
  if (CLIENT_ID.test(id)) save("tugdrop.client", id);
}
