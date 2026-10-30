<script setup lang="ts">
// The quick text composer (lib/quickText), opened by its global shortcut from any app: pick
// someone (the same list as New message), type, and Enter sends exactly as the Messages view does.
// Sent, or closed with Esc: the store asks the backend to give the keyboard back to the app the
// person was in. A text that didn't go keeps tug in front to say so.
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { Info, Search, SendHorizontal, X } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { useFocusTrap } from "../lib/focusTrap";
import { formatAddress, groupConversations } from "../lib/format";
import { contactRecipients, recentRecipients, recipientOptions, type Recipient } from "../lib/recipients";
import { replyUnavailable } from "../lib/availability";
import { afterQuickSend, canQuickSend } from "../lib/quickText";
import AppAvatar from "./AppAvatar.vue";

const tug = useTugStore();
const root = ref<HTMLElement | null>(null);
const search = ref<HTMLInputElement | null>(null);
const box = ref<HTMLTextAreaElement | null>(null);
const list = ref<HTMLElement | null>(null);
const query = ref("");
const active = ref(0);
/** Who it goes to; null while still picking. */
const to = ref<Recipient | null>(null);
const text = ref("");
const sending = ref(false);

// Esc closes from anywhere in the composer, the same as cancelling. Not mid-send: the text is
// already on its way, and closing would hide how it went.
useFocusTrap(root, () => {
  if (!sending.value) tug.closeQuickText(true);
});

onMounted(async () => {
  await nextTick();
  search.value?.focus();
});

const recent = computed(() =>
  recentRecipients(groupConversations(tug.notifications, tug.messages, tug.contacts), (c) => tug.isKnown(c)),
);
const contacts = computed(() => contactRecipients(tug.contacts));
const options = computed(() => recipientOptions(query.value, recent.value, contacts.value));
watch(query, () => (active.value = 0));

/** Why a text can't go from tug right now (the same words as the Messages view), or null. */
const unavailable = computed(() => replyUnavailable(tug.status, tug.switchContext));
const canSend = computed(() =>
  canQuickSend({ to: to.value?.address ?? null, text: text.value, unavailable: unavailable.value, sending: sending.value }),
);

// Typing gets message access open by the time it's sent (throttled in the store).
watch(text, (t) => {
  if (t.trim()) tug.warmTexts();
});

async function pick(o: Recipient | undefined) {
  if (!o) return;
  to.value = o;
  await nextTick();
  box.value?.focus();
}

async function change() {
  to.value = null;
  await nextTick();
  search.value?.focus();
}

async function move(delta: number) {
  const n = options.value.length;
  if (!n) return;
  active.value = (active.value + delta + n) % n;
  await nextTick();
  list.value?.querySelector<HTMLElement>(`[data-index="${active.value}"]`)?.scrollIntoView({ block: "nearest" });
}

function onSearchKey(e: KeyboardEvent) {
  if (e.key === "ArrowDown") void move(1);
  else if (e.key === "ArrowUp") void move(-1);
  else if (e.key === "Enter") void pick(options.value[active.value]);
  else return;
  e.preventDefault();
}

function onBoxKey(e: KeyboardEvent) {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    void send();
  }
}

// Only ever from the person's own Enter or Send.
async function send() {
  const person = to.value;
  if (!person || !canSend.value) return;
  sending.value = true;
  const result = await tug.sendMessage(person.address, text.value);
  sending.value = false;
  const next = afterQuickSend(result);
  if (next === "giveBack") {
    tug.closeQuickText(true);
  } else if (next === "showConversation") {
    // "May have sent" or "Not sent" with Retry: shown in the conversation, with tug left in front.
    tug.closeQuickText(false);
    tug.closeSettings();
    tug.startConversation(person.address, person.name);
  } else {
    // Nothing was recorded and tug has said why: the text stays to send again.
    await nextTick();
    box.value?.focus();
  }
}

const showHeader = (i: number) => i === 0 || options.value[i].section !== options.value[i - 1].section;
const sectionLabel = (s: Recipient["section"]) => ({ Recent: "Recent", Contacts: "All contacts", Number: "Text a number" })[s];
</script>

