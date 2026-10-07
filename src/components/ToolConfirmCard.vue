<script setup lang="ts">
// "<tool> wants to text <name>": the only way a text from an AI tool (or `tug text`) is sent.
// Focus starts on Don't send and Esc means Don't send, so a stray Enter can't send anything.
// After 2 minutes it closes as not sent (Rust decides; the countdown is only for show).
import { computed, nextTick, onMounted, onUnmounted, ref } from "vue";
import { MessageSquareText } from "lucide-vue-next";
import { useDevToolsStore } from "../stores/devtools";
import { useFocusTrap } from "../lib/focusTrap";
import { askerName, countdown, secondsLeft } from "../lib/devtools";
import { formatAddress } from "../lib/format";
import type { DevToolsConfirm } from "../types/protocol";

const props = defineProps<{ request: DevToolsConfirm }>();
const devtools = useDevToolsStore();
const root = ref<HTMLElement | null>(null);
const dontSend = ref<HTMLButtonElement | null>(null);
useFocusTrap(root, () => void devtools.answer(false));

const now = ref(Date.now());
let timer: number | undefined;
onMounted(async () => {
  timer = window.setInterval(() => {
    now.value = Date.now();
    if (now.value >= props.request.expiresAt) devtools.confirm = null;
  }, 1000);
  await nextTick();
  dontSend.value?.focus();
});
onUnmounted(() => window.clearInterval(timer));

const left = computed(() => countdown(secondsLeft(props.request.expiresAt, now.value)));
const number = computed(() => formatAddress(props.request.toAddress));
const showNumber = computed(() => number.value !== props.request.toName);
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-ink/40 backdrop-blur-[2px]">
    <div
      ref="root"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="confirm-title"
      aria-describedby="confirm-message"
      class="w-[440px] rounded-xl bg-surface-dark p-8 text-on-dark"
    >
      <p class="caption-upper flex items-center gap-2 text-on-dark-soft">
        <MessageSquareText :size="14" /> Send a text?
      </p>
      <p id="confirm-title" class="mt-2 font-display text-[26px] leading-tight" style="letter-spacing: -0.02em">
        {{ askerName(request.tool) }} wants to text {{ request.toName }}
      </p>
      <p v-if="showNumber" class="mt-1 font-mono text-[13px] text-on-dark-soft">{{ number }}</p>

      <p
        id="confirm-message"
        class="selectable mt-5 max-h-48 overflow-y-auto rounded-lg bg-surface-dark-elevated px-4 py-3 text-[15px] leading-snug whitespace-pre-wrap break-words text-on-dark"
      >{{ request.message }}</p>

      <p class="mt-4 text-[13px] text-on-dark-soft">
        Nothing is sent unless you click Send. Not sent in {{ left }}.
      </p>

      <div class="mt-6 flex justify-end gap-2">
        <button ref="dontSend" class="btn-on-dark" @click="devtools.answer(false)">Don't send</button>
        <button class="btn-primary" @click="devtools.answer(true)">Send</button>
      </div>
    </div>
  </div>
</template>
