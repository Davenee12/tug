<script setup lang="ts">
import { computed, nextTick, onMounted, reactive, ref, watch } from "vue";
import { ChevronRight, History, Image, Info, Phone, Plus, RotateCcw, ShieldQuestionMark, X } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { useScheduledTextsStore } from "../stores/scheduledTexts";
import { useBackupStore } from "../stores/backup";
import { backupLabel, isImported, photoHint } from "../lib/backupImport";
import { api } from "../lib/ipc";
import MessageAttachments from "./MessageAttachments.vue";
import { replyUnavailable } from "../lib/availability";
import { clockTime, dayLabel, formatAddress, groupConversations, threadKey, type Conversation, type ConversationItem } from "../lib/format";
import { shouldStickToBottom } from "../lib/scroll";
import { reuseUnchanged, sameConversation, sameItems } from "../lib/stableList";
import { olderWindowStart, REVEAL_PX, startIncluding, windowStart } from "../lib/threadWindow";
import { bubbleKind, distinguishesIMessage, itemContent, type Shown } from "../lib/messageType";
import AppAvatar from "./AppAvatar.vue";
import CodeChip from "./CodeChip.vue";
import ConversationRow from "./ConversationRow.vue";
import MediaChip from "./MediaChip.vue";
import MessageComposer from "./MessageComposer.vue";
import SmartReplyChips from "./SmartReplyChips.vue";
import ScheduledTexts from "./ScheduledTexts.vue";
import SmartText from "./SmartText.vue";
import { findCode } from "../lib/codes";
import { conversationReplies } from "../lib/smartReplies";
import type { SmsMessage } from "../types/protocol";

const tug = useTugStore();
const scheduled = useScheduledTextsStore();
// History brought in from an iPhone backup: conversations older than the recent page, and each
// conversation's whole history when it's opened (stores/backup).
const backup = useBackupStore();
onMounted(() => void backup.ensureLoaded());
// Regrouped on every text and notification, but kept stable: a conversation whose contents didn't
// change keeps its object, so only the rows that changed re-render (lib/stableList).
const grouped = computed<Conversation[]>((prev) =>
  reuseUnchanged(prev, groupConversations(tug.notifications, tug.messages, tug.contacts), sameConversation),
);
const convs = computed(() => {
  const list = grouped.value;
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
      return [empty, ...list];
    }
  }
  return list;
});
// Filter unknown senders: people you don't know wait in a collapsed section at the bottom.
// The draft (always someone you chose to text) stays with your conversations.
const sections = computed<{ known: Conversation[]; unknown: Conversation[] }>((prev) => {
  const known: Conversation[] = [];
  const unknown: Conversation[] = [];
  for (const c of convs.value) (c.items.length === 0 || tug.isKnown(c) ? known : unknown).push(c);
  // Unchanged lists: hand back the same object, so nothing that reads them re-renders.
  return prev && sameItems(prev.known, known) && sameItems(prev.unknown, unknown) ? prev : { known, unknown };
});
// The open conversation lives in the store so the Feed can open one directly. With nothing
// chosen, the newest *known* one opens: opening reads it, and that shouldn't happen to spam.
const selected = computed(
  () => convs.value.find((c) => c.key === tug.selectedThread) ?? sections.value.known[0] ?? null,
);
const selectedUnknown = computed(() => !!selected.value && sections.value.unknown.some((c) => c.key === selected.value!.key));
// Opening a conversation brings its whole history in (the window keeps only recent texts).
watch(
  () => selected.value?.key,
  () => {
    if (selected.value) void backup.loadHistory(selected.value);
  },
  { immediate: true },
);
// Collapsed until asked for; opening one from the Feed or search expands it so it's visible.
const showUnknown = ref(false);
watch(selectedUnknown, (u) => u && (showUnknown.value = true), { immediate: true });
const unknownNew = computed(() => sections.value.unknown.reduce((n, c) => n + tug.newCount(c.key, c.notifications), 0));

function select(key: string) {
  tug.composeTo = null;
  tug.selectedThread = key;
  tug.markSeen(key);
}

