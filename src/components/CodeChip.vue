<script setup lang="ts">
import { ref } from "vue";
import { Check, Copy } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import type { PhoneNotification } from "../types/protocol";

// One-click copy for a one-time code, wherever it shows up.
const props = defineProps<{ code: string; from?: PhoneNotification[] }>();
const tug = useTugStore();
const copied = ref(false);
let timer: number | undefined;

async function copy() {
  if (!(await tug.copyCode(props.code, props.from))) return;
  copied.value = true;
  window.clearTimeout(timer);
  timer = window.setTimeout(() => (copied.value = false), 1500);
}
</script>

<template>
  <button
    type="button"
    :class="[
      'inline-flex shrink-0 items-center gap-1.5 rounded-full border px-2.5 py-1 text-[12px] transition-colors',
      copied ? 'border-success/40 bg-success/10 text-ink' : 'border-hairline bg-canvas text-ink active:bg-surface-card',
    ]"
    :title="`Copy ${code} (Ctrl+Shift+C copies the latest code)`"
    :aria-label="copied ? `Copied ${code}` : `Copy code ${code}`"
    @click.stop="copy"
    @keydown.enter.stop
  >
    <component :is="copied ? Check : Copy" :size="12" class="shrink-0" />
    <span v-if="copied">Copied</span>
    <span v-else class="font-mono tracking-wider">{{ code }}</span>
  </button>
</template>
