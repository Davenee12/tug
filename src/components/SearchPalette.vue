<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { Bell, CheckCheck, Copy, FastForward, ListMusic, ListPlus, MessageSquare, Moon, Music2, Phone, Plus, QrCode, Repeat, Rewind, Search, SendHorizontal, Settings2, SkipBack, SkipForward, Pause, Play, ThumbsDown, ThumbsUp, Trash2, Volume1, Volume2, X } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { useDropStore } from "../stores/drop";
import { useFocusTrap } from "../lib/focusTrap";
import { parseActions, type Action, type ActionContext, type Person } from "../lib/commands";
import {
  appLabel,
  canClear,
  cleanName,
  clockTime,
  dayLabel,
  formatAddress,
  groupConversations,
  highlight,
  isConversation,
  notificationTime,
  snippet,
  threadKey,
} from "../lib/format";
import type { Contact, PhoneNotification, SearchResults, SmsMessage } from "../types/protocol";
import AppAvatar from "./AppAvatar.vue";

const tug = useTugStore();
const drop = useDropStore();
const root = ref<HTMLElement | null>(null);
const input = ref<HTMLInputElement | null>(null);
const list = ref<HTMLElement | null>(null);
const query = ref("");
const results = ref<SearchResults | null>(null);
const searching = ref(false);
const active = ref(0);
/** The user moved the selection themselves (arrows/mouse), so Enter means that row. */
const chose = ref(false);
/** A cleared, non-chat notification shown in full here (it has nowhere else to open). */
const expanded = ref<number | null>(null);

// Esc closes from anywhere in the dialog, not only from the search box.
useFocusTrap(root, () => close());
onMounted(async () => {
  await nextTick();
  input.value?.focus();
});

let timer: number | undefined;
onUnmounted(() => window.clearTimeout(timer));
let seq = 0;
watch(query, (q) => {
  active.value = 0;
  chose.value = false;
  window.clearTimeout(timer);
  if (!q.trim()) {
    results.value = null;
    searching.value = false;
    return;
  }
  searching.value = true;
  timer = window.setTimeout(async () => {
    const mine = ++seq;
    const r = await tug.searchAll(q);
    if (mine !== seq) return; // a newer search started meanwhile
    results.value = r ?? null;
    searching.value = false;
  }, 120);
});

type Option =
  | { kind: "action"; key: string; a: Action }
  | { kind: "person"; key: string; c: Contact }
  | { kind: "message"; key: string; m: SmsMessage }
  | { kind: "notification"; key: string; n: PhoneNotification };

// Everyone a "text …" action could mean: contacts, plus numbers you've texted with.
const people = computed<Person[]>(() => {
  const byAddress = new Map<string, Person>();
  for (const c of tug.contacts) byAddress.set(c.address, { name: c.name, address: c.address });
  for (const c of groupConversations(tug.notifications, tug.messages, tug.contacts)) {
    for (const a of c.addresses) if (!byAddress.has(a)) byAddress.set(a, { name: c.contact, address: a });
  }
  return [...byAddress.values()];
});

// What's waiting right now, so the one-off actions can say exactly what they'll do.
const ctx = computed<ActionContext>(() => {
  const feed = tug.notifications.filter((n) => n.removedAt == null);
  const conversations = groupConversations(tug.notifications, tug.messages, tug.contacts);
  // One row per app, newest first (notifications are already newest-first), to scroll to.
  const apps = new Map<string, { appId: string; label: string; focusId: number }>();
  for (const n of feed) if (!apps.has(n.appId)) apps.set(n.appId, { appId: n.appId, label: appLabel(n), focusId: n.id });
  return {
    code: tug.latestCode(),
    clearable: feed.filter(canClear),
    doNotDisturb: tug.settings.doNotDisturb,
    unread: conversations.filter((c) => tug.newCount(c.key, c.notifications) > 0).length,
    apps: [...apps.values()],
    canDial: tug.canDial,
    notifications: tug.notifications,
    playlists: tug.playlists,
    spotifyConnected: tug.spotify.connected,
  };
});

// Actions answer as you type (no search round-trip), above the results.
const actions = computed<Option[]>(() =>
  parseActions(query.value, people.value, ctx.value).map((a, i) => ({ kind: "action" as const, key: `a:${i}:${a.label}`, a })),
);

