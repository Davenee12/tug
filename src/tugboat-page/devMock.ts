// Dev-only stand-in for the PC, so the phone page can be reviewed in a browser:
// `npm run dev:tugboat` → http://localhost:1430/?mock (add &busy for files on offer and PC text,
// &offline for the "can't reach your PC" banner, &closed for the ended state, &game for tug's game
// asking for a controller: the controller opens, and the PC "hits" the boat every 5 s; &clip for the
// PC's clipboard arriving as text, &clipimage as a picture).

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
  const state: PageState = {
    offers,
    text: busy ? { id: 1, text: "https://maps.app.goo.gl/x7Qp — meet at the north entrance" } : null,
    game: params.has("game"),
    clip: params.has("clip")
      ? { id: 1, text: "221B Example Street, flat 2. Door code 4821.\nhttps://example.com/booking/7Q2X", image: null }
      : params.has("clipimage")
        ? { id: 1, text: null, image: { id: "clip", name: "Clipboard picture.png", size: 48_213, chunkSize: 2 << 20, chunks: 1, type: "image/png" } }
        : null,
  };
  const started = Date.now();
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
    async clipAck(id) {
      await sleep(200);
      console.debug("[mock] the PC hears the page has clipboard", id);
      // Like the PC: once it has heard, it stops handing the clipboard out.
      state.clip = null;
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
      if (offer.type === "image/png") return previewPicture();
      return new Blob(["Tugboat preview"], { type: offer.type });
    },
    async pad(input, probe) {
      await sleep(15);
      if (!state.game) throw new TugboatError("no-game");
      console.debug("[mock] pad", input, probe);
      return { paused: false, hits: Math.floor((Date.now() - started) / 5000) };
    },
  };
}

/** A small made-up picture for the clipboard preview (a coral square with a cream stripe). */
function previewPicture(): Promise<Blob> {
  const canvas = document.createElement("canvas");
  canvas.width = 320;
  canvas.height = 200;
  const g = canvas.getContext("2d");
  if (g) {
    g.fillStyle = "#cc785c";
    g.fillRect(0, 0, 320, 200);
    g.fillStyle = "#faf9f5";
    g.fillRect(0, 80, 320, 40);
  }
  return new Promise((resolve) => canvas.toBlob((b) => resolve(b ?? new Blob()), "image/png"));
}