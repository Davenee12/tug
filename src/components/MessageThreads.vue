<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { ChevronRight, Info, Phone, Plus, RotateCcw, SendHorizontal, ShieldQuestionMark } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { clockTime, dayLabel, formatAddress, groupConversations, threadKey, type Conversation, type ConversationItem } from "../lib/format";
import { shouldStickToBottom } from "../lib/scroll";
import CodeChip from "./CodeChip.vue";
import ConversationRow from "./ConversationRow.vue";
import { findCode } from "../lib/codes";

const tug = useTugStore();
const convs = computed(() => {
  const list = groupConversations(tug.notifications, tug.messages, tug.contacts);
  // A conversation started with + shows (empty) until its first message exists.
  const draft = tug.composeTo;
  // Once a real conversation exists for the draft's number, the draft is done (M9).
  if (draft && !list.some((c) => c.addresses.includes(draft.address))) {
    const key = threadKey({ appId: "com.apple.MobileSMS", title: draft.name });
    if (!list.some((c) => c.key === key)) {
      const placeholder = { kind: "message", id: "draft", at: new Date(), body: "" } as unknown as ConversationItem;
      const empty: Conversation = {
        key,
        appId: "com.apple.MobileSMS",
        appLabel: "Messages",
        contact: draft.name,
        address: draft.address,
        addresses: [draft.address],
        items: [],
        latest: placeholder,
        notifications: [],
      };
      list.unshift(empty);
    }
  }
  return list;
});
// Filter unknown senders: people you don't know wait in a collapsed section at the bottom.
// The draft (always someone you chose to text) stays with your conversations.
const sections = computed(() => {
  const known: Conversation[] = [];
  const unknown: Conversation[] = [];
  for (const c of convs.value) (c.items.length === 0 || tug.isKnown(c) ? known : unknown).push(c);
  return { known, unknown };
});
// The open conversation lives in the store so the Feed can open one directly. With nothing
// chosen, the newest *known* one opens: opening reads it, and that shouldn't happen to spam.
const selected = computed(
  () => convs.value.find((c) => c.key === tug.selectedThread) ?? sections.value.known[0] ?? null,
);
const selectedUnknown = computed(() => !!selected.value && sections.value.unknown.some((c) => c.key === selected.value!.key));
// Collapsed until asked for; opening one from the Feed or search expands it so it's visible.
const showUnknown = ref(false);
watch(selectedUnknown, (u) => u && (showUnknown.value = true), { immediate: true });
const unknownNew = computed(() => sections.value.unknown.reduce((n, c) => n + tug.newCount(c.key, c.notifications), 0));

function select(key: string) {
  tug.composeTo = null;
  tug.selectedThread = key;
  tug.markSeen(key);
}

// Opening Messages without a choice shows the newest conversation, which counts as seen
// (in tug only) — unless something is covering it, e.g. the New message picker (Ctrl+N).
watch(
  () => [selected.value?.key, tug.overlayOpen] as const,
  ([key]) => {
    if (key && tug.view === "messages" && !tug.overlayOpen && tug.newCount(key, selected.value!.notifications)) {
      tug.markSeen(key);
    }
  },
  { immediate: true },
);

// The conversation on screen is read: clear it on the phone (and so the Feed) and mark its
// texts read there — on open, when new texts land in it, and when you come back to it. That
// includes the newest one Messages shows by default (Dave's call). Clearing the phone can't
// be undone, so only while you can actually see it: the window is focused and nothing
// (search, the picker, settings, pairing) covers it.
const focused = ref(document.hasFocus());
const onFocus = () => (focused.value = true);
const onBlur = () => (focused.value = false);
onMounted(() => {
  window.addEventListener("focus", onFocus);
  window.addEventListener("blur", onBlur);
});
onUnmounted(() => {
  window.removeEventListener("focus", onFocus);
  window.removeEventListener("blur", onBlur);
});
const lookingAt = (c: Conversation | null): c is Conversation =>
  !!c && c.items.length > 0 && focused.value && tug.view === "messages" && !tug.overlayOpen;
