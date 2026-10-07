// Developer tools: Settings › Developer tools state and the "send this text?" card. The bridge,
// the checks and the sending are all in Rust (src-tauri/src/devtools); this mirrors its status
// and passes the person's clicks back.

import { defineStore } from "pinia";
import { ref } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, errorMessage, on } from "../lib/ipc";
import type { DevToolsConfirm, DevToolsStatus } from "../types/protocol";

export const useDevToolsStore = defineStore("devtools", () => {
  const status = ref<DevToolsStatus | null>(null);
  /** A text waiting for Send / Don't send. */
  const confirm = ref<DevToolsConfirm | null>(null);
  const working = ref(false);
  const error = ref<string | null>(null);

  let teardown: UnlistenFn[] = [];
  let started = false;

  function apply(s: DevToolsStatus) {
    status.value = s;
    confirm.value = s.pending;
  }

  async function init() {
    if (started) return;
    started = true;
    teardown.push(
      await on("devtools-status", apply),
      await on("devtools-confirm", (c) => (confirm.value = c)),
    );
    // After a window reload a text may still be waiting: show its card again.
    void refresh();
  }

  function dispose() {
    for (const off of teardown) off();
    teardown = [];
    started = false;
  }

  async function refresh() {
    try {
      apply(await api.devtoolsStatus());
    } catch {
      /* the next event will tell */
    }
  }

  async function run(f: () => Promise<DevToolsStatus>): Promise<boolean> {
    working.value = true;
    error.value = null;
    try {
      apply(await f());
      return true;
    } catch (e) {
      error.value = errorMessage(e);
      return false;
    } finally {
      working.value = false;
    }
  }

  const setEnabled = (on: boolean) => run(() => api.devtoolsSetEnabled(on));
  const setPermission = (key: string, on: boolean) => run(() => api.devtoolsSetPermission(key, on));
  const revoke = () => run(() => api.devtoolsRevoke());
  const setOnPath = (on: boolean) => run(() => api.devtoolsSetOnPath(on));

  /**
   * Send (true) or Don't send for the card with this id. Ignored unless it's the card showing
   * now, so a click meant for one card can never answer the next. The card closes at once;
   * Rust also ignores anything but the question it's waiting on.
   */
  async function answer(id: number, send: boolean) {
    const c = confirm.value;
    if (!c || c.id !== id) return;
    confirm.value = null;
    try {
      await api.devtoolsConfirm(id, send);
    } catch {
      /* it expires as not sent */
    }
  }

  return { status, confirm, working, error, init, dispose, refresh, setEnabled, setPermission, revoke, setOnPath, answer };
});
