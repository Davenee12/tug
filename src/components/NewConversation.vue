<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { Info, Phone, Search, X } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { useFocusTrap } from "../lib/focusTrap";
import { cleanName, formatAddress, groupConversations } from "../lib/format";
import { normalizeAddress } from "../lib/address";
import AppAvatar from "./AppAvatar.vue";

const tug = useTugStore();
const query = ref("");
const input = ref<HTMLInputElement | null>(null);
const list = ref<HTMLElement | null>(null);
const active = ref(0);
const root = ref<HTMLElement | null>(null);
// Esc closes from anywhere in the dialog, not only from the search box.
useFocusTrap(root, () => close());

onMounted(async () => {
  await nextTick();
  input.value?.focus();
});

interface Option {
  key: string;
  name: string;
  address: string;
  section: "Recent" | "Contacts" | "Number";
}

/** Case- and accent-insensitive: "jose" finds "José". */
const fold = (s: string) =>
  s
    .normalize("NFD")
    .replace(/\p{Diacritic}/gu, "")
    .toLowerCase();

// People you've texted, most recent first, wherever tug knows their number. Unknown senders
// (spam, short codes) stay out, like everywhere else Filter unknown senders applies; a number
// can still be typed.
const recent = computed<Option[]>(() =>
  groupConversations(tug.notifications, tug.messages, tug.contacts)
    .filter((c) => c.appId === "com.apple.MobileSMS" && c.address && tug.isKnown(c))
    .map((c) => ({ key: `r:${c.address}`, name: c.contact, address: c.address!, section: "Recent" as const })),
);

const contacts = computed<Option[]>(() =>
  [...tug.contacts]
    .sort((a, b) => cleanName(a.name).localeCompare(cleanName(b.name)))
    .map((c) => ({ key: `c:${c.address}`, name: cleanName(c.name), address: c.address, section: "Contacts" as const })),
);

const options = computed<Option[]>(() => {
  const q = fold(query.value.trim());
  const digits = query.value.replace(/\D/g, "");
  const seen = new Set<string>();
  const take = (o: Option) => {
    if (seen.has(o.address)) return false;
    seen.add(o.address);
    return true;
  };
  if (!q) {
    // Nothing typed: the people you talk to, then everyone else.
    return [...recent.value.slice(0, 6).filter(take), ...contacts.value.filter(take)];
  }
  const matches = (o: Option) => fold(o.name).includes(q) || (digits.length >= 3 && o.address.includes(digits));
  const out = [...recent.value.filter(matches).filter(take), ...contacts.value.filter(matches).filter(take)];
  // A typed number that isn't a saved contact can still be texted.
  if (digits.length >= 7 && !out.some((o) => o.address.endsWith(digits.slice(-10)))) {
    // +1 only for a bare 10-digit number on a PC in a +1 region; anything else goes as typed and
    // the iPhone reads it the way it would a number typed there.
    const address = normalizeAddress(query.value.trim().startsWith("+") ? `+${digits}` : digits);
    out.unshift({ key: `n:${address}`, name: formatAddress(address), address, section: "Number" });
  }
  return out;
});

watch(query, () => (active.value = 0));

function close() {
  tug.pickerOpen = false;
}

function callFromPicker(o: Option) {
  close();
  void tug.call(o.address, o.name);
}

function pick(o: Option | undefined) {
  if (!o) return;
  tug.startConversation(o.address, o.name);
  close();
}

async function move(delta: number) {
  const n = options.value.length;
  if (!n) return;
  active.value = (active.value + delta + n) % n;
  await nextTick();
  list.value?.querySelector<HTMLElement>(`[data-index="${active.value}"]`)?.scrollIntoView({ block: "nearest" });
}

function onKey(e: KeyboardEvent) {
  if (e.key === "ArrowDown") void move(1);
  else if (e.key === "ArrowUp") void move(-1);
  else if (e.key === "Enter") pick(options.value[active.value]);
  else if (e.key === "Escape") close();
  else return;
  e.preventDefault();
}

