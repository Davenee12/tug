<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { Bell, CheckCheck, ClipboardCopy, Copy, FastForward, Gamepad2, ListMusic, ListPlus, MessageSquare, Mic, Moon, Music2, Phone, PhoneIncoming, PhoneMissed, PhoneOutgoing, Plus, QrCode, Repeat, Rewind, Search, SendHorizontal, Settings2, SkipBack, SkipForward, Pause, Play, ThumbsDown, ThumbsUp, Trash2, Volume1, Volume2, X } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { useTugboatStore } from "../stores/tugboat";
import { useGameStore } from "../stores/game";
import { useVoiceStore } from "../stores/voice";
import { useFocusTrap } from "../lib/focusTrap";
import { byArrival } from "../lib/messageSync";
import {
  afterPaletteSend,
  callPick,
  matchCalls,
  notificationPick,
  parseActions,
  type Action,
  type ActionContext,
  type Person,
} from "../lib/commands";
import {
  appLabel,
  callKeys,
  callName,
  callTime,
  canClear,
  cleanName,
  clockTime,
  dayLabel,
  formatAddress,
  groupConversations,
  highlight,
  notificationTime,
  snippet,
  threadKey,
} from "../lib/format";
import type { CallDirection, CallRecord, Contact, PhoneNotification, SearchResults, SmsMessage } from "../types/protocol";
import { isTugChant } from "../lib/logoTricks";
import { logoSpins } from "../lib/logoSpin";
import AppAvatar from "./AppAvatar.vue";

const tug = useTugStore();
const tugboat = useTugboatStore();
const game = useGameStore();
const voice = useVoiceStore();
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
  // Opened with something to put back (a text that couldn't be sent): it searches like typing.
  if (tug.searchDraft) {
    query.value = tug.searchDraft;
    tug.searchDraft = "";
  }
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
  // A little secret for anyone who says the name five times: the logo spins. Not a search.
  if (isTugChant(q)) {
    logoSpins.value++;
    close();
    return;
  }
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
  | { kind: "call"; key: string; call: CallRecord; name: string; pick: ReturnType<typeof callPick> }
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
    voiceTyping: voice.enabled,
    unread: conversations.filter((c) => tug.newCount(c.key, c.notifications) > 0).length,
    apps: [...apps.values()],
    canDial: tug.canDial,
    notifications: tug.notifications,
    playlists: tug.playlists,
    spotifyConnected: tug.spotify.connected,
    showGames: tug.settings.showGames,
  };
});

// Recent calls come from the phone and are kept in the window, not the database, so they're
// matched here, under the names the Calls list shows. Keyed as the Calls list keys its rows, so
// "Show in Calls" points at the same row.
const nameFor = computed(() => new Map(tug.contacts.map((c) => [c.address, c.name])));
const callKeyOf = computed(() => {
  const keys = callKeys(tug.calls);
  return new Map(tug.calls.map((c, i) => [c, keys[i]]));
});
const calls = computed<Option[]>(() =>
  matchCalls(query.value, tug.calls, nameFor.value).map((call) => {
    const name = callName(call, nameFor.value);
    // What Enter does, worked out with the row, so the row can say it.
    const pick = callPick(call, tug.callRoute(name, call.number));
    return { kind: "call" as const, key: `c:${callKeyOf.value.get(call)}`, call, name, pick };
  }),
);

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
    // With the results, so People never lands above a call the selection is already on.
    ...calls.value,
    ...r.messages.slice(0, 12).map((m) => ({ kind: "message" as const, key: `m:${m.id}`, m })),
    ...notifications.slice(0, 12).map((n) => ({ kind: "notification" as const, key: `n:${n.id}`, n })),
  ];
});

const sectionOf = (o: Option) =>
  ({ action: "Actions", person: "People", call: "Calls", message: "Texts", notification: "Notifications" })[o.kind];
