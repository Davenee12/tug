<script setup lang="ts">
// One conversation in the Messages list (known ones, and the Unknown senders section).
import { X } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { relativeTime, type Conversation, type ConversationItem } from "../lib/format";
import AppAvatar from "./AppAvatar.vue";

defineProps<{ c: Conversation; active: boolean }>();
defineEmits<{ select: [] }>();

const tug = useTugStore();
const outgoing = (i: ConversationItem) => i.kind === "message" && i.m.direction === "out";
</script>

<template>
  <div class="group relative">
    <button
      :class="['flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left', active ? 'bg-surface-card' : 'active:bg-surface-soft']"
      @click="$emit('select')"
    >
      <AppAvatar :app-id="c.appId" :label="c.contact" :photo-key="c.address ?? undefined" person size="sm" />
      <span class="min-w-0 flex-1">
        <span class="flex items-baseline gap-2">
          <span :class="['truncate text-[14px] text-ink', tug.newCount(c.key, c.notifications) ? 'font-semibold' : 'font-medium']">
            {{ c.contact }}
          </span>
          <span
            v-if="c.items.length"
            class="ml-auto shrink-0 font-mono text-[11px] text-muted-soft group-focus-within:opacity-0 group-hover:opacity-0"
          >
            {{ relativeTime(c.latest.at) }}
          </span>
        </span>
        <span class="flex items-center gap-2">
          <span class="min-w-0 flex-1 truncate text-[13px] text-muted">
            <template v-if="!c.items.length">New message</template>
            <template v-else-if="outgoing(c.latest)">You: </template>{{ c.latest.body }}
          </span>
          <span
            v-if="tug.newCount(c.key, c.notifications)"
            class="flex h-[18px] min-w-[18px] shrink-0 items-center justify-center rounded-full bg-ink px-1 text-[11px] font-semibold text-on-dark"
          >
            {{ tug.newCount(c.key, c.notifications) }}
          </span>
        </span>
      </span>
    </button>
    <!-- Delete from tug (not the phone); Undo in the toast. -->
    <button
      v-if="c.items.length"
      class="absolute top-2 right-2 rounded-md p-1 text-muted-soft opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 hover:bg-surface-cream-strong hover:text-ink focus-visible:opacity-100"
      :aria-label="`Delete conversation with ${c.contact}`"
      :title="`Delete from tug (your iPhone keeps it)`"
      @click.stop="tug.deleteConversation(c)"
    >
      <X :size="14" />
    </button>
  </div>
</template>
