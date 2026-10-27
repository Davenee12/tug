<script setup lang="ts">
import { computed, onUnmounted, ref } from "vue";
import { Check, Copy, ExternalLink, MapPin, Package } from "lucide-vue-next";
import type { SmartAction } from "../lib/smartActions";
import { useSmartAction } from "../lib/useSmartAction";

// One compact button for a smart action (lib/smartActions) on a Feed row or a notification:
// "Track parcel", "Open in Maps", "Open link", "Copy number", "Copy email". Like CodeChip, it
// never opens or expands the row it sits in, and a copy shows "Copied" for a moment.
const props = defineProps<{ action: SmartAction }>();
const { run } = useSmartAction();
const copied = ref(false);
let timer: number | undefined;
onUnmounted(() => window.clearTimeout(timer));

const icon = computed(() => {
  if (copied.value) return Check;
  if (props.action.kind === "tracking") return Package;
  if (props.action.kind === "address") return MapPin;
  return props.action.url ? ExternalLink : Copy;
});

async function act() {
  if (!(await run(props.action)) || props.action.url) return;
  copied.value = true;
  window.clearTimeout(timer);
  timer = window.setTimeout(() => (copied.value = false), 1500);
}
</script>

<template>
  <button
    type="button"
    :class="[
      'inline-flex shrink-0 items-center gap-1.5 rounded-full border px-2.5 py-1 text-[12px] whitespace-nowrap transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ink/25',
      copied ? 'border-success/40 bg-success/10 text-ink' : 'border-hairline bg-canvas text-ink active:bg-surface-card',
    ]"
    :title="action.title"
    @click.stop="act"
    @keydown.enter.stop
    @keydown.space.stop
  >
    <component :is="icon" :size="12" class="shrink-0" aria-hidden="true" />
    {{ copied ? "Copied" : action.label }}
  </button>
</template>
