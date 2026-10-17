// Automatic updates: mirrors the Rust status (src-tauri/src/updater) for the Update button, its
// confirm and Settings › About, and passes the person's clicks back. The words come from lib/update.

import { defineStore } from "pinia";
import { computed, ref } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, errorMessage, on } from "../lib/ipc";
import { isCurrent } from "../lib/ordering";
import {
  CLOSED,
  followStatus,
  heldBack,
  openConfirm,
  parseResume,
  pressRestart,
  restartFailed,
  resumeFor,
  updateAbout,
  updatePill,
  type Confirm,
} from "../lib/update";
import type { UpdateStatus } from "../types/protocol";
import { useTugStore } from "./tug";

export const useUpdateStore = defineStore("update", () => {
  const status = ref<UpdateStatus | null>(null);
  const confirm = ref<Confirm>(CLOSED);
  const tug = useTugStore();
  const pill = computed(() => updatePill(status.value));
  const about = computed(() => updateAbout(status.value));

  let teardown: UnlistenFn[] = [];
  let started = false;

  /** Keep the newest status (an answer can land after a newer event; lib/ordering). */
  function apply(next: UpdateStatus) {
    if (!isCurrent(next, status.value)) return;
    status.value = next;
    confirm.value = followStatus(confirm.value, next);
  }

  async function init() {
    if (started) return;
    started = true;
    try {
      teardown.push(await on("update-status", apply));
      apply(await api.updateStatus());
    } catch {
      /* the next event will tell */
    }
    // Back from an update restart: reopen the screen the person left.
    try {
      const screen = parseResume(await api.updateTakeResume());
      if (screen?.view === "settings") tug.openSettings(screen.section);
      else if (screen) tug.view = screen.view;
    } catch {
      /* nothing saved */
    }
  }

  function dispose() {
    for (const off of teardown) off();
    teardown = [];
    started = false;
  }

  async function run(call: () => Promise<UpdateStatus>) {
    try {
      apply(await call());
    } catch (e) {
      tug.notify("error", errorMessage(e));
    }
  }

  const check = () => run(api.updateCheck);
  const download = () => run(api.updateDownload);
  const setAuto = (on: boolean) => run(() => api.updateSetAuto(on));

  async function openReleasePage() {
    try {
      await api.updateOpenReleasePage();
    } catch (e) {
      tug.notify("error", errorMessage(e));
    }
  }

  /** The Update button: the confirm on an installed copy, the download page on the portable exe. */
  function pressPill() {
    const p = pill.value;
    if (!p) return;
    if (p.action === "download") void openReleasePage();
    else confirm.value = openConfirm(status.value);
  }

  /** Restart now. tug restarts (and nothing comes back), or the confirm says what has to finish. */
  async function restart() {
    const next = pressRestart(confirm.value);
    if (next === confirm.value) return;
    confirm.value = next;
    try {
      const answer = await api.updateApply(resumeFor(tug.view, tug.settingsSection));
      apply(answer.status);
      if (answer.blocked) confirm.value = heldBack(confirm.value, answer.blocked);
    } catch (e) {
      // The failed status may already have closed the confirm; then the message goes to the toast.
      confirm.value = restartFailed(confirm.value, errorMessage(e));
      if (!confirm.value.open) tug.notify("error", errorMessage(e));
    }
  }

  function later() {
    confirm.value = CLOSED;
  }

  return { status, confirm, pill, about, init, dispose, check, download, setAuto, openReleasePage, pressPill, restart, later };
});
