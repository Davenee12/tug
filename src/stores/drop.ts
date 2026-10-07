// tug Drop's panel state: the session status from Rust, whether the panel is open, and files
// dragged onto tug's window. The work (server, crypto, files) is all in src-tauri/src/drop.

import { defineStore } from "pinia";
import { ref } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, on } from "../lib/ipc";
import { DROP_OFF } from "../lib/drop";
import type { DropSkipped, DropStatus } from "../types/protocol";

export const useDropStore = defineStore("drop", () => {
  /** The Drop panel is showing. */
  const open = ref(false);
  const status = ref<DropStatus>({ ...DROP_OFF });
  /** Starting a session (finding the network, opening the port). */
  const starting = ref(false);
  const error = ref<string | null>(null);
  /** When the current code first appeared, for the "Can't connect?" help. */
  const shownAt = ref<number | null>(null);
  /** Files are being dragged over tug's window. */
  const dragging = ref(false);

  let teardown: UnlistenFn[] = [];
  let started = false;

  function apply(s: DropStatus) {
    const hadCode = status.value.url !== null;
    status.value = s;
    if (s.url && (!hadCode || shownAt.value === null)) shownAt.value = Date.now();
    if (!s.url) shownAt.value = null;
  }

  /** Listen for status, and for files dragged onto the window (they open Drop, offered to the phone). */
  async function init(onSkipped: (skipped: DropSkipped[]) => void) {
    if (started) return;
    started = true;
    teardown.push(await on("drop-status", apply));
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
      apply(await api.dropStart());
    } catch (e) {
      error.value = typeof e === "string" ? e : "Couldn't start Drop.";
    } finally {
      starting.value = false;
    }
  }

  /** Open the panel and the session (sidebar button, Ctrl+K "drop"). */
  async function show() {
    open.value = true;
    await start();
  }

  /** Close the panel: the server stops and the code stops working. */
  async function close() {
    open.value = false;
    dragging.value = false;
    try {
      await api.dropStop();
    } catch {
      /* already stopped */
    }
    apply({ ...DROP_OFF, folder: status.value.folder });
  }

  /** Offer files to the phone (dragged in, or picked), opening Drop first if it's closed. */
  async function offer(paths: string[]): Promise<DropSkipped[]> {
    open.value = true;
    try {
      return await api.dropOfferFiles(paths);
    } catch (e) {
      error.value = typeof e === "string" ? e : "Couldn't offer those files.";
      return [];
    }
  }

  async function pickFiles(): Promise<DropSkipped[]> {
    try {
      return await api.dropPickFiles();
    } catch {
      return [];
    }
  }

  async function removeOffer(id: string) {
    await api.dropRemoveOffer(id).catch(() => undefined);
  }

  async function sendText(text: string): Promise<boolean> {
    try {
      await api.dropSendText(text);
      return true;
    } catch {
      return false;
    }
  }

  async function openFolder(path: string | null = null) {
    await api.dropOpenFolder(path).catch(() => undefined);
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