const showHeader = (i: number) => i === 0 || options.value[i].section !== options.value[i - 1].section;
const sectionLabel = (s: Option["section"]) => ({ Recent: "Recent", Contacts: "All contacts", Number: "Text a number" })[s];
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-start justify-center bg-ink/30 px-6 pt-[12vh] backdrop-blur-[2px]" @click.self="close">
    <div
      ref="root"
      role="dialog"
      aria-modal="true"
      aria-label="New message"
      class="flex max-h-[70vh] w-full max-w-[520px] flex-col overflow-hidden rounded-xl border border-hairline bg-canvas"
    >
      <header class="flex items-center justify-between px-5 pt-4 pb-2">
        <h2 class="headline text-[24px] leading-none">New message</h2>
        <button class="rounded-md p-1.5 text-muted active:bg-surface-card" aria-label="Close" @click="close">
          <X :size="16" />
        </button>
      </header>

      <div class="px-5 pb-3">
        <div class="relative">
          <Search :size="17" class="pointer-events-none absolute top-1/2 left-3.5 -translate-y-1/2 text-muted-soft" />
          <input
            ref="input"
            v-model="query"
            class="input h-12 pl-11 text-[16px]"
            placeholder="Name or phone number"
            spellcheck="false"
            autocomplete="off"
            @keydown="onKey"
          />
        </div>
      </div>

      <p v-if="tug.contacts.length === 0" class="mx-5 mb-2 flex gap-2 rounded-lg bg-surface-soft px-3 py-2 text-[12px] text-muted">
        <Info :size="13" class="mt-0.5 shrink-0" />
        {{
          tug.status.contactsError ??
          "Your contacts appear once your iPhone shares them (Settings › Bluetooth › ⓘ › Sync Contacts). You can still type a number."
        }}
      </p>

      <ul ref="list" class="min-h-0 flex-1 overflow-y-auto border-t border-hairline-soft px-2 py-2">
        <template v-for="(o, i) in options" :key="o.key">
          <li v-if="showHeader(i)" class="caption-upper px-3 pt-3 pb-1 text-muted-soft">{{ sectionLabel(o.section) }}</li>
          <li :class="['flex items-center rounded-lg', i === active ? 'bg-surface-card' : '']" @mousemove="active = i">
            <button :data-index="i" class="flex min-w-0 flex-1 items-center gap-3 px-3 py-2 text-left" @click="pick(o)">
              <AppAvatar app-id="com.apple.MobileSMS" :label="o.name" person size="sm" />
              <span class="min-w-0 flex-1">
                <span class="block truncate text-[14px] font-medium text-ink">{{ o.name }}</span>
                <span class="block truncate font-mono text-[12px] text-muted-soft">{{ formatAddress(o.address) }}</span>
              </span>
            </button>
            <!-- Experimental: only once Settings has checked hands-free dialing works here. -->
            <button
              v-if="tug.canDial && !o.address.includes('@')"
              class="mr-2 shrink-0 rounded-md p-1.5 text-muted-soft hover:bg-surface-cream-strong hover:text-ink"
              :aria-label="`Call ${o.name}`"
              :title="`Call ${o.name} on your iPhone (experimental)`"
              :disabled="tug.calling !== null"
              @click="callFromPicker(o)"
            >
              <Phone :size="15" />
            </button>
          </li>
        </template>
        <li v-if="query && options.length === 0" class="px-3 py-6 text-center text-[13px] text-muted-soft">
          No one matches “{{ query }}”. Type a full number to text it.
        </li>
      </ul>

      <footer class="flex gap-4 border-t border-hairline-soft px-5 py-2.5 font-mono text-[11px] text-muted-soft">
        <span>↑↓ move</span><span>Enter start</span><span>Esc close</span>
      </footer>
    </div>
  </div>
</template>