// The conversation on screen counts as seen (in tug only), including the newest one Messages
// opens without a choice — but only while someone can see it: the window shown and focused, and
// nothing covering it (e.g. the New message picker, Ctrl+N). Hidden in the tray, a new text stays
// unread (tray, taskbar dot); coming back to the window marks the one on screen then.
watch(
  () => [selected.value?.key, selected.value?.notifications.length, tug.canSee] as const,
  ([key]) => {
    if (key && tug.view === "messages" && tug.canSee && tug.newCount(key, selected.value!.notifications)) {
      tug.markSeen(key);
    }
  },
  { immediate: true },
);

// The conversation on screen is read: clear it on the phone (and so the Feed) and mark its
// texts read there — on open, when new texts land in it, and when you come back to it. That
// includes the newest one Messages shows by default (a deliberate choice). Clearing the phone can't
// be undone, so only while you can actually see it: the window is shown and focused and nothing
// (search, the picker, settings, pairing) covers it (the store's canSee).
const lookingAt = (c: Conversation | null): c is Conversation =>
  !!c && c.items.length > 0 && tug.canSee && tug.view === "messages";
watch(
  () =>
    [
      selected.value?.key,
      selected.value?.items.length,
      selected.value?.notifications.length,
      tug.canSee,
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

// A long conversation renders its newest items only; scrolling up near the top reveals older ones
// (lib/threadWindow). `anchor` is the oldest item on screen, set when the conversation opens.
const anchor = ref<string | null>(null);
watch(
  () => selected.value?.key,
  () => {
    const items = selected.value?.items ?? [];
    anchor.value = items[windowStart(items, null)]?.id ?? null;
  },
  { immediate: true },
);
const firstShown = computed(() => windowStart(selected.value?.items ?? [], anchor.value));
const shownItems = computed(() => (selected.value?.items ?? []).slice(firstShown.value));
let revealing = false;
async function showOlder() {
  const el = scroller.value;
  const items = selected.value?.items ?? [];
  if (!el || revealing || firstShown.value === 0) return;
  revealing = true;
  // Keep what's on screen still while rows go in above it (the browser's own anchoring is held
  // off for this one change, so the two don't both adjust).
  el.style.overflowAnchor = "none";
  const top = el.scrollTop;
  const height = el.scrollHeight;
  anchor.value = items[olderWindowStart(firstShown.value)].id;
  await nextTick();
  el.scrollTop = top + (el.scrollHeight - height);
  el.style.overflowAnchor = "";
  revealing = false;
}
function onThreadScroll() {
  if ((scroller.value?.scrollTop ?? Infinity) < REVEAL_PX) void showOlder();
}

// Arriving from search: scroll to the item and briefly highlight it.
const flashed = ref<string | null>(null);
watch(
  () => [tug.focusItem, selected.value?.key] as const,
  async ([item]) => {
    if (!item) return;
    // An older item than the window shows: reveal down to it first.
    const items = selected.value?.items ?? [];
    const start = startIncluding(firstShown.value, items.findIndex((i) => i.id === item));
    if (start !== firstShown.value) anchor.value = items[start].id;
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

function showDay(shown: number): boolean {
  // Against the item before it in the whole conversation, shown or not, so the labels match.
  const i = firstShown.value + shown;
  const items = selected.value?.items ?? [];
  return i === 0 || dayLabel(items[i].at) !== dayLabel(items[i - 1].at);
}

const outgoing = (i: ConversationItem) => i.kind === "message" && i.m.direction === "out";
// Only tint when the phone actually distinguishes iMessage from text (both seen); otherwise the
// bubbles keep today's neutral ink. Decided once over all messages, not per conversation.
const distinguishes = computed(() => distinguishesIMessage(tug.messages));
// Bubble background. Only received messages carry the phone's type (sent rows have none), so the
// tint goes on incoming bubbles: blue when that person reached you over iMessage, green for a
// text. Neutral unless the phone distinguishes the two (the test iPhone reports every one as
// SMS_GSM, so today it stays neutral).
function bubbleClass(i: ConversationItem): string {
  if (outgoing(i)) return "rounded-br-sm bg-ink text-on-dark";
  const tint = i.kind === "message" ? bubbleKind(i.m, distinguishes.value) : null;
  const bg = tint === "imessage" ? "bg-imessage text-on-dark" : tint === "sms" ? "bg-sms text-on-dark" : "bg-surface-card text-ink";
  return `rounded-bl-sm ${bg}`;
}
const codeIn = (i: ConversationItem) => findCode(i.body);
// What each bubble shows: words, a photo chip ("Attachment: 1 Photo"), or both (lib/messageType).
// The words go through SmartText, so links, numbers, addresses and parcel numbers in them are one
// click (lib/smartActions); a code keeps its own chip underneath.
const contents = computed(() => new Map(shownItems.value.map((i) => [i.id, itemContent(i)])));
const contentOf = (i: ConversationItem): Shown => contents.value.get(i.id) ?? itemContent(i);
// A backup's real photos and videos, shown in place of a chip.
const attachmentsOf = (i: ConversationItem) => (i.kind === "message" ? (i.m.attachments ?? []) : []);
const unsent = (i: ConversationItem) => i.kind === "message" && !!i.m.unsent;
const edited = (i: ConversationItem) => i.kind === "message" && !!i.m.edited;
// "From your iPhone backup, Oct 12": once, above the oldest history the backup brought in.
const firstImported = computed(() => selected.value?.items.find((i) => i.kind === "message" && isImported(i.m))?.id ?? null);
const importedLabel = computed(() => backupLabel(backup.imported));
// Once: why a HEIC photo shows as "Open photo" instead of the picture, and which free extension
// would show it (HEIF Image Extensions, or HEVC Video Extensions when HEIF is already there).
const shownPhotoHint = computed(() =>
  photoHint({
    heif: backup.needsHeif && !backup.heifHintDismissed,
    hevc: backup.needsHevc && !backup.hevcHintDismissed,
  }),
);
const getExtension = (url: string) => void api.openUrl(url).catch(() => undefined);
const statusLabel = (i: ConversationItem) => {
  if (i.kind !== "message" || i.m.direction !== "out") return "";
  // "accepted" (the iPhone took it) and "sent" (a MAP SendingSuccess event confirmed it, when
  // live texts are working) both read as "Sent" — a send the user can trust either way.
  // "unconfirmed": the whole text went out but the phone never answered. No Retry: it may have
  // sent, and a second try could text them twice.
  return {
    pending: "Sending…",
    accepted: "Sent",
    sent: "Sent",
    failed: "Not sent",
    unconfirmed: "May have sent — check your iPhone",
    received: "",
  }[i.m.status];
};

// Composer: replies go through the iPhone over message access (MAP).
// One draft per conversation: a shared box carried text typed to one person into another's
// conversation, where Enter would send it to the wrong person.
// Only MessageComposer reads a draft while rendering, so typing re-renders the composer alone,
// not this whole view with the conversation in it.
const drafts = reactive(new Map<string, string>());
const draftFor = (key: string) => drafts.get(key) ?? "";
const setDraftFor = (key: string, text: string) => drafts.set(key, text);
const draft = computed(() => (selected.value ? draftFor(selected.value.key) : ""));
// Typing a text gets message access open by the time it's sent (throttled in the store).
watch(draft, (text) => {
  if (text.trim()) tug.warmTexts();
});
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
  // Every conversation here is a Messages one (other chat apps stay in the Feed: lib/format).
  if (!selected.value) return "";
  const unavailable = replyUnavailable(tug.status, tug.switchContext);
  if (unavailable) return unavailable;
  if (!selected.value.addresses.length) return "tug will learn this number when their next text arrives.";
  return "";
});

// Suggested replies to their newest text, while it's theirs (Settings › Notifications; lib/smartReplies).
// Only where the composer is: a conversation tug can't reply to gets no suggestions either.
const suggestions = computed(() =>
  canReply.value
    ? conversationReplies(selected.value, { enabled: tug.settings.smartReplies, defaults: tug.settings.smartReplyDefaults })
    : [],
);

// A suggestion sends exactly like typing it and pressing Enter (same Sending…, Not sent with Retry).
async function send(text = draft.value) {
  const conv = selected.value;
  const to = replyTo.value;
  if (!conv || !to || !text.trim() || sending.value) return;
  sending.value = true;
  // Keyed by the conversation it was typed in, even if another is opened while it sends.
  const key = conv.key;
  if (text === draftFor(key)) setDraftFor(key, "");
  const result = await tug.sendMessage(to, text);
  // Only when nothing was recorded: a failed send already shows as "Not sent" with Retry, and
  // putting the text back too invited sending it twice.
  if (result === "error" && !draftFor(key)) setDraftFor(key, text);
  // You've answered, so their notifications on the phone are done with; but only once the phone
  // took the text (as for a pop-up reply): a failed or unconfirmed one leaves them to come back to.
  if (result === "sent") void tug.clearItems(conv.notifications);
  sending.value = false;
}

// A stranger's texts keep their web links as plain words (lib/smartActions): one-click links are how
// scam texts work. Everything else in them still acts.
const linksOk = computed(() => !selected.value || tug.isKnownSender(selected.value));

// Send later: the draft goes into the conversation's scheduled texts (stores/scheduledTexts) and
// the box empties, as if it had been sent.
async function sendLater(at: number, label: string) {
  const conv = selected.value;
  const to = replyTo.value;
  const text = draft.value;
  if (!conv || !to || !text.trim()) return;
  const outcome = await scheduled.schedule({ address: to, name: conv.contact, threadKey: conv.key }, text, at);
  if (outcome !== "ok") {
    tug.notify(
      "error",
      outcome === "full" ? "50 texts are already waiting to be sent. Send or cancel one first." : "Couldn't save the scheduled text. Try again in a moment.",
    );
    return;
  }
  if (draftFor(conv.key) === text) setDraftFor(conv.key, "");
  tug.notify("info", `Your text will send ${label.charAt(0).toLowerCase()}${label.slice(1)}.`);
}

// Retry resends that same message to the number it was meant for (not a new copy to whichever
// number the conversation shows now); on success the "Not sent" state goes away.
const retrying = ref<number | null>(null);
async function retry(m: SmsMessage) {
  if (retrying.value !== null) return;
  retrying.value = m.id;
  const conv = selected.value;
  if ((await tug.retryMessage(m.id)) && conv) void tug.clearItems(conv.notifications);
  retrying.value = null;
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
        <AppAvatar :app-id="selected.appId" :label="selected.contact" :photo-key="selected.address ?? undefined" person />
        <div class="min-w-0 flex-1">
          <p class="headline preview-private truncate text-[26px] leading-tight">{{ selected.contact }}</p>
          <p class="text-[13px] text-muted">
            {{ selected.appLabel }}<template v-if="selected.address"> · <span class="preview-private">{{ formatAddress(selected.address) }}</span></template>
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
      <!-- Once, until dismissed: why an iPhone photo shows as "Open photo". -->
      <div v-if="shownPhotoHint" class="flex items-center gap-3 border-b border-hairline bg-surface-soft px-8 py-2.5">
        <Image :size="15" class="shrink-0 text-muted" />
        <p class="min-w-0 flex-1 text-[13px] text-muted">{{ shownPhotoHint.text }}</p>
        <button class="btn-secondary btn-sm shrink-0" @click="getExtension(shownPhotoHint.url)">Get it</button>
        <button
          class="shrink-0 rounded p-1 text-muted hover:bg-surface-card hover:text-ink"
          aria-label="Dismiss"
          @click="backup.dismissPhotoHint(shownPhotoHint.kind)"
        >
          <X :size="13" />
        </button>
      </div>

      <div ref="scroller" class="flex-1 overflow-y-auto px-8 py-6" @scroll.passive="onThreadScroll">
        <template v-for="(item, i) in shownItems" :key="item.id">
          <div v-if="showDay(i)" class="caption-upper my-4 text-center text-muted-soft">{{ dayLabel(item.at) }}</div>
          <p
            v-if="item.id === firstImported"
            class="mx-auto my-3 flex max-w-md items-center justify-center gap-1.5 text-center text-[12px] text-muted-soft"
          >
            <History :size="12" class="shrink-0" /> {{ importedLabel }}
          </p>
          <p
            v-if="item.kind === 'message' && item.m.gapBefore"
            class="mx-auto my-3 flex max-w-md items-center justify-center gap-1.5 text-center text-[12px] text-muted"
          >
            <Info :size="12" class="shrink-0" /> Earlier texts from while tug was away may only be on your iPhone.
          </p>
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
                'preview-private selectable rounded-xl px-4 py-2.5 text-[14px] whitespace-pre-line',
                bubbleClass(item),
                item.kind === 'message' && item.m.status === 'pending' ? 'opacity-70' : '',
              ]"
            >
              <span v-if="item.kind === 'notification' && item.n.subtitle" class="mb-0.5 block text-[12px] font-medium text-muted">
                {{ item.n.subtitle }}
              </span>
              <span v-if="unsent(item)" class="italic opacity-70">{{ contentOf(item).text }}</span>
              <SmartText v-else-if="contentOf(item).text" :text="contentOf(item).text" :links="linksOk" />
              <MessageAttachments
                v-if="attachmentsOf(item).length"
                :attachments="attachmentsOf(item)"
                :class="contentOf(item).text ? 'mt-2' : ''"
              />
              <MediaChip
                v-else-if="contentOf(item).media || contentOf(item).unreadable"
                :media="contentOf(item).media"
                :unreadable="contentOf(item).unreadable"
                :class="contentOf(item).text ? 'mt-2' : ''"
              />
            </div>
            <span class="mx-1 mt-1 flex items-center gap-1.5 font-mono text-[11px] text-muted-soft">
              {{ clockTime(item.at) }}<template v-if="edited(item)"> · Edited</template>
              <template v-if="statusLabel(item)">
                · <span :class="item.kind === 'message' && item.m.status === 'failed' ? 'text-error' : ''">{{ statusLabel(item) }}</span>
              </template>
              <button
                v-if="item.kind === 'message' && item.m.status === 'failed'"
                class="flex items-center gap-1 rounded px-1 text-ink active:bg-surface-card"
                :disabled="retrying !== null"
                @click="retry(item.m)"
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
            class="preview-private rounded-md border border-hairline bg-canvas px-2 py-1 font-mono text-[12px] text-ink"
            :value="replyTo ?? ''"
            @change="chosen[selected.key] = ($event.target as HTMLSelectElement).value"
          >
            <option v-if="!replyTo" value="" disabled>Choose a number</option>
            <option v-for="a in selected.addresses" :key="a" :value="a">{{ formatAddress(a) }}</option>
          </select>
          <span v-if="selected.addresses.length > 1">· {{ selected.addresses.length }} numbers under this name</span>
        </label>
        <ScheduledTexts :conversation="selected" />
        <SmartReplyChips
          v-if="canReply"
          :replies="suggestions"
          :contact="selected.contact"
          :disabled="sending || !replyTo"
          @pick="(text) => send(text)"
        />
        <MessageComposer
          v-if="canReply"
          :drafts="drafts"
          :draft-key="selected.key"
          :placeholder="tug.previewsHidden ? 'Text message' : `Text ${selected.contact}`"
          class="preview-private"
          :blocked="sending || !replyTo"
          @update="(text) => setDraftFor(selected!.key, text)"
          @send="send()"
          @schedule="sendLater"
        />
        <p v-else class="flex items-center gap-2 py-1 text-[13px] text-muted">
          <Info :size="14" class="shrink-0" />
          {{ replyHint }}
        </p>
        <!-- Once, until dismissed: why a reply typed on the phone isn't in the thread. -->
        <p v-if="canReply && !tug.sentHintDismissed" class="mt-1.5 flex items-start gap-2 px-1 text-[11px] text-muted-soft">
          <span class="min-w-0 flex-1">Texts you send from tug show here. Texts you send on your iPhone stay on your iPhone.</span>
          <button
            class="shrink-0 rounded p-0.5 hover:bg-surface-card hover:text-ink"
            aria-label="Dismiss this tip"
            title="Got it"
            @click="tug.dismissSentHint()"
          >
            <X :size="12" />
          </button>
        </p>
      </footer>
    </section>
  </div>
</template>
