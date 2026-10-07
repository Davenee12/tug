<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { MessageSquare, Phone, PhoneIncoming, PhoneMissed, PhoneOutgoing } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { callsEmptyHint } from "../lib/availability";
import { callKey, callKeys, callName, callTime, clockTime, formatAddress, groupCalls, MESSAGES_APP } from "../lib/format";
import { preservedScrollTop } from "../lib/scroll";
import type { CallDirection, CallRecord } from "../types/protocol";
import AppAvatar from "./AppAvatar.vue";

// Recents from the iPhone (PBAP call history): who, which way, when. Message anyone back.
// Click a person to call them: back from their missed call (works without hands-free), or by
// dialing once Settings has checked that hands-free works on this PC.
const tug = useTugStore();
// Calls placed on the phone send nothing tug can see, so while this list is open (and tug is
// on screen) ask for it every few seconds; the backend keeps pulls at least 10 s apart.
const LIVE_MS = 10_000;
let live: number | undefined;
onMounted(() => {
  tug.refreshCalls();
  live = window.setInterval(() => document.visibilityState === "visible" && tug.refreshCalls(), LIVE_MS);
});
onUnmounted(() => window.clearInterval(live));

const nameFor = computed(() => new Map(tug.contacts.map((c) => [c.address, c.name])));
const groups = computed(() => groupCalls(tug.calls));

// A stable key per call so a new call at the top doesn't re-key (and so re-render) every row below
// it. Built from the same objects groupCalls returns, so a per-render identity lookup is enough.
const keyMap = computed(() => {
  const keys = callKeys(tug.calls);
  const m = new Map<CallRecord, string>();
  tug.calls.forEach((c, i) => m.set(c, keys[i]));
  return m;
});
const keyOf = (c: CallRecord) => keyMap.value.get(c) ?? callKey(c);

// Hold the reader's place as calls arrive. The list is newest-first, so a new call lands at the top:
// left alone the viewport would slide down by the new row's height. Measure before the DOM updates
// (this watcher runs pre-flush), then restore — stay pinned at the top if they were already there
// (so the newest call is seen), otherwise keep the rows under their eye still.
const scroller = ref<HTMLElement | null>(null);
const topKey = (gs: Array<{ calls: CallRecord[] }>): string | undefined => {
  const c = gs[0]?.calls[0];
  return c ? callKey(c) : undefined;
};
watch(groups, async (next, prev) => {
  const el = scroller.value;
  if (!el) return;
  const prevTop = el.scrollTop;
  const prevHeight = el.scrollHeight;
  const prependedAtTop = topKey(next) !== topKey(prev ?? []);
  await nextTick();
  const after = scroller.value;
  if (!after) return;
  const top = preservedScrollTop({ prevTop, prevHeight, newHeight: after.scrollHeight, prependedAtTop });
  if (top !== null) after.scrollTop = top;
});

const DIRECTION: Record<CallDirection, { label: string; icon: typeof Phone; tone: string }> = {
  incoming: { label: "Incoming", icon: PhoneIncoming, tone: "bg-accent-teal/20 text-ink" },
  outgoing: { label: "Outgoing", icon: PhoneOutgoing, tone: "bg-surface-cream-strong text-body-strong" },
  missed: { label: "Missed", icon: PhoneMissed, tone: "bg-error/10 text-error" },
};

const name = (c: CallRecord) => callName(c, nameFor.value);
const route = (c: CallRecord) => tug.callRoute(name(c), c.number);
// Every row with a number can be clicked: it calls when tug can, and says why when it can't
// (rows that look tappable but do nothing were confusing).
function callFrom(c: CallRecord) {
  if (c.number) void tug.callPerson(name(c), c.number);
}
// The number goes under the name only when the name isn't the number already.
const showNumber = (c: CallRecord) => !!c.number && name(c) !== formatAddress(c.number);
// A caller's photo (when the iPhone shared one) stands in for the direction tile; direction stays
// in the row's subtitle. A number finds it most reliably, falling back to the name.
const photoKey = (c: CallRecord) => c.number ?? name(c);
const photo = (c: CallRecord) => tug.contactPhoto(photoKey(c));
const time = (c: CallRecord) => {
  const at = callTime(c);
  return at ? clockTime(at) : "";
};

