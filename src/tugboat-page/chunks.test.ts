import { describe, expect, it } from "vitest";
import {
  CHUNK,
  canRetryUpload,
  chunkBounds,
  chunkCount,
  formatSize,
  isFatal,
  messageFor,
  missing,
  nextSeq,
  receivedBytes,
  retryDelay,
} from "./chunks";

describe("chunks", () => {
  it("cuts files like the PC does", () => {
    expect(chunkCount(0)).toBe(1);
    expect(chunkCount(1)).toBe(1);
    expect(chunkCount(CHUNK)).toBe(1);
    expect(chunkCount(CHUNK + 1)).toBe(2);
    expect(chunkBounds(1, CHUNK + 10)).toEqual([CHUNK, CHUNK + 10]);
    expect(chunkBounds(0, 0)).toEqual([0, 0]);
  });

  it("works out what's left after a pause", () => {
    expect(missing(6, [])).toEqual([0, 1, 2, 3, 4, 5]);
    expect(missing(6, [[0, 3]])).toEqual([3, 4, 5]);
    expect(missing(6, [[0, 2], [4, 5]])).toEqual([2, 3, 5]);
    expect(missing(3, [[0, 3]])).toEqual([]);
    // Out-of-range ranges from a confused server don't break it.
    expect(missing(2, [[-1, 9]])).toEqual([]);
  });

  it("counts bytes already sent", () => {
    const size = 2 * CHUNK + 100;
    expect(receivedBytes(size, [])).toBe(0);
    expect(receivedBytes(size, [[0, 1]])).toBe(CHUNK);
    expect(receivedBytes(size, [[2, 3]])).toBe(100);
    expect(receivedBytes(size, [[0, 3]])).toBe(size);
  });
});

describe("sequence numbers", () => {
  it("follow the clock but never go backwards", () => {
    const t = 1_790_000_000_000;
    const a = nextSeq(0n, t);
    expect(a).toBe(BigInt(t) * 1024n);
    const b = nextSeq(a, t); // same millisecond
    expect(b).toBe(a + 1n);
    const c = nextSeq(b, t - 60_000); // the clock stepped back
    expect(c).toBe(b + 1n);
    expect(nextSeq(c, t + 1) > c).toBe(true);
  });
});

describe("retries and messages", () => {
  it("backs off up to 15 s", () => {
    expect(retryDelay(0)).toBe(1000);
    expect(retryDelay(3)).toBe(8000);
    expect(retryDelay(10)).toBe(15000);
  });

  it("knows which errors are final", () => {
    expect(isFatal("closed")).toBe(true);
    expect(isFatal("in-use")).toBe(true);
    expect(isFatal("network")).toBe(false);
    expect(isFatal("bad-chunk")).toBe(false);
  });

  it("speaks plainly", () => {
    expect(messageFor("closed")).toMatch(/scan the new code/);
    expect(messageFor("network")).toMatch(/same Wi-Fi/);
    expect(messageFor("anything")).toBe("Something went wrong. Try again.");
  });

  it("formats sizes", () => {
    expect(formatSize(512)).toBe("512 B");
    expect(formatSize(1536)).toBe("1.5 KB");
    expect(formatSize(412_000_000)).toBe("393 MB");
    expect(formatSize(3 * 1024 ** 3)).toBe("3.0 GB");
  });
});

describe("retrying a failed upload", () => {
  it("offers Try again for a dropped connection or a full disk", () => {
    expect(canRetryUpload("network")).toBe(true);
    expect(canRetryUpload("no-space")).toBe(true);
    expect(canRetryUpload("error")).toBe(true);
  });

  it("doesn't when only a new scan or a smaller file helps", () => {
    for (const code of ["too-big", "closed", "unauthorized", "in-use"]) expect(canRetryUpload(code)).toBe(false);
  });

  it("points to a new scan when the PC can't be reached", () => {
    expect(messageFor("network")).toMatch(/scan the new code on your PC/);
  });
});