watch(
  () =>
    [
      selected.value?.key,
      selected.value?.items.length,
      selected.value?.notifications.length,
      focused.value,
      tug.overlayOpen,
      tug.selectedThread,
    ] as const,
  () => {
    const c = selected.value;
    if (!lookingAt(c)) return;
    const ids = tug.messages.filter((m) => m.direction === "in" && c.addresses.includes(m.address)).map((m) => m.id);
    tug.readConversation(c.notifications, ids);
  },
  { immediate: true },
);

// Keep the newest message in view, like any chat — but only when it's wanted. Opening a
// conversation (the key changes) jumps to the bottom so you see the latest. A new message in the
// one you're reading sticks to the bottom only if you were already there; if you've scrolled up
// into history, it stays put. The reader's position is measured here, before the DOM updates (this
// watcher runs pre-flush), so "were they at the bottom?" is asked of the content as it was.
const scroller = ref<HTMLElement | null>(null);
watch(
  () => [selected.value?.key, selected.value?.items.length] as const,
  async ([key], old) => {
    const switched = key !== old?.[0];
    const el = scroller.value;
    const stick = switched || !el || shouldStickToBottom(el.scrollTop, el.scrollHeight, el.clientHeight);
    await nextTick();
    if (stick) scroller.value?.scrollTo({ top: scroller.value.scrollHeight });
  },
  { immediate: true },
);

// When the draft's first message lands under a different name (e.g. a typed number that
// turns out to be a contact), follow it to the real conversation instead of stranding the user.
watch(convs, (list) => {
  const draft = tug.composeTo;
  if (!draft) return;
  const real = list.find((c) => c.items.length > 0 && c.addresses.includes(draft.address));
  if (real) tug.openThread(real.key);
});

// Arriving from search: scroll to the item and briefly highlight it.
const flashed = ref<string | null>(null);
watch(
  () => [tug.focusItem, selected.value?.key] as const,
  async ([item]) => {
    if (!item) return;
    await nextTick();
    const el = scroller.value?.querySelector<HTMLElement>(`[data-item="${item}"]`);
    if (!el) return;
    el.scrollIntoView({ block: "center" });
    flashed.value = item;
    tug.focusItem = null;
    window.setTimeout(() => (flashed.value = null), 1800);
  },
  // Immediate: search usually sets the target just before this view mounts.
  { flush: "post", immediate: true },
);

function showDay(i: number): boolean {
  const items = selected.value?.items ?? [];
  return i === 0 || dayLabel(items[i].at) !== dayLabel(items[i - 1].at);
}

const outgoing = (i: ConversationItem) => i.kind === "message" && i.m.direction === "out";
const codeIn = (i: ConversationItem) => findCode(i.body);
const statusLabel = (i: ConversationItem) => {
  if (i.kind !== "message" || i.m.direction !== "out") return "";
  return { pending: "Sending…", accepted: "Sent via iPhone", failed: "Not sent", received: "" }[i.m.status];
};

// Composer: replies go through the iPhone over message access (MAP).
// One draft per conversation: a shared box carried text typed to one person into another's
// conversation, where Enter would send it to the wrong person.
const drafts = ref<Record<string, string>>({});
const draft = computed({
  get: () => (selected.value ? (drafts.value[selected.value.key] ?? "") : ""),
  set: (text: string) => {
    if (selected.value) drafts.value = { ...drafts.value, [selected.value.key]: text };
  },
});
const draftFor = (key: string) => drafts.value[key] ?? "";
const setDraftFor = (key: string, text: string) => (drafts.value = { ...drafts.value, [key]: text });
const sending = ref(false);
/** Number chosen in the To: picker, per conversation (several numbers under one name). */
const chosen = ref<Record<string, string>>({});
const replyTo = computed(() => {
  const c = selected.value;
  if (!c) return null;
  const pick = chosen.value[c.key];
  return pick && c.addresses.includes(pick) ? pick : c.address;
});
const canPickNumber = computed(() => (selected.value?.addresses.length ?? 0) > 1);
const canReply = computed(
  () => (!!replyTo.value || canPickNumber.value) && tug.status.services.messages && !!selected.value?.addresses.length,
);
const replyHint = computed(() => {
  if (!selected.value) return "";
  if (selected.value.appId !== "com.apple.MobileSMS") return `Reply to ${selected.value.appLabel} messages on your phone.`;
  if (tug.status.messagesError) return tug.status.messagesError;
  if (!tug.status.services.messages) return "Connecting to your iPhone's messages…";
  if (!selected.value.addresses.length) return "tug will learn this number when their next text arrives.";
  return "";
});

