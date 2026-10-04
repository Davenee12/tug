<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { Info, Plus, RotateCcw, SendHorizontal, X } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import {
  clockTime,
  dayLabel,
  formatAddress,
  groupConversations,
  relativeTime,
  threadKey,
  type Conversation,
  type ConversationItem,
} from "../lib/format";
import AppAvatar from "./AppAvatar.vue";
import NewConversation from "./NewConversation.vue";

const tug = useTugStore();
const picking = ref(false);
const convs = computed(() => {
  const list = groupConversations(tug.visible, tug.messages, tug.contacts);
  // A conversation started with + shows (empty) until its first message exists.
  const draft = tug.composeTo;
  if (draft) {
    const key = threadKey({ appId: "com.apple.MobileSMS", title: draft.name });
    if (!list.some((c) => c.key === key)) {
      const placeholder = { kind: "message", id: "draft", at: new Date(), body: "" } as unknown as ConversationItem;
      const empty: Conversation = {
        key,
        appId: "com.apple.MobileSMS",
        appLabel: "Messages",
        contact: draft.name,
        address: draft.address,
        items: [],
        latest: placeholder,
        notifications: [],
      };
      list.unshift(empty);
    }
  }
  return list;
});
// The open conversation lives in the store so the Feed can open one directly.
const selected = computed(() => convs.value.find((c) => c.key === tug.selectedThread) ?? convs.value[0] ?? null);

function select(key: string) {
  tug.composeTo = null;
  tug.selectedThread = key;
  tug.markSeen(key);
}

// Opening Messages without a choice shows the newest conversation, which counts as seen.
watch(
  () => selected.value?.key,
  (key) => {
    if (key && tug.view === "messages" && tug.newCount(key, selected.value!.notifications)) tug.markSeen(key);
  },
  { immediate: true },
);

// Keep the latest message in view, like any chat.
const scroller = ref<HTMLElement | null>(null);
watch(
  () => [selected.value?.key, selected.value?.items.length],
  async () => {
    await nextTick();
    scroller.value?.scrollTo({ top: scroller.value.scrollHeight });
  },
  { immediate: true },
);

function showDay(i: number): boolean {
  const items = selected.value?.items ?? [];
  return i === 0 || dayLabel(items[i].at) !== dayLabel(items[i - 1].at);
}

const outgoing = (i: ConversationItem) => i.kind === "message" && i.m.direction === "out";
const statusLabel = (i: ConversationItem) => {
  if (i.kind !== "message" || i.m.direction !== "out") return "";
  return { pending: "Sending…", accepted: "Sent via iPhone", failed: "Not sent", received: "" }[i.m.status];
};

// Composer: replies go through the iPhone over message access (MAP).
const draft = ref("");
const sending = ref(false);
const canReply = computed(() => !!selected.value?.address && tug.status.services.messages);
const replyHint = computed(() => {
  if (!selected.value) return "";
  if (selected.value.appId !== "com.apple.MobileSMS") return `Reply to ${selected.value.appLabel} messages on your phone.`;
  if (tug.status.messagesError) return tug.status.messagesError;
  if (!tug.status.services.messages) return "Connecting to your iPhone's messages…";
  if (!selected.value.address) return "tug will learn this number when their next text arrives.";
  return "";
});

async function send(text = draft.value) {
  const conv = selected.value;
  if (!conv?.address || !text.trim() || sending.value) return;
  sending.value = true;
  if (text === draft.value) draft.value = "";
  const ok = await tug.sendMessage(conv.address, text);
  if (!ok && !draft.value) draft.value = text;
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
          :aria-label="picking ? 'Cancel new message' : 'New message'"
          :title="picking ? 'Cancel' : 'New message'"
          @click="picking = !picking"
        >
          <X v-if="picking" :size="16" />
          <Plus v-else :size="16" />
        </button>
      </div>
      <NewConversation v-if="picking" @close="picking = false" />
      <p v-if="!picking && convs.length === 0" class="px-3 py-6 text-[13px] text-muted">
        No conversations yet. Texts from your iPhone and other chats collect here. Use + to start one.
      </p>
      <button
        v-for="c in convs"
        :key="c.key"
        :class="['flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left', selected?.key === c.key ? 'bg-surface-card' : 'active:bg-surface-soft']"
        @click="select(c.key)"
      >
        <AppAvatar :app-id="c.appId" :label="c.contact" size="sm" />
        <span class="min-w-0 flex-1">
          <span class="flex items-baseline gap-2">
            <span :class="['truncate text-[14px] text-ink', tug.newCount(c.key, c.notifications) ? 'font-semibold' : 'font-medium']">
              {{ c.contact }}
            </span>
            <span v-if="c.items.length" class="ml-auto shrink-0 font-mono text-[11px] text-muted-soft">
              {{ relativeTime(c.latest.at) }}
            </span>
          </span>
          <span class="flex items-center gap-2">
            <span class="min-w-0 flex-1 truncate text-[13px] text-muted">
              <template v-if="!c.items.length">New message</template>
              <template v-else-if="outgoing(c.latest)">You: </template>{{ c.latest.body }}
            </span>
            <span
              v-if="tug.newCount(c.key, c.notifications)"
              class="flex h-[18px] min-w-[18px] shrink-0 items-center justify-center rounded-full bg-ink px-1 text-[11px] font-semibold text-on-dark"
            >
              {{ tug.newCount(c.key, c.notifications) }}
            </span>
          </span>
        </span>
      </button>
    </nav>

    <section v-if="selected" class="flex min-w-0 flex-1 flex-col">
      <header class="border-b border-hairline px-8 py-4">
        <p class="headline text-[26px] leading-tight">{{ selected.contact }}</p>
        <p class="text-[13px] text-muted">
          {{ selected.appLabel }}<template v-if="selected.address"> · {{ formatAddress(selected.address) }}</template>
        </p>
      </header>

      <div ref="scroller" class="flex-1 overflow-y-auto px-8 py-6">
        <template v-for="(item, i) in selected.items" :key="item.id">
          <div v-if="showDay(i)" class="caption-upper my-4 text-center text-muted-soft">{{ dayLabel(item.at) }}</div>
          <div :class="['mb-2 flex max-w-[75%] flex-col', outgoing(item) ? 'ml-auto items-end' : 'items-start']">
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
          </div>
        </template>
      </div>

      <footer class="border-t border-hairline px-6 py-3">
        <form v-if="canReply" class="flex items-end gap-2" @submit.prevent="send()">
          <textarea
            v-model="draft"
            rows="1"
            class="input h-auto max-h-32 min-h-10 resize-none py-2.5 leading-snug"
            :placeholder="`Text ${selected.contact}`"
            @keydown="onKey"
          />
          <button type="submit" class="btn-primary w-10 shrink-0 px-0" :disabled="!draft.trim() || sending" aria-label="Send">
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