<template>
  <div
    class="fixed inset-0 z-50 flex items-start justify-center bg-ink/30 px-6 pt-[12vh] backdrop-blur-[2px]"
    @click.self="!sending && tug.closeQuickText(true)"
  >
    <div
      ref="root"
      role="dialog"
      aria-modal="true"
      aria-label="Text someone"
      class="flex max-h-[70vh] w-full max-w-[520px] flex-col overflow-hidden rounded-xl border border-hairline bg-canvas"
    >
      <header class="flex items-center justify-between px-5 pt-4 pb-2">
        <h2 class="headline text-[24px] leading-none">Text someone</h2>
        <button
          class="rounded-md p-1.5 text-muted active:bg-surface-card"
          aria-label="Close"
          :disabled="sending"
          @click="tug.closeQuickText(true)"
        >
          <X :size="16" />
        </button>
      </header>

      <!-- Plainly, up front: a text can't go from tug right now, so Send stays off. -->
      <p v-if="unavailable" role="status" class="mx-5 mb-3 flex gap-2 rounded-lg bg-surface-soft px-3 py-2 text-[13px] text-body">
        <Info :size="14" class="mt-0.5 shrink-0" />
        {{ unavailable }}
      </p>

      <template v-if="!to">
        <div class="px-5 pb-3">
          <div class="relative">
            <Search :size="17" class="pointer-events-none absolute top-1/2 left-3.5 -translate-y-1/2 text-muted-soft" />
            <input
              ref="search"
              v-model="query"
              class="input h-12 pl-11 text-[16px]"
              placeholder="Name or phone number"
              aria-label="Who to text"
              spellcheck="false"
              autocomplete="off"
              @keydown="onSearchKey"
            />
          </div>
        </div>

        <ul ref="list" class="min-h-0 flex-1 overflow-y-auto border-t border-hairline-soft px-2 py-2">
          <template v-for="(o, i) in options" :key="o.key">
            <li v-if="showHeader(i)" class="caption-upper px-3 pt-3 pb-1 text-muted-soft">{{ sectionLabel(o.section) }}</li>
            <li :class="['rounded-lg', i === active ? 'bg-surface-card' : '']" @mousemove="active = i">
              <button :data-index="i" class="flex w-full min-w-0 items-center gap-3 px-3 py-2 text-left" @click="pick(o)">
                <AppAvatar app-id="com.apple.MobileSMS" :label="o.name" person size="sm" />
                <span class="min-w-0 flex-1">
                  <span class="block truncate text-[14px] font-medium text-ink">{{ o.name }}</span>
                  <span class="block truncate font-mono text-[12px] text-muted-soft">{{ formatAddress(o.address) }}</span>
                </span>
              </button>
            </li>
          </template>
          <li v-if="query && options.length === 0" class="px-3 py-6 text-center text-[13px] text-muted-soft">
            No one matches “{{ query }}”. Type a full number to text it.
          </li>
          <li v-else-if="!query && options.length === 0" class="px-3 py-6 text-center text-[13px] text-muted-soft">
            Type a phone number to text it.
          </li>
        </ul>

        <footer class="flex gap-4 border-t border-hairline-soft px-5 py-2.5 font-mono text-[11px] text-muted-soft">
          <span>↑↓ move</span><span>Enter choose</span><span>Esc close</span>
        </footer>
      </template>

      <template v-else>
        <div class="flex items-center gap-3 border-t border-hairline-soft px-5 py-3">
          <AppAvatar app-id="com.apple.MobileSMS" :label="to.name" person size="sm" />
          <span class="preview-private min-w-0 flex-1">
            <span class="block truncate text-[14px] font-medium text-ink">{{ to.name }}</span>
            <span class="block truncate font-mono text-[12px] text-muted-soft">{{ formatAddress(to.address) }}</span>
          </span>
          <button class="btn-secondary btn-sm" :disabled="sending" @click="change">Change</button>
        </div>
        <form class="flex items-end gap-2 px-5 pb-4" @submit.prevent="send">
          <textarea
            ref="box"
            v-model="text"
            rows="3"
            class="input preview-private h-auto max-h-40 min-h-20 resize-none py-2.5 leading-snug"
            :placeholder="`Text ${to.name}`"
            aria-label="Message"
            :readonly="sending"
            @keydown="onBoxKey"
          />
          <button type="submit" class="btn-primary shrink-0" :disabled="!canSend">
            <SendHorizontal :size="16" /> {{ sending ? "Sending…" : "Send" }}
          </button>
        </form>
        <footer class="flex gap-4 border-t border-hairline-soft px-5 py-2.5 font-mono text-[11px] text-muted-soft">
          <span>Enter send</span><span>Shift+Enter new line</span><span>Esc close</span>
        </footer>
      </template>
    </div>
  </div>
</template>