const options = computed<Option[]>(() => {
  const r = results.value;
  if (!r) return actions.value;
  // A Messages notification whose text is already listed under Texts is the same message.
  const texts = new Set(r.messages.map((m) => m.body.trim()));
  const notifications = r.notifications.filter(
    (n) => !(n.appId === "com.apple.MobileSMS" && texts.has((n.message || n.subtitle).trim())),
  );
  return [
    ...actions.value,
    ...r.people.slice(0, 5).map((c) => ({ kind: "person" as const, key: `p:${c.address}`, c })),
    ...r.messages.slice(0, 12).map((m) => ({ kind: "message" as const, key: `m:${m.id}`, m })),
    ...notifications.slice(0, 12).map((n) => ({ kind: "notification" as const, key: `n:${n.id}`, n })),
  ];
});

const sectionOf = (o: Option) =>
  ({ action: "Actions", person: "People", message: "Texts", notification: "Notifications" })[o.kind];
const MEDIA_ICONS = {
  play: Play,
  pause: Pause,
  togglePlayPause: Play,
  nextTrack: SkipForward,
  previousTrack: SkipBack,
  volumeUp: Volume2,
  volumeDown: Volume1,
  advanceRepeatMode: Repeat,
  skipForward: FastForward,
  skipBackward: Rewind,
  likeTrack: ThumbsUp,
  dislikeTrack: ThumbsDown,
};
function actionIcon(a: Action) {
  if (a.kind === "send") return SendHorizontal;
  if (a.kind === "call" || a.kind === "call-setup") return Phone;
  if (a.kind === "open-chat") return MessageSquare;
  if (a.kind === "media") return MEDIA_ICONS[a.command];
  if (a.kind === "play-playlist") return ListMusic;
  if (a.kind === "play-search") return Play;
  if (a.kind === "queue-search") return ListPlus;
  if (a.kind === "copy-code") return Copy;
  if (a.kind === "clear-all") return Trash2;
  if (a.kind === "mark-all-read") return CheckCheck;
  if (a.kind === "dnd") return Moon;
  if (a.kind === "show-app") return Bell;
  if (a.target === "spotify") return Music2;
  if (a.target === "drop") return QrCode;
  return a.target === "settings" ? Settings2 : Plus;
}
/** Several people match a "text …": Enter waits until one is picked on purpose. */
const picksPerson = (a: Action) => a.kind === "send" || a.kind === "call";
const ambiguous = computed(() => {
  const rows = actions.value.filter((o) => o.kind === "action" && picksPerson(o.a));
  return rows.length > 1;
});
const showHeader = (i: number) => i === 0 || options.value[i].kind !== options.value[i - 1].kind;

const messageName = (m: SmsMessage) => cleanName(m.contactName ?? formatAddress(m.address));
const when = (d: Date) => `${dayLabel(d)} · ${clockTime(d)}`;
const messageTime = (m: SmsMessage) => new Date(m.sentAt ?? m.receivedAt);

function close() {
  tug.searchOpen = false;
}

/** Open the conversation for an address/name, preferring one that already exists. */
function openConversation(address: string | null, name: string) {
  const key = threadKey({ appId: "com.apple.MobileSMS", title: name });
  const existing = groupConversations(tug.notifications, tug.messages, tug.contacts).find(
    (c) => c.key === key || (address !== null && c.addresses.includes(address)),
  );
  if (existing) tug.openThread(existing.key);
  else if (address) tug.startConversation(address, name);
  else tug.openThread(key);
}

async function run(a: Action) {
  close();
  if (a.kind === "send") {
    const ok = await tug.sendMessage(a.person.address, a.text);
    if (ok) openConversation(a.person.address, cleanName(a.person.name));
  } else if (a.kind === "open-chat") {
    openConversation(a.person.address, cleanName(a.person.name));
  } else if (a.kind === "media") {
    void tug.media(a.command);
  } else if (a.kind === "play-playlist") {
    void tug.playContext(a.uri, a.name);
  } else if (a.kind === "play-search") {
    void tug.playFromSearch(a.query);
  } else if (a.kind === "queue-search") {
    void tug.queueFromSearch(a.query);
  } else if (a.kind === "call") {
    void tug.callPerson(a.person.name, a.person.address);
  } else if (a.kind === "call-setup") {
    tug.openSettings("iphone");
  } else if (a.kind === "copy-code") {
    if (a.code) void tug.copyCode(a.code, a.from);
  } else if (a.kind === "clear-all") {
    if (a.items.length) void tug.clearItems(a.items);
  } else if (a.kind === "mark-all-read") {
    if (a.count) tug.markAllRead();
  } else if (a.kind === "dnd") {
    void tug.setSetting("doNotDisturb", a.enabled);
  } else if (a.kind === "show-app") {
    tug.view = "feed";
    tug.focusItem = `n${a.focusId}`;
  } else if (a.target === "new-message") {
    tug.view = "messages";
    tug.pickerOpen = true;
  } else if (a.target === "drop") {
    void drop.show();
  } else if (a.target === "spotify") {
    tug.openSpotifyPanel("search");
  } else {
    tug.openSettings();
  }
}

