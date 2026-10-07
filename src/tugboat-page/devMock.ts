// Dev-only stand-in for the PC, so the phone page can be reviewed in a browser:
// `npm run dev:tugboat` → http://localhost:1430/?mock (add &busy for files on offer and PC text,
// &offline for the "can't reach your PC" banner, &closed for the ended state).

import { TugboatError, type TugboatApi, type PageOffer, type PageState } from "./client";

export function mockApi(): TugboatApi {
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
      if (params.has("closed")) throw new TugboatError("closed");
      if (params.has("offline")) throw new TugboatError("network");
      return state;
    },
    async sendText() {
      await sleep(400);
    },
    async upload(file, onProgress, signal) {
      // Pretend to send at ~40 MB/s.
      for (let sent = 0; sent < file.size; sent += 4 << 20) {
        if (signal.aborted) throw new TugboatError("cancelled");
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
      return new Blob(["Tugboat preview"], { type: offer.type });
    },
  };
}
