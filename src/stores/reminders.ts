// Remind me (lib/reminders): the reminders themselves, saved in tug's settings on this PC, and the
// check that brings each one back when it's due. A look every 15 seconds rather than one long
// timer, so a PC that slept through a reminder still brings it back as soon as it wakes; one that
// came due while tug was closed comes back as tug starts (`init`, once its history has loaded).

import { defineStore } from "pinia";
import { ref } from "vue";
import { api } from "../lib/ipc";
import {
  addReminder,
  parseReminders,
  reminderFor,
  reminderId,
  removeReminder,
  setReminder,
  splitDue,
  type Reminder,
} from "../lib/reminders";
import { useTugStore } from "./tug";

export const REMINDERS_SETTING = "ui.reminders";
const CHECK_MS = 15_000;

/** What a row offers a reminder for. */
export interface RemindTarget {
  title: string;
  body: string;
  notificationIds: number[];
  threadKey: string | null;
}

export const useRemindersStore = defineStore("reminders", () => {
  const list = ref<Reminder[]>([]);
  /** Only once the saved list was read, so a failed read can't wipe it. */
  let canSave = false;
  let loaded: Promise<void> | null = null;
  let timer: ReturnType<typeof setInterval> | undefined;

  function load(): Promise<void> {
    loaded ??= Promise.resolve()
      .then(() => api.getSettings())
      .then((raw) => {
        // Anything set while this was loading is kept.
        list.value = list.value.reduce(addReminder, parseReminders(raw[REMINDERS_SETTING]));
        canSave = true;
      })
      .catch(() => {
        // Ask again next time rather than never saving for the rest of the session.
        loaded = null;
      });
    return loaded;
  }

  function save() {
    if (!canSave) return;
    void api.setSetting(REMINDERS_SETTING, JSON.stringify(list.value)).catch(() => undefined);
  }

  /** Bring back everything that's due, each once. */
  function check(now = Date.now()) {
    if (!canSave) void load(); // the first read failed: the saved ones still need bringing in
    const { due, pending } = splitDue(list.value, now);
    if (due.length === 0) return;
    list.value = pending;
    save();
    const tug = useTugStore();
    for (const r of due) void tug.showReminder(r);
  }

  /** At startup, once tug's history has loaded: anything that came due while tug was closed, then the regular look. */
  async function init() {
    await load();
    check();
    if (timer === undefined) timer = setInterval(() => check(), CHECK_MS);
  }

  function dispose() {
    clearInterval(timer);
    timer = undefined;
  }

  /**
   * Remind me about this row at `at`; replaces a reminder it already had. "full" when 50 are
   * already waiting, "unsaved" when tug's settings can't be read (it wouldn't survive a restart).
   */
  async function remind(target: RemindTarget, at: number): Promise<"ok" | "full" | "unsaved"> {
    await load();
    if (!canSave) return "unsaved";
    const now = Date.now();
    const latestId = target.notificationIds.length ? Math.max(...target.notificationIds) : null;
    const next = setReminder(list.value, target, {
      id: reminderId(now),
      at,
      title: target.title,
      body: target.body,
      notificationId: latestId,
      threadKey: target.threadKey,
      createdAt: now,
    });
    if (!next) return "full";
    list.value = next;
    save();
    return "ok";
  }

  function cancel(id: string) {
    list.value = removeReminder(list.value, id);
    save();
  }

  /** The row's pending reminder, if it has one. */
  function forTarget(target: Pick<RemindTarget, "threadKey" | "notificationIds">): Reminder | null {
    return reminderFor(list.value, target);
  }

  return { list, load, init, dispose, check, remind, cancel, forTarget };
});
