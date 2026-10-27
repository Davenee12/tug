<script setup lang="ts">
import { computed } from "vue";
import { smartSegments, type SmartAction } from "../lib/smartActions";
import { useSmartAction } from "../lib/useSmartAction";

// A message's words with its links, numbers, addresses and parcel numbers made clickable
// (lib/smartActions). Inline and still selectable: the rest of the text copies as before.
const props = defineProps<{ text: string }>();
const segments = computed(() => smartSegments(props.text));
const { run } = useSmartAction();

function onClick(action: SmartAction) {
  // Dragging across a link to select the text isn't a click on it.
  const selection = window.getSelection();
  if (selection && !selection.isCollapsed && selection.toString().trim()) return;
  void run(action);
}

function onKey(e: KeyboardEvent, action: SmartAction) {
  // Links open with Enter; buttons (copy) with Enter or Space, as everywhere else.
  if (e.key === "Enter" || (e.key === " " && !action.url)) {
    e.preventDefault();
    e.stopPropagation();
    void run(action);
  }
}
</script>

<template>
  <span>
    <template v-for="(s, i) in segments" :key="i">
      <template v-if="s.kind === 'text'">{{ s.text }}</template>
      <span
        v-else
        :role="s.action.url ? 'link' : 'button'"
        tabindex="0"
        :title="s.action.title"
        :aria-label="s.action.title"
        class="cursor-pointer rounded-sm underline decoration-current/40 underline-offset-2 box-decoration-clone wrap-anywhere outline-none hover:decoration-current focus-visible:ring-2 focus-visible:ring-current"
        @click.stop="onClick(s.action)"
        @keydown="onKey($event, s.action)"
      >{{ s.text }}</span>
    </template>
  </span>
</template>
