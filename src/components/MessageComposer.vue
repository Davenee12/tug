<script setup lang="ts">
// The reply box under a conversation. Its own component so a keystroke re-renders just this, not
// the conversation above it. The drafts (one per conversation) stay with MessageThreads, which
// sends; this only shows and edits the one for `draftKey`.
import { computed } from "vue";
import { Clock, SendHorizontal } from "lucide-vue-next";
import TimeMenu from "./TimeMenu.vue";

const props = defineProps<{
  /** Every conversation's draft, by conversation key (reactive; read here, written by the parent). */
  drafts: ReadonlyMap<string, string>;
  draftKey: string;
  placeholder: string;
  /** A send is under way, or there's no number to send to yet. */
  blocked: boolean;
}>();
const emit = defineEmits<{ update: [text: string]; send: []; schedule: [at: number, label: string] }>();

const text = computed({
  get: () => props.drafts.get(props.draftKey) ?? "",
  set: (value: string) => emit("update", value),
});

function onKey(e: KeyboardEvent) {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    emit("send");
  }
}
</script>

<template>
  <form class="flex items-end gap-2" @submit.prevent="emit('send')">
    <textarea
      v-model="text"
      rows="1"
      class="input h-auto max-h-32 min-h-10 resize-none py-2.5 leading-snug"
      :placeholder="placeholder"
      @keydown="onKey"
    />
    <TimeMenu
      heading="Send later"
      title="Send later"
      up
      :disabled="!text.trim() || blocked"
      button-class="btn-secondary w-10 px-0"
      @pick="(at, label) => emit('schedule', at, label)"
    >
      <template #button><Clock :size="16" /></template>
    </TimeMenu>
    <button type="submit" class="btn-primary w-10 shrink-0 px-0" :disabled="!text.trim() || blocked" aria-label="Send">
      <SendHorizontal :size="16" />
    </button>
  </form>
</template>