// As in the Calls list: which way the call went, by icon and tone.
const CALL_DIRECTION: Record<CallDirection, { label: string; icon: typeof Phone; tone: string }> = {
  incoming: { label: "Incoming", icon: PhoneIncoming, tone: "bg-accent-teal/20 text-ink" },
  outgoing: { label: "Outgoing", icon: PhoneOutgoing, tone: "bg-surface-cream-strong text-body-strong" },
  missed: { label: "Missed", icon: PhoneMissed, tone: "bg-error/10 text-error" },
};
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
  if (a.kind === "voice") return Mic;
  if (a.kind === "show-app") return Bell;
  if (a.kind === "send-clipboard") return ClipboardCopy;
  if (a.target === "spotify") return Music2;
  if (a.target === "tugboat") return QrCode;
  if (a.target === "tugboat-run" || a.target === "harbor-stack") return Gamepad2;
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
const callWhen = (c: CallRecord) => {
  const at = callTime(c);
  return at ? when(at) : "";
};
// The number goes under the name only when the name isn't the number already (as in Calls).
const showCallNumber = (c: CallRecord, name: string) => !!c.number && name !== formatAddress(c.number);

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
    const typed = query.value;
    const result = await tug.sendMessage(a.person.address, a.text);
    if (afterPaletteSend(result) === "open-conversation") openConversation(a.person.address, cleanName(a.person.name));
    else if (!tug.searchOpen) {
      // Nothing was recorded (the error says why): come back with what was typed, ready to retry.
      tug.searchDraft = typed;
      tug.searchOpen = true;
    }
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
  } else if (a.kind === "voice") {
    void voice.toggleFromPalette(a.enabled);
  } else if (a.kind === "show-app") {
    tug.view = "feed";
    tug.focusItem = `n${a.focusId}`;
  } else if (a.kind === "send-clipboard") {
    void tugboat.sendClipboard();
  } else if (a.target === "new-message") {
    tug.view = "messages";
    tug.pickerOpen = true;
  } else if (a.target === "tugboat") {
    void tugboat.show();
  } else if (a.target === "tugboat-run" || a.target === "harbor-stack") {
    void game.show(a.target);
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
  } else if (o.kind === "call") {
    // A call: what its row says (lib/commands callPick).
    if (o.pick.open === "call") void tug.callPerson(o.name, o.call.number);
    else if (o.pick.open === "message") openConversation(o.call.number, o.name);
    else {
      tug.view = "calls";
      tug.focusItem = `c${callKeyOf.value.get(o.call) ?? ""}`;
    }
  } else if (o.kind === "message") {
    // An old text (brought in from a backup) may be older than what the window has loaded: add it,
    // so the conversation has it to scroll to while the rest of its history loads.
    if (!tug.messages.some((m) => m.id === o.m.id)) tug.messages = [...tug.messages, o.m].sort(byArrival);
    openConversation(o.m.address, messageName(o.m));
    tug.focusItem = `m${o.m.id}`;
  } else {
    // A text's notification opens its conversation; any other app's (WhatsApp, Teams…) its Feed row.
    const target = notificationPick(o.n);
    if (target.open === "messages") {
      tug.openThread(target.thread);
    } else if (target.open === "feed") {
      tug.view = "feed";
    } else {
      // Cleared app notifications aren't in the feed any more: show them in full here.
      expanded.value = expanded.value === o.n.id ? null : o.n.id;
      return;
    }
    tug.focusItem = `n${o.n.id}`;
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
    <!-- While the screen is shared or presented, people, texts and notifications blur as in the Feed
         (lib/sharePrivacy); Show anyway in the Feed's banner reveals them here too. -->
    <div
      ref="root"
      role="dialog"
      aria-modal="true"
      aria-label="Search"
      :class="[
        'flex max-h-[76vh] w-full max-w-[620px] flex-col overflow-hidden rounded-xl border border-hairline bg-canvas',
        tug.previewsHidden && 'previews-hidden',
      ]"
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
            <kbd class="mx-0.5 rounded border border-hairline px-1.5 font-mono text-[11px]">tugboat</kbd>
            <kbd class="mx-0.5 rounded border border-hairline px-1.5 font-mono text-[11px]">send clipboard</kbd>
            <kbd v-if="tug.settings.showGames" class="mx-0.5 rounded border border-hairline px-1.5 font-mono text-[11px]">game</kbd>
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
                  <span class="preview-private block truncate text-[14px] font-medium text-ink">
                    <template v-for="(r, j) in highlight(cleanName(o.c.name), query)" :key="j">
                      <mark v-if="r.match" class="rounded-sm bg-accent-amber/30 text-ink">{{ r.text }}</mark>
                      <template v-else>{{ r.text }}</template>
                    </template>
                  </span>
                  <span class="preview-private block font-mono text-[12px] text-muted-soft">{{ formatAddress(o.c.address) }}</span>
                </span>
              </template>

              <!-- Recent call -->
              <template v-else-if="o.kind === 'call'">
                <span
                  :class="['grid size-8 shrink-0 place-items-center rounded-lg', CALL_DIRECTION[o.call.direction].tone]"
                  :title="CALL_DIRECTION[o.call.direction].label"
                >
                  <component :is="CALL_DIRECTION[o.call.direction].icon" :size="15" />
                </span>
                <span class="min-w-0 flex-1">
                  <span class="flex items-baseline gap-2">
                    <span
                      :class="[
                        'preview-private truncate text-[14px] font-medium',
                        o.call.direction === 'missed' ? 'text-error' : 'text-ink',
                      ]"
                    >
                      <template v-for="(r, j) in highlight(o.name, query)" :key="j">
                        <mark v-if="r.match" class="rounded-sm bg-accent-amber/30 text-ink">{{ r.text }}</mark>
                        <template v-else>{{ r.text }}</template>
                      </template>
                    </span>
                    <span class="ml-auto shrink-0 font-mono text-[11px] text-muted-soft">{{ callWhen(o.call) }}</span>
                  </span>
                  <span class="flex items-baseline gap-2">
                    <span class="min-w-0 truncate text-[13px] text-muted">
                      {{ CALL_DIRECTION[o.call.direction].label }}<template v-if="showCallNumber(o.call, o.name)"> · <span class="preview-private">{{ formatAddress(o.call.number!) }}</span></template>
                    </span>
                    <!-- What Enter (or a click) does here: call back, call, message, or show it in Calls. -->
                    <span v-if="i === active" class="ml-auto flex shrink-0 items-center gap-1.5 text-[12px] text-muted">
                      {{ o.pick.label }}
                      <kbd class="rounded border border-hairline px-1.5 font-mono text-[11px]">Enter</kbd>
                    </span>
                  </span>
                </span>
              </template>

              <!-- Text message -->
              <template v-else-if="o.kind === 'message'">
                <AppAvatar app-id="com.apple.MobileSMS" :label="messageName(o.m)" :photo-key="o.m.address" person size="sm" />
                <span class="min-w-0 flex-1">
                  <span class="flex items-baseline gap-2">
                    <span class="preview-private truncate text-[14px] font-medium text-ink">{{ messageName(o.m) }}</span>
                    <span class="ml-auto shrink-0 font-mono text-[11px] text-muted-soft">{{ when(messageTime(o.m)) }}</span>
                  </span>
                  <span class="preview-private block text-[13px] text-body">
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
                      {{ appLabel(o.n) }}<template v-if="o.n.title"> · <span class="preview-private">{{ cleanName(o.n.title) }}</span></template>
                    </span>
                    <span v-if="o.n.removedAt" class="shrink-0 text-[11px] text-muted-soft">cleared</span>
                    <span class="ml-auto shrink-0 font-mono text-[11px] text-muted-soft">{{ when(notificationTime(o.n)) }}</span>
                  </span>
                  <span :class="['preview-private block text-[13px] text-body', expanded === o.n.id ? 'whitespace-pre-line' : '']">
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
