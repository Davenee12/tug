<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";
import { Info, Search } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { formatAddress } from "../lib/format";
import AppAvatar from "./AppAvatar.vue";

const emit = defineEmits<{ close: [] }>();
const tug = useTugStore();
const query = ref("");
const input = ref<HTMLInputElement | null>(null);
onMounted(async () => {
  await nextTick();
  input.value?.focus();
});

interface Option {
  key: string;
  name: string;
  address: string;
}

// Contacts from the phone (or learned), one option per number.
const options = computed<Option[]>(() => {
  const q = query.value.trim().toLowerCase();
  const digits = q.replace(/\D/g, "");
  const matches = tug.contacts
    .filter((c) => !q || c.name.toLowerCase().includes(q) || (digits.length >= 3 && c.address.includes(digits)))
    .slice(0, 50)
    .map((c) => ({ key: c.address, name: c.name, address: c.address }));
  // A typed number that isn't a saved contact can still be texted.
  if (digits.length >= 7 && !matches.some((m) => m.address.endsWith(digits.slice(-10)))) {
    const address = digits.length === 10 ? `+1${digits}` : q.startsWith("+") ? `+${digits}` : digits;
    matches.unshift({ key: `typed:${address}`, name: formatAddress(address), address });
  }
  return matches;
});

function pick(o: Option) {
  tug.startConversation(o.address, o.name);
  emit("close");
}

function onKey(e: KeyboardEvent) {
  if (e.key === "Enter" && options.value[0]) pick(options.value[0]);
  if (e.key === "Escape") emit("close");
}
</script>

<template>
  <div class="flex flex-col gap-2 px-3 py-2">
    <div class="relative">
      <Search :size="15" class="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-muted-soft" />
      <input
        ref="input"
        v-model="query"
        class="input pl-9"
        placeholder="Name or number"
        spellcheck="false"
        @keydown="onKey"
      />
    </div>
    <p v-if="tug.contacts.length === 0" class="flex gap-2 px-1 text-[12px] text-muted">
      <Info :size="13" class="mt-0.5 shrink-0" />
      {{
        tug.status.contactsError ??
        "Your contacts appear once your iPhone shares them (Settings › Bluetooth › ⓘ › Sync Contacts). You can still type a number."
      }}
    </p>
    <ul class="flex max-h-80 flex-col overflow-y-auto">
      <li v-for="o in options" :key="o.key">
        <button class="flex w-full items-center gap-3 rounded-lg px-2 py-2 text-left active:bg-surface-soft" @click="pick(o)">
          <AppAvatar app-id="com.apple.MobileSMS" :label="o.name" size="sm" />
          <span class="min-w-0 flex-1">
            <span class="block truncate text-[14px] font-medium text-ink">{{ o.name }}</span>
            <span class="block truncate font-mono text-[12px] text-muted-soft">{{ formatAddress(o.address) }}</span>
          </span>
        </button>
      </li>
      <li v-if="query && options.length === 0" class="px-2 py-3 text-[13px] text-muted-soft">No matching contacts.</li>
    </ul>
  </div>
</template>
