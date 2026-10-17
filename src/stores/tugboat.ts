// Tugboat's panel state: the session status from Rust, whether the panel is open, and files
// dragged over tug's window (only to show the overlay: Rust takes the drop itself, so no path
// ever comes from page script). The work (server, crypto, files) is all in src-tauri/src/tugboat.

import { defineStore } from "pinia";
import { ref } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, on } from "../lib/ipc";
import { TUGBOAT_OFF, clipEventMessage, clipNeedsPanel, clipPhone } from "../lib/tugboat";
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
  /** Reading and sending the clipboard right now: a second press meanwhile does nothing. */
  const clipSending = ref(false);

  let teardown: UnlistenFn[] = [];
  let started = false;
  let say: (kind: "info" | "error", text: string) => void = () => undefined;

  function apply(s: TugboatStatus) {
    const hadCode = status.value.url !== null;
    status.value = s;
    // Stopped because tug's window hid to the tray: don't greet the user with a dead panel.
    if (s.phase === "off" && s.ended === "hidden") open.value = false;
    if (s.url && (!hadCode || shownAt.value === null)) shownAt.value = Date.now();
    if (!s.url) shownAt.value = null;
  }

  /**
   * Listen for status; for files dropped onto the window (Rust opens Tugboat and offers them, then
   * says which were skipped); for text from the phone landing on the clipboard; and for the shared
   * clipboard (the tray's "Send clipboard to iPhone", and the phone saying it has it). The say handler shows
   * a short note at the bottom of the window.
   */
  async function init(handlers: {
    onSkipped: (skipped: TugboatSkipped[]) => void;
    onText: (ok: boolean) => void;
    say: (kind: "info" | "error", text: string) => void;
  }) {
    if (started) return;
    started = true;
    say = handlers.say;
    teardown.push(
      await on("tugboat-status", apply),
      await on("tugboat-dropped", (skipped) => {
        open.value = true;
        handlers.onSkipped(skipped);
      }),
      await on("tugboat-text", ({ ok }) => handlers.onText(ok)),
      await on("tugboat-send-clipboard", () => void sendClipboard()),
      await on("tugboat-clip", (e) => {
        const m = clipEventMessage(e, status.value.phone);
        if (m) say(m.kind, m.text);
      }),
    );
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
          else if (p.type === "leave" || p.type === "drop") dragging.value = false;
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

  /** "Copy link": done in Rust, kept out of clipboard history and sync. */
  async function copyLink(): Promise<boolean> {
    try {
      await api.tugboatCopyLink();
      return true;
    } catch {
      return false;
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

  /**
   * "Send clipboard to iPhone" (tray, Ctrl+K, the panel). One press, one send: Rust reads the
   * clipboard now and holds it for the phone for up to 2 minutes, opening Tugboat if it's off.
   * Nothing here says "sent": the "tugboat-clip" event does, once the phone has it.
   */
  async function sendClipboard() {
    if (clipSending.value) return;
    clipSending.value = true;
    try {
      const s = await api.tugboatSendClipboard();
      apply(s);
      if (clipNeedsPanel(s)) open.value = true;
      else say("info", `Sending your clipboard to your ${clipPhone(s.phone)}…`);
    } catch (e) {
      // Rust's own words ("Your clipboard is empty…"), or a plain fallback.
      say("error", typeof e === "string" ? e : "Couldn't send your clipboard.");
    } finally {
      clipSending.value = false;
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
    pickFiles,
    copyLink,
    removeOffer,
    sendText,
    clipSending,
    sendClipboard,
    openFolder,
  };
});