function pick(o: Option | undefined) {
  if (!o) return;
  if (o.kind === "action") {
    void run(o.a);
    return;
  }
  if (o.kind === "person") {
    openConversation(o.c.address, cleanName(o.c.name));
  } else if (o.kind === "message") {
    openConversation(o.m.address, messageName(o.m));
    tug.focusItem = `m${o.m.id}`;
  } else if (isConversation(o.n)) {
    tug.openThread(threadKey(o.n));
    tug.focusItem = `n${o.n.id}`;
  } else if (o.n.removedAt == null) {
    tug.view = "feed";
    tug.focusItem = `n${o.n.id}`;
  } else {
    // Cleared app notifications aren't in the feed any more: show them in full here.
    expanded.value = expanded.value === o.n.id ? null : o.n.id;
    return;
  }
  close();
}

async function move(delta: number) {
  const n = options.value.length;
  if (!n) return;
  active.value = (active.value + delta + n) % n;
  chose.value = true;
  await nextTick();
  list.value?.querySelector<HTMLElement>(`[data-index="${active.value}"]`)?.scrollIntoView({ block: "nearest" });
}

function onKey(e: KeyboardEvent) {
  if (e.key === "ArrowDown") void move(1);
  else if (e.key === "ArrowUp") void move(-1);
  else if (e.key === "Enter") {
    const o = options.value[active.value];
    if (o?.kind === "action" && picksPerson(o.a) && ambiguous.value && !chose.value) {
      tug.notify("info", "More than one person matches. Pick one with ↑↓, then Enter.");
    } else pick(o);
  }
  else if (e.key === "Escape") close();
  else return;
  e.preventDefault();
}
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-start justify-center bg-ink/30 px-6 pt-[10vh] backdrop-blur-[2px]" @click.self="close">
    <div
      ref="root"
      role="dialog"
      aria-modal="true"
      aria-label="Search"
      class="flex max-h-[76vh] w-full max-w-[620px] flex-col overflow-hidden rounded-xl border border-hairline bg-canvas"
    >
      <div class="flex items-center gap-3 border-b border-hairline-soft px-5 py-3.5">
        <Search :size="18" class="shrink-0 text-muted-soft" />
        <input
          ref="input"
          v-model="query"
          class="h-8 flex-1 bg-transparent text-[17px] text-ink outline-none placeholder:text-muted-soft"
          placeholder="Search, or type an action: text zoe on my way · pause"
          spellcheck="false"
          autocomplete="off"
          aria-label="Search"
          @keydown="onKey"
        />
        <button class="rounded-md p-1.5 text-muted active:bg-surface-card" aria-label="Close search" @click="close">
          <X :size="16" />
        </button>
      </div>

      <ul ref="list" class="min-h-0 flex-1 overflow-y-auto px-2 py-2">
        <li v-if="!query.trim()" class="px-4 py-8 text-center text-[13px] text-muted">
          <p class="headline text-[22px] text-ink">Find anything</p>
          <p class="mt-1">A name, a phone number, or words from a text or notification.</p>
          <p class="mt-4 text-[12px] text-muted-soft">
            Or do something:
            <kbd class="mx-0.5 rounded border border-hairline px-1.5 font-mono text-[11px]">text zoe on my way</kbd>
            <kbd class="mx-0.5 rounded border border-hairline px-1.5 font-mono text-[11px]">pause</kbd>
            <kbd class="mx-0.5 rounded border border-hairline px-1.5 font-mono text-[11px]">copy code</kbd>
            <kbd class="mx-0.5 rounded border border-hairline px-1.5 font-mono text-[11px]">clear all</kbd>
            <kbd class="mx-0.5 rounded border border-hairline px-1.5 font-mono text-[11px]">dnd</kbd>
            <kbd class="mx-0.5 rounded border border-hairline px-1.5 font-mono text-[11px]">new message</kbd>
            <kbd class="mx-0.5 rounded border border-hairline px-1.5 font-mono text-[11px]">drop</kbd>
          </p>
        </li>
        <li v-else-if="!searching && results && options.length === 0" class="px-4 py-8 text-center text-[13px] text-muted">
          Nothing matches “{{ query.trim() }}”.
        </li>

        <template v-for="(o, i) in options" :key="o.key">
          <li v-if="showHeader(i)" class="caption-upper px-3 pt-3 pb-1 text-muted-soft">{{ sectionOf(o) }}</li>
          <li>
            <button
              :data-index="i"
              :class="['flex w-full items-start gap-3 rounded-lg px-3 py-2 text-left', i === active ? 'bg-surface-card' : '']"
              @mousemove="
                active = i;
                chose = true;
              "
              @click="pick(o)"
            >
              <!-- Action -->
              <template v-if="o.kind === 'action'">
                <span class="flex size-8 shrink-0 items-center justify-center rounded-full bg-surface-card text-ink">
                  <component :is="actionIcon(o.a)" :size="15" />
                </span>
                <span class="min-w-0 flex-1 self-center">
                  <span class="block truncate text-[14px] font-medium text-ink">{{ o.a.label }}</span>
                  <span v-if="'detail' in o.a" class="block font-mono text-[12px] text-muted-soft">{{ o.a.detail }}</span>
                </span>
                <kbd v-if="i === active" class="self-center rounded border border-hairline px-1.5 font-mono text-[11px] text-muted">
                  Enter
                </kbd>
              </template>

              <!-- Person -->
              <template v-else-if="o.kind === 'person'">
                <AppAvatar app-id="com.apple.MobileSMS" :label="o.c.name" :photo-key="o.c.address" person size="sm" />
                <span class="min-w-0 flex-1">
                  <span class="block truncate text-[14px] font-medium text-ink">
                    <template v-for="(r, j) in highlight(cleanName(o.c.name), query)" :key="j">
                      <mark v-if="r.match" class="rounded-sm bg-accent-amber/30 text-ink">{{ r.text }}</mark>
                      <template v-else>{{ r.text }}</template>
                    </template>
                  </span>
                  <span class="block font-mono text-[12px] text-muted-soft">{{ formatAddress(o.c.address) }}</span>
                </span>
              </template>

              <!-- Text message -->
              <template v-else-if="o.kind === 'message'">
                <AppAvatar app-id="com.apple.MobileSMS" :label="messageName(o.m)" size="sm" />
                <span class="min-w-0 flex-1">
                  <span class="flex items-baseline gap-2">
                    <span class="truncate text-[14px] font-medium text-ink">{{ messageName(o.m) }}</span>
                    <span class="ml-auto shrink-0 font-mono text-[11px] text-muted-soft">{{ when(messageTime(o.m)) }}</span>
                  </span>
                  <span class="block text-[13px] text-body">
                    <span v-if="o.m.direction === 'out'" class="text-muted">You: </span>
                    <template v-for="(r, j) in highlight(snippet(o.m.body, query), query)" :key="j">
                      <mark v-if="r.match" class="rounded-sm bg-accent-amber/30 text-ink">{{ r.text }}</mark>
                      <template v-else>{{ r.text }}</template>
                    </template>
                  </span>
                </span>
              </template>

              <!-- Notification -->
              <template v-else>
                <AppAvatar :app-id="o.n.appId" :label="appLabel(o.n)" size="sm" />
                <span class="min-w-0 flex-1">
                  <span class="flex items-baseline gap-2">
                    <span class="truncate text-[14px] font-medium text-ink">
                      {{ appLabel(o.n) }}<template v-if="o.n.title"> · {{ cleanName(o.n.title) }}</template>
                    </span>
                    <span v-if="o.n.removedAt" class="shrink-0 text-[11px] text-muted-soft">cleared</span>
                    <span class="ml-auto shrink-0 font-mono text-[11px] text-muted-soft">{{ when(notificationTime(o.n)) }}</span>
                  </span>
                  <span :class="['block text-[13px] text-body', expanded === o.n.id ? 'whitespace-pre-line' : '']">
                    <template
                      v-for="(r, j) in highlight(expanded === o.n.id ? o.n.message : snippet(o.n.message || o.n.subtitle, query), query)"
                      :key="j"
                    >
                      <mark v-if="r.match" class="rounded-sm bg-accent-amber/30 text-ink">{{ r.text }}</mark>
                      <template v-else>{{ r.text }}</template>
                    </template>
                  </span>
                </span>
              </template>
            </button>
          </li>
        </template>
      </ul>

      <footer class="flex gap-4 border-t border-hairline-soft px-5 py-2.5 font-mono text-[11px] text-muted-soft">
        <span>↑↓ move</span><span>Enter open</span><span>Esc close</span>
        <span v-if="searching" class="ml-auto">Searching…</span>
      </footer>
    </div>
  </div>
</template>