// Same answer as the Sync Contacts switch (lib/availability): never "turn it on" while it's on.
const emptyHint = computed(() => callsEmptyHint(tug.status, tug.switchContext));
</script>

<template>
  <div ref="scroller" class="flex min-h-0 flex-1 flex-col overflow-y-auto px-5 pb-10">
    <div v-if="tug.calls.length === 0" class="flex flex-1 items-center justify-center py-10">
      <div class="max-w-lg text-center">
        <p class="headline text-[28px]">No recent calls yet</p>
        <p class="mt-2 text-[14px] text-muted">{{ emptyHint }}</p>
      </div>
    </div>

    <div class="mx-auto w-full max-w-3xl">
      <section v-for="g in groups" :key="g.label">
        <h2 class="caption-upper sticky top-0 z-10 bg-canvas/95 px-3 pt-5 pb-2 text-muted backdrop-blur-sm">{{ g.label }}</h2>
        <ul class="flex flex-col gap-0.5">
          <li
            v-for="c in g.calls"
            :key="keyOf(c)"
            :class="['group flex items-center gap-3 rounded-xl px-3 py-2.5 hover:bg-surface-soft', c.number ? 'cursor-pointer' : '']"
            :title="c.number ? `Call ${name(c)}${route(c) === 'back' ? ' back' : ''} on your iPhone` : undefined"
            @click="callFrom(c)"
          >
            <AppAvatar
              v-if="photo(c)"
              :app-id="MESSAGES_APP"
              :label="name(c)"
              :photo-key="photoKey(c)"
              person
              size="sm"
            />
            <span
              v-else
              :class="['grid size-8 shrink-0 place-items-center rounded-lg', DIRECTION[c.direction].tone]"
              :title="DIRECTION[c.direction].label"
            >
              <component :is="DIRECTION[c.direction].icon" :size="15" />
            </span>
            <span class="min-w-0 flex-1">
              <span class="flex items-baseline gap-2">
                <span :class="['truncate text-[14px] font-medium', c.direction === 'missed' ? 'text-error' : 'text-ink']">
                  {{ name(c) }}
                </span>
                <span class="ml-auto shrink-0 font-mono text-[11px] text-muted-soft">{{ time(c) }}</span>
              </span>
              <span class="mt-0.5 block truncate text-[13px] text-muted">
                {{ DIRECTION[c.direction].label }}<template v-if="showNumber(c)"> · {{ formatAddress(c.number!) }}</template>
              </span>
            </span>
            <template v-if="c.number">
              <button
                class="shrink-0 rounded-md p-1.5 text-muted-soft opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 hover:bg-surface-card hover:text-ink focus-visible:opacity-100"
                :aria-label="`Message ${name(c)}`"
                :title="`Message ${name(c)}`"
                @click.stop="tug.startConversation(c.number, name(c))"
              >
                <MessageSquare :size="15" />
              </button>
              <button
                :class="['btn-secondary btn-sm shrink-0', route(c) ? '' : 'text-muted']"
                :disabled="tug.calling !== null"
                :title="`Call ${name(c)} on your iPhone`"
                @click.stop="callFrom(c)"
              >
                <Phone :size="13" />
                {{ tug.calling === c.number ? "Calling…" : route(c) === "back" || c.direction === "missed" ? "Call back" : "Call" }}
              </button>
            </template>
            <!-- Same width as the Message button, so times line up on rows without a number. -->
            <span v-else class="size-[27px] shrink-0" aria-hidden="true" />
          </li>
        </ul>
      </section>

      <p v-if="tug.calls.length" class="px-3 pt-6 text-[12px] text-muted-soft">
        Your iPhone's last {{ tug.calls.length }} calls. Calls themselves happen on the phone.
        <button
          v-if="!tug.canDial"
          class="underline active:text-ink"
          @click="tug.openSettings('iphone')"
        >
          Calling from tug (experimental)
        </button>
      </p>
    </div>
  </div>
</template>