async function send(text = draft.value) {
  const conv = selected.value;
  const to = replyTo.value;
  if (!conv || !to || !text.trim() || sending.value) return;
  sending.value = true;
  // Keyed by the conversation it was typed in, even if another is opened while it sends.
  const key = conv.key;
  if (text === draftFor(key)) setDraftFor(key, "");
  const ok = await tug.sendMessage(to, text);
  if (!ok && !draftFor(key)) setDraftFor(key, text);
  // You've answered, so their notifications on the phone are done with.
  if (ok) void tug.clearItems(conv.notifications);
  sending.value = false;
}

function onKey(e: KeyboardEvent) {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    void send();
  }
}
</script>

<template>
  <div class="flex min-h-0 flex-1">
    <nav class="w-72 shrink-0 overflow-y-auto border-r border-hairline px-3 py-2">
      <div class="flex items-center justify-between px-3 pt-1 pb-2">
        <span class="caption-upper text-muted">Conversations</span>
        <button
          class="rounded-md p-1 text-ink active:bg-surface-card"
          aria-label="New message"
          title="New message (Ctrl+N)"
          @click="tug.pickerOpen = true"
        >
          <Plus :size="16" />
        </button>
      </div>
      <p v-if="convs.length === 0" class="px-3 py-6 text-[13px] text-muted">
        No conversations yet. Texts from your iPhone and other chats collect here. Use + to start one.
      </p>
      <ConversationRow v-for="c in sections.known" :key="c.key" :c="c" :active="selected?.key === c.key" @select="select(c.key)" />
      <p v-if="convs.length > 0 && sections.known.length === 0" class="px-3 py-4 text-[13px] text-muted">
        Nothing from people you know yet.
      </p>

      <!-- Filter unknown senders: never interleaved, collapsed until asked for. -->
      <section v-if="sections.unknown.length" class="mt-3 border-t border-hairline-soft pt-2">
        <button
          class="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left active:bg-surface-soft"
          :aria-expanded="showUnknown"
          @click="showUnknown = !showUnknown"
        >
          <ChevronRight :size="14" :class="['shrink-0 text-muted transition-transform', showUnknown ? 'rotate-90' : '']" />
          <span class="caption-upper text-muted">Unknown senders</span>
          <span class="ml-auto font-mono text-[11px] text-muted-soft">
            {{ sections.unknown.length }}<template v-if="unknownNew"> · {{ unknownNew }} new</template>
          </span>
        </button>
        <template v-if="showUnknown">
          <ConversationRow v-for="c in sections.unknown" :key="c.key" :c="c" :active="selected?.key === c.key" @select="select(c.key)" />
        </template>
      </section>
    </nav>

    <section v-if="selected" class="flex min-w-0 flex-1 flex-col">
      <header class="flex items-center gap-4 border-b border-hairline px-8 py-4">
        <div class="min-w-0 flex-1">
          <p class="headline truncate text-[26px] leading-tight">{{ selected.contact }}</p>
          <p class="text-[13px] text-muted">
            {{ selected.appLabel }}<template v-if="selected.address"> · {{ formatAddress(selected.address) }}</template>
          </p>
        </div>
        <!-- Experimental: only once Settings has checked hands-free dialing works here. -->
        <button
          v-if="tug.canDial && replyTo && selected.appId === 'com.apple.MobileSMS'"
          class="btn-secondary btn-sm shrink-0"
          :disabled="tug.calling !== null"
          :title="`Call ${selected.contact} on your iPhone (experimental)`"
          @click="tug.call(replyTo, selected.contact)"
        >
          <Phone :size="13" /> {{ tug.calling === replyTo ? "Calling…" : "Call" }}
        </button>
      </header>
      <div v-if="selectedUnknown" class="flex items-center gap-3 border-b border-hairline bg-surface-soft px-8 py-2.5">
        <ShieldQuestionMark :size="15" class="shrink-0 text-muted" />
        <p class="min-w-0 flex-1 text-[13px] text-muted">
          Not in your contacts. Texts from unknown senders don't count as unread or pop up, except codes. Replying moves them to your
          conversations.
        </p>
        <button class="btn-secondary btn-sm shrink-0" @click="tug.moveToConversations(selected)">Move to conversations</button>
      </div>

      <div ref="scroller" class="flex-1 overflow-y-auto px-8 py-6">
        <template v-for="(item, i) in selected.items" :key="item.id">
          <div v-if="showDay(i)" class="caption-upper my-4 text-center text-muted-soft">{{ dayLabel(item.at) }}</div>
          <div
            :data-item="item.id"
            :class="[
              'mb-2 flex max-w-[75%] flex-col rounded-xl transition-shadow duration-500',
              outgoing(item) ? 'ml-auto items-end' : 'items-start',
              flashed === item.id ? 'ring-2 ring-accent-amber ring-offset-4 ring-offset-canvas' : '',
            ]"
          >
            <div
              :class="[
                'selectable rounded-xl px-4 py-2.5 text-[14px] whitespace-pre-line',
                outgoing(item) ? 'rounded-br-sm bg-ink text-on-dark' : 'rounded-bl-sm bg-surface-card text-ink',
                item.kind === 'message' && item.m.status === 'pending' ? 'opacity-70' : '',
              ]"
            >
              <span v-if="item.kind === 'notification' && item.n.subtitle" class="mb-0.5 block text-[12px] font-medium text-muted">
                {{ item.n.subtitle }}
              </span>
              {{ item.body || "(no preview)" }}
            </div>
            <span class="mx-1 mt-1 flex items-center gap-1.5 font-mono text-[11px] text-muted-soft">
              {{ clockTime(item.at) }}
              <template v-if="statusLabel(item)">
                · <span :class="item.kind === 'message' && item.m.status === 'failed' ? 'text-error' : ''">{{ statusLabel(item) }}</span>
              </template>
              <button
                v-if="item.kind === 'message' && item.m.status === 'failed'"
                class="flex items-center gap-1 rounded px-1 text-ink active:bg-surface-card"
                @click="send(item.body)"
              >
                <RotateCcw :size="11" /> Retry
              </button>
            </span>
            <CodeChip
              v-if="!outgoing(item) && codeIn(item)"
              class="mt-1.5"
              :code="codeIn(item)!.code"
              :from="item.kind === 'notification' ? [item.n] : []"
            />
          </div>
        </template>
      </div>

      <footer class="border-t border-hairline px-6 py-3">
        <label v-if="canReply && canPickNumber" class="mb-2 flex items-center gap-2 px-1 text-[12px] text-muted">
          To:
          <select
            class="rounded-md border border-hairline bg-canvas px-2 py-1 font-mono text-[12px] text-ink"
            :value="replyTo ?? ''"
            @change="chosen[selected.key] = ($event.target as HTMLSelectElement).value"
          >
            <option v-if="!replyTo" value="" disabled>Choose a number</option>
            <option v-for="a in selected.addresses" :key="a" :value="a">{{ formatAddress(a) }}</option>
          </select>
          <span v-if="selected.addresses.length > 1">· {{ selected.addresses.length }} numbers under this name</span>
        </label>
        <form v-if="canReply" class="flex items-end gap-2" @submit.prevent="send()">
          <textarea
            v-model="draft"
            rows="1"
            class="input h-auto max-h-32 min-h-10 resize-none py-2.5 leading-snug"
            :placeholder="`Text ${selected.contact}`"
            @keydown="onKey"
          />
          <button type="submit" class="btn-primary w-10 shrink-0 px-0" :disabled="!draft.trim() || sending || !replyTo" aria-label="Send">
            <SendHorizontal :size="16" />
          </button>
        </form>
        <p v-else class="flex items-center gap-2 py-1 text-[13px] text-muted">
          <Info :size="14" class="shrink-0" />
          {{ replyHint }}
        </p>
        <p v-if="canReply" class="mt-1.5 px-1 text-[11px] text-muted-soft">
          Sent through your iPhone. Messages you send from the phone itself don't appear here.
        </p>
      </footer>
    </section>
  </div>
</template>
