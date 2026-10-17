<script setup lang="ts">
// Suggested replies above the composer: one tap sends that text, the same way as typing it and
// pressing Enter. Which replies (if any) is decided by lib/smartReplies `conversationReplies`;
// sending, and its Sending… / Not sent with Retry, belong to MessageThreads.
defineProps<{
  replies: string[];
  /** Who they go to, for screen readers and the tooltip. */
  contact: string;
  /** A send is under way (one tap, one send), or there's no number to send to. */
  disabled: boolean;
}>();
const emit = defineEmits<{ pick: [text: string] }>();
</script>

<template>
  <div v-if="replies.length" class="mb-2 flex flex-wrap gap-1.5 px-1" role="group" aria-label="Suggested replies">
    <button
      v-for="r in replies"
      :key="r"
      type="button"
      class="inline-flex max-w-full items-center rounded-full border border-hairline bg-canvas px-3 py-1 text-[13px] text-ink transition-colors active:bg-surface-card disabled:cursor-not-allowed disabled:text-muted-soft"
      :disabled="disabled"
      :title="`Send “${r}” to ${contact}`"
      :aria-label="`Send “${r}” to ${contact}`"
      @click="emit('pick', r)"
    >
      <span class="truncate">{{ r }}</span>
    </button>
  </div>
</template>
