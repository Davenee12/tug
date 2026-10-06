// Dev-only stand-in for the PC, so the phone page can be reviewed in a browser:
// `npm run dev:drop` → http://localhost:1430/?mock (add &busy for files on offer and PC text,
// &offline for the "can't reach your PC" banner, &closed for the ended state).

import { DropError, type DropApi, type PageOffer, type PageState } from "./client";

export function mockApi(): DropApi {
  const params = new URLSearchParams(location.search);
  const busy = params.has("busy");
  const offers: PageOffer[] = busy
    ? [
        { id: "a", name: "Boarding pass.pdf", size: 184_320, chunkSize: 2 << 20, chunks: 1, type: "application/pdf" },
        { id: "b", name: "Holiday slideshow.mp4", size: 412_000_000, chunkSize: 2 << 20, chunks: 197, type: "video/mp4" },
      ]
    : [];
  const state: PageState = { offers, text: busy ? { id: 1, text: "https://maps.app.goo.gl/x7Qp — meet at the north entrance" } : null };
  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
  return {
    async state() {
      await sleep(150);
      if (params.has("closed")) throw new DropError("closed");
      if (params.has("offline")) throw new DropError("network");
      return state;
    },
    async sendText() {
      await sleep(400);
    },
    async upload(file, onProgress, signal) {
      // Pretend to send at ~40 MB/s.
      for (let sent = 0; sent < file.size; sent += 4 << 20) {
        if (signal.aborted) throw new DropError("cancelled");
        onProgress(sent);
        await sleep(100);
      }
      return file.name;
    },
    async cancel() {},
    async download(offer, onProgress) {
      for (let got = 0; got < offer.size; got += Math.max(offer.size / 10, 1)) {
        onProgress(got);
        await sleep(120);
      }
      return new Blob(["tug drop preview"], { type: offer.type });
    },
  };
}
