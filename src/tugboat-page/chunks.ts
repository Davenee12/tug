// Pure helpers for the phone page: cutting files into chunks, working out what's left to send
// after Safari paused the page, request sequence numbers, and the words for each error.

/** Chunk size for files the phone sends (the PC accepts 64 KB – 4 MB). */
export const CHUNK = 2 * 1024 * 1024;
/** Chunks of one file in flight at once. */
export const PARALLEL = 3;
/** Largest file the PC accepts from the phone. */
export const MAX_UPLOAD = 8 * 1024 ** 3;
/** Largest file the page will assemble in memory for saving. */
export const MAX_DOWNLOAD = 1024 ** 3;

/** Every file has at least one chunk (an empty file is one empty chunk), as on the PC. */
export function chunkCount(size: number, chunk = CHUNK): number {
  return Math.max(1, Math.ceil(size / chunk));
}

/** Byte range `[start, end)` of chunk `i`. */
export function chunkBounds(i: number, size: number, chunk = CHUNK): [number, number] {
  const start = i * chunk;
  return [start, Math.min(start + chunk, size)];
}

/** Chunk indexes not covered by the PC's `[start, end)` "received" ranges, in order. */
export function missing(chunks: number, received: Array<[number, number]>): number[] {
  const have = new Uint8Array(chunks);
  for (const [a, b] of received) for (let i = Math.max(0, a); i < Math.min(b, chunks); i++) have[i] = 1;
  const out: number[] = [];
  for (let i = 0; i < chunks; i++) if (!have[i]) out.push(i);
  return out;
}

/** Bytes already on the PC, from its "received" ranges. */
export function receivedBytes(size: number, received: Array<[number, number]>, chunk = CHUNK): number {
  let total = 0;
  for (const [a, b] of received) {
    for (let i = a; i < b; i++) {
      const [s, e] = chunkBounds(i, size, chunk);
      total += Math.max(0, e - s);
    }
  }
  return total;
}

/**
 * The next request sequence number: from the clock (`ms × 1024`) so a reloaded page carries on
 * above the last one, but never below `last + 1` even if the phone's clock steps back.
 */
export function nextSeq(last: bigint, nowMs: number): bigint {
  const fromClock = BigInt(Math.floor(nowMs)) * 1024n;
  return fromClock > last ? fromClock : last + 1n;
}

/** Back off 1, 2, 4, 8 s… up to 15 s between retries of a dropped connection. */
export function retryDelay(attempt: number): number {
  return Math.min(1000 * 2 ** Math.max(0, attempt), 15_000);
}

/** Errors that retrying won't fix. Anything else (a dropped connection) is retried. */
export function isFatal(code: string): boolean {
  return ["closed", "unauthorized", "in-use", "too-big", "no-space", "changed", "not-found"].includes(code);
}

/** A failed upload worth offering "Try again" for: not one the PC refused for good (too big, or
 * Tugboat closed / the link expired / open elsewhere, where only a new scan helps). */
export function canRetryUpload(code: string): boolean {
  return !["too-big", "closed", "unauthorized", "in-use"].includes(code);
}

/** What to tell the person, for an error code from the PC (or "network" when it can't be reached). */
export function messageFor(code: string): string {
  switch (code) {
    case "network":
      return "Can't reach your PC. Make sure Tugboat is still open in tug and you're on the same Wi-Fi. If your PC changed networks, scan the new code on your PC.";
    case "closed":
      return "Tugboat was closed on your PC. Open Tugboat in tug and scan the new code.";
    case "unauthorized":
      return "This link has expired. Open Tugboat in tug and scan the code again.";
    case "in-use":
      return "Tugboat is already open on another device. Close Tugboat in tug and open it again for a new code.";
    case "too-big":
      return "This file is too big to send.";
    case "no-space":
      return "Your PC is out of space.";
    case "changed":
      return "This file changed on your PC. Send it again from tug.";
    case "not-found":
      return "This file is no longer offered.";
    default:
      return "Something went wrong. Try again.";
  }
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let v = bytes / 1024;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[u]}`;
}
