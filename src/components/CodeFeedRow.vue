<script setup lang="ts">
import { X } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { MESSAGES_APP, relativeTime } from "../lib/format";
import type { CodeEntry } from "../lib/codeFeed";
import AppAvatar from "./AppAvatar.vue";
import CodeChip from "./CodeChip.vue";

// A verification code that arrived as a text with no notification behind it. The row looks like a
// conversation (the body opens it in Messages), with a prominent Copy code and a Clear that hides
// the row without touching the text.
const props = defineProps<{ entry: CodeEntry }>();
const tug = useTugStore();

const when = () => relativeTime(new Date(props.entry.at));
</script>

<template>
  <div class="rounded-xl transition-colors">
    <div
      role="button"
      tabindex="0"
      class="flex w-full cursor-default items-center gap-3 rounded-xl px-3 py-2.5 text-left active:bg-surface-soft"
      @click="tug.openThread(entry.conversationKey)"
      @keydown.enter.self="tug.openThread(entry.conversationKey)"
    >
      <AppAvatar :app-id="MESSAGES_APP" :label="entry.sender" person size="sm" />
      <span class="min-w-0 flex-1">
        <span class="flex items-baseline gap-2">
          <span class="truncate text-[14px] font-semibold text-ink">{{ entry.sender }}</span>
          <span class="shrink-0 text-[12px] text-muted-soft">Verification code</span>
          <span class="ml-auto shrink-0 font-mono text-[11px] text-muted-soft">{{ when() }}</span>
        </span>
        <span class="mt-0.5 block truncate text-[13px] text-body-strong">{{ entry.body }}</span>
      </span>
      <CodeChip :code="entry.code.code" />
      <span
        role="button"
        tabindex="0"
        class="shrink-0 rounded-md p-1 text-muted-soft active:bg-surface-card"
        :title="`Clear this code from the Feed (the text stays in Messages)`"
        @click.stop="tug.clearCode(entry)"
        @keydown.enter.stop="tug.clearCode(entry)"
      >
        <X :size="14" />
      </span>
    </div>
  </div>
</template>
