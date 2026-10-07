// Tugboat's panel state: the session status from Rust, whether the panel is open, and files
// dragged onto tug's window. The work (server, crypto, files) is all in src-tauri/src/tugboat.

import { defineStore } from "pinia";
import { ref } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, on } from "../lib/ipc";
import { TUGBOAT_OFF } from "../lib/tugboat";
import type { TugboatSkipped, TugboatStatus } from "../types/protocol";

export const useTugboatStore = defineStore("tugboat", () => {
  /** The Tugboat panel is showing. */
  const open = ref(false);
  const status = ref<TugboatStatus>({ ...TUGBOAT_OFF });
  /** Starting a session (finding the network, opening the port). */
  const starting = ref(false);
  const error = ref<string | null>(null);
  /** When the current code first appeared, for the "Can't connect?" help. */
  const shownAt = ref<number | null>(null);
  /** Files are being dragged over tug's window. */
  const dragging = ref(false);

  let teardown: UnlistenFn[] = [];
  let started = false;

  function apply(s: TugboatStatus) {
    const hadCode = status.value.url !== null;
    status.value = s;
    if (s.url && (!hadCode || shownAt.value === null)) shownAt.value = Date.now();
    if (!s.url) shownAt.value = null;
  }

  /** Listen for status, and for files dragged onto the window (they open Tugboat, offered to the phone). */
  async function init(onSkipped: (skipped: TugboatSkipped[]) => void) {
    if (started) return;
    started = true;
    teardown.push(await on("tugboat-status", apply));
    // After a window reload Tugboat may still be running: show it again rather than leave it unseen.
    try {
      const s = await api.tugboatStatus();
      apply(s);
      if (s.phase !== "off") open.value = true;
    } catch {
      /* the next event will tell */
    }
    try {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      teardown.push(
        await getCurrentWebview().onDragDropEvent((e) => {
          const p = e.payload;
          if (p.type === "enter") dragging.value = p.paths.length > 0;
          else if (p.type === "leave") dragging.value = false;
          else if (p.type === "drop") {
            dragging.value = false;
            if (p.paths.length) void offer(p.paths).then(onSkipped);
          }
        }),
      );
    } catch {
      // Not in Tauri (the browser preview): no native file drops.
    }
  }

  function dispose() {
    for (const off of teardown) off();
    teardown = [];
    started = false;
  }

  async function start() {
    if (starting.value) return;
    starting.value = true;
    error.value = null;
    try {
      apply(await api.tugboatStart());
    } catch (e) {
      error.value = typeof e === "string" ? e : "Couldn't start Tugboat.";
    } finally {
      starting.value = false;
    }
  }

  /** Open the panel and the session (sidebar button, Ctrl+K "tugboat"). */
  async function show() {
    open.value = true;
    await start();
  }

  /** Close the panel: the server stops and the code stops working. */
  async function close() {
    open.value = false;
    dragging.value = false;
    try {
      await api.tugboatStop();
    } catch {
      /* already stopped */
    }
    apply({ ...TUGBOAT_OFF, folder: status.value.folder });
  }

  /** Offer files to the phone (dragged in, or picked), opening Tugboat first if it's closed. */
  async function offer(paths: string[]): Promise<TugboatSkipped[]> {
    open.value = true;
    try {
      return await api.tugboatOfferFiles(paths);
    } catch (e) {
      error.value = typeof e === "string" ? e : "Couldn't offer those files.";
      return [];
    }
  }

  async function pickFiles(): Promise<TugboatSkipped[]> {
    try {
      return await api.tugboatPickFiles();
    } catch {
      return [];
    }
  }

  async function removeOffer(id: string) {
    await api.tugboatRemoveOffer(id).catch(() => undefined);
  }

  async function sendText(text: string): Promise<boolean> {
    try {
      await api.tugboatSendText(text);
      return true;
    } catch {
      return false;
    }
  }

  async function openFolder(path: string | null = null) {
    await api.tugboatOpenFolder(path).catch(() => undefined);
  }

  return {
    open,
    status,
    starting,
    error,
    shownAt,
    dragging,
    init,
    dispose,
    start,
    show,
    close,
    offer,
    pickFiles,
    removeOffer,
    sendText,
    openFolder,
  };
});
