// Send later (lib/scheduledTexts): texts waiting to be sent, saved in tug's settings on this PC, and
// the look every 15 seconds that sends what's due. A text comes off the saved list *before* it's
// sent, so even a crash mid-send can't send it twice; if nothing was recorded at all it comes back
// as held, for the person to Send now or Discard.

import { defineStore } from "pinia";
import { ref } from "vue";
import { api } from "../lib/ipc";
import { clockTime } from "../lib/format";
import { reminderId } from "../lib/reminders";
import { addScheduled, forThread, held, parseScheduled, plan, withoutScheduled, type ScheduledText } from "../lib/scheduledTexts";
import { useTugStore } from "./tug";

export const SCHEDULED_SETTING = "ui.scheduledTexts";
const CHECK_MS = 15_000;

export interface SendTarget {
  address: string;
  name: string;
  threadKey: string;
}

export const useScheduledTextsStore = defineStore("scheduledTexts", () => {
  const list = ref<ScheduledText[]>([]);
  let canSave = false;
  let loaded: Promise<void> | null = null;
  let timer: ReturnType<typeof setInterval> | undefined;
  /** One look at a time: a send can take a while, and the next look mustn't send it again. */
  let checking = false;

  function load(): Promise<void> {
    loaded ??= Promise.resolve()
      .then(() => api.getSettings())
      .then((raw) => {
        list.value = parseScheduled(raw[SCHEDULED_SETTING]);
        canSave = true;
      })
      .catch(() => undefined);
    return loaded;
  }

  function save() {
    if (!canSave) return;
    void api.setSetting(SCHEDULED_SETTING, JSON.stringify(list.value)).catch(() => undefined);
  }

  /** Send one (already off the list); a send that recorded nothing comes back held. */
  async function send(t: ScheduledText) {
    const tug = useTugStore();
    const outcome = await tug.sendMessage(t.address, t.text);
    if (outcome === "error") {
      list.value = addScheduled(list.value, { ...t, state: "held" }) ?? list.value;
      save();
      return;
    }
    // "failed" is already a "Not sent" row with Retry in the conversation (sendMessage said so).
    if (outcome !== "failed") tug.notify("info", `Sent your scheduled text to ${t.name}.`);
  }

  async function check(now = Date.now()) {
    if (checking) return;
    checking = true;
    try {
      const tug = useTugStore();
      const { send: due, hold } = plan(list.value, now, tug.status.services.messages);
      if (hold.length) {
        list.value = held(list.value, hold.map((t) => t.id));
        save();
        for (const t of hold) {
          tug.notify("info", `Your text to ${t.name} wasn't sent: it was due at ${clockTime(new Date(t.at))}.`, {
            label: "Open",
            run: () => tug.openThread(t.threadKey),
          });
        }
      }
      if (due.length) {
        // Off the saved list first: at most once, whatever happens next.
        for (const t of due) list.value = withoutScheduled(list.value, t.id);
        save();
        for (const t of due) await send(t);
      }
    } finally {
      checking = false;
    }
  }

  /** At startup, once tug's history has loaded, then every 15 seconds. */
  async function init() {
    await load();
    await check();
    if (timer === undefined) timer = setInterval(() => void check(), CHECK_MS);
  }

  function dispose() {
    clearInterval(timer);
    timer = undefined;
  }

  /** Schedule `text` to `target` at `at`; false if too many are already waiting. */
  async function schedule(target: SendTarget, text: string, at: number): Promise<boolean> {
    await load();
    const now = Date.now();
    const next = addScheduled(list.value, {
      id: reminderId(now),
      ...target,
      text,
      at,
      createdAt: now,
      state: "waiting",
    });
    if (!next) return false;
    list.value = next;
    save();
    return true;
  }

  function discard(id: string) {
    list.value = withoutScheduled(list.value, id);
    save();
  }

  /** Send a waiting or held text right away (the person pressed Send now). */
  async function sendNow(id: string) {
    const t = list.value.find((x) => x.id === id);
    if (!t) return;
    list.value = withoutScheduled(list.value, id);
    save();
    await send({ ...t, state: "waiting" });
  }

  function inThread(threadKey: string): ScheduledText[] {
    return forThread(list.value, threadKey);
  }

  return { list, load, init, dispose, check, schedule, discard, sendNow, inThread };
});
