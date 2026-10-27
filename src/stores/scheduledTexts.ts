// Send later (lib/scheduledTexts): texts waiting to be sent, saved in tug's settings on this PC, and
// the look every 15 seconds that sends what's due. Due texts go one at a time, each coming off the
// saved list just *before* it's sent, so even a crash mid-send can't send it twice and can only
// lose the one in flight; if nothing was recorded at all it comes back as held, for the person to
// Send now or Discard. Each is checked against the 30-minute limit again right before it goes.

import { defineStore } from "pinia";
import { ref } from "vue";
import { api } from "../lib/ipc";
import { clockTime } from "../lib/format";
import { reminderId } from "../lib/reminders";
import { addScheduled, forConversation, held, parseScheduled, plan, tooLate, withoutScheduled, type ScheduledText } from "../lib/scheduledTexts";
import { useTugStore } from "./tug";

export const SCHEDULED_SETTING = "ui.scheduledTexts";
const CHECK_MS = 15_000;
/** A send that hasn't answered in this long stops holding up the others (it may still finish). */
const SEND_WAIT_MS = 2 * 60_000;

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
        // Anything already in memory (back from a send) is kept alongside what was saved.
        list.value = list.value.reduce((all, t) => addScheduled(all, t, true) ?? all, parseScheduled(raw[SCHEDULED_SETTING]));
        canSave = true;
      })
      .catch(() => {
        // Ask again next time rather than never saving (or sending) for the rest of the session.
        loaded = null;
      });
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
      list.value = addScheduled(list.value, { ...t, state: "held" }, true) ?? list.value;
      save();
      return;
    }
    // "failed" is already a "Not sent" row with Retry in the conversation (sendMessage said so);
    // "unconfirmed" shows there as "May have sent".
    if (outcome === "sent") tug.notify("info", `Sent your scheduled text to ${t.name}.`);
  }

  function holdOne(t: ScheduledText) {
    const tug = useTugStore();
    list.value = held(list.value, [t.id]);
    save();
    tug.notify("info", `Your text to ${t.name} wasn't sent: it was due at ${clockTime(new Date(t.at))}.`, {
      label: "Open",
      run: () => tug.startConversation(t.address, t.name),
    });
  }

  async function check(now = Date.now()) {
    if (checking) return;
    checking = true;
    try {
      if (!canSave) await load(); // the first read failed: the saved texts still need bringing in
      if (!canSave) return;
      const tug = useTugStore();
      const { send: due, hold } = plan(list.value, now, tug.status.services.messages);
      for (const t of hold) holdOne(t);
      for (const t of due) {
        // Still waiting (not sent now or cancelled meanwhile), and still on time: an earlier send
        // or a busy phone can take a while.
        const current = list.value.find((x) => x.id === t.id);
        if (!current || current.state !== "waiting") continue;
        if (tooLate(current, Date.now())) {
          holdOne(current);
          continue;
        }
        // Off the saved list first: at most once, whatever happens next.
        list.value = withoutScheduled(list.value, t.id);
        save();
        let waited: ReturnType<typeof setTimeout> | undefined;
        await Promise.race([send(current), new Promise((r) => (waited = setTimeout(r, SEND_WAIT_MS)))]);
        clearTimeout(waited);
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

  /**
   * Schedule `text` to `target` at `at`. "full" when 50 are already waiting, "unsaved" when tug's
   * settings can't be read (it wouldn't survive a restart, so it isn't taken).
   */
  async function schedule(target: SendTarget, text: string, at: number): Promise<"ok" | "full" | "unsaved"> {
    await load();
    if (!canSave) return "unsaved";
    const now = Date.now();
    const next = addScheduled(list.value, {
      id: reminderId(now),
      ...target,
      text,
      at,
      createdAt: now,
      state: "waiting",
    });
    if (!next) return "full";
    list.value = next;
    save();
    return "ok";
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

  /** A conversation's scheduled texts, by its key or its numbers (lib `forConversation`). */
  function inConversation(conv: { key: string; addresses: string[] }): ScheduledText[] {
    return forConversation(list.value, conv);
  }

  return { list, load, init, dispose, check, schedule, discard, sendNow, inConversation };
});
