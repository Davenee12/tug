<script setup lang="ts">
// "<tool> wants to text <name>" or "Send report.pdf (2.1 MB) to your iPhone?": the only way a
// text or a file from an AI tool (or `tug text`, `tug boat`) is sent. Focus starts on Don't send
// and Esc means Don't send, so a stray Enter can't send anything. Send stays disabled for a
// moment after the card appears and whenever tug's window isn't in front, so a click or key
// press meant for something else can't land on it. Every answer carries this card's id: it can
// never answer a different card. After 2 minutes it closes as not sent (Rust decides; the
// countdown is only for show).
import { computed, nextTick, onMounted, onUnmounted, ref } from "vue";
import { FileUp, MessageSquareText } from "lucide-vue-next";
import { useDevToolsStore } from "../stores/devtools";
import { useFocusTrap } from "../lib/focusTrap";
import { askerName, countdown, fileFrom, filesQuestion, secondsLeft, sendArmed } from "../lib/devtools";
import { formatAddress } from "../lib/format";
import { formatSize } from "../lib/tugboat";
import type { DevToolsConfirm } from "../types/protocol";

const props = defineProps<{ request: DevToolsConfirm }>();
const devtools = useDevToolsStore();
const root = ref<HTMLElement | null>(null);
const dontSend = ref<HTMLButtonElement | null>(null);
const messageBox = ref<HTMLElement | null>(null);
const answer = (send: boolean) => void devtools.answer(props.request.id, send);
useFocusTrap(root, () => answer(false));

const shownAt = Date.now();
const now = ref(shownAt);
const focused = ref(document.hasFocus());
const overflowing = ref(false);
const onFocus = () => {
  focused.value = true;
  now.value = Date.now();
};
const onBlur = () => (focused.value = false);
let timer: number | undefined;
onMounted(async () => {
  window.addEventListener("focus", onFocus);
  window.addEventListener("blur", onBlur);
  // Ticks often enough to arm Send on time; the countdown only needs seconds.
  timer = window.setInterval(() => {
    now.value = Date.now();
    focused.value = document.hasFocus();
    if (now.value >= props.request.expiresAt && devtools.confirm?.id === props.request.id) devtools.confirm = null;
  }, 250);
  await nextTick();
  dontSend.value?.focus();
  const box = messageBox.value;
  overflowing.value = !!box && box.scrollHeight > box.clientHeight + 1;
});
onUnmounted(() => {
  window.clearInterval(timer);
  window.removeEventListener("focus", onFocus);
  window.removeEventListener("blur", onBlur);
});

const armed = computed(() => sendArmed(shownAt, now.value, focused.value));
const left = computed(() => countdown(secondsLeft(props.request.expiresAt, now.value)));
const number = computed(() => (props.request.kind === "text" ? formatAddress(props.request.toAddress) : ""));
const showNumber = computed(() => props.request.kind === "text" && number.value !== props.request.toName);
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
      <template v-if="request.kind === 'text'">
        <p class="caption-upper flex items-center gap-2 text-on-dark-soft">
          <MessageSquareText :size="14" /> Send a text?
        </p>
        <p id="confirm-title" class="mt-2 font-display text-[26px] leading-tight" style="letter-spacing: -0.02em">
          {{ askerName(request.tool) }} wants to text {{ request.toName }}
        </p>
        <p v-if="showNumber" class="mt-1 font-mono text-[13px] text-on-dark-soft">{{ number }}</p>

        <p
          id="confirm-message"
          ref="messageBox"
          tabindex="0"
          class="selectable mt-5 max-h-48 overflow-y-auto rounded-lg bg-surface-dark-elevated px-4 py-3 text-[15px] leading-snug whitespace-pre-wrap break-words text-on-dark"
        >{{ request.message }}</p>
        <p v-if="overflowing" class="mt-2 text-[12px] text-accent-amber">
          This message is longer than the box. Scroll through all of it before you send.
        </p>
      </template>

      <template v-else>
        <p class="caption-upper flex items-center gap-2 text-on-dark-soft">
          <FileUp :size="14" /> Send files to your iPhone?
        </p>
        <p id="confirm-title" class="mt-2 font-display text-[26px] leading-tight break-words" style="letter-spacing: -0.02em">
          {{ filesQuestion(request.files) }}
        </p>
        <p class="mt-1 text-[13px] text-on-dark-soft">{{ askerName(request.tool) }} is asking. They'll go to your iPhone with Tugboat.</p>

        <ul
          id="confirm-message"
          ref="messageBox"
          tabindex="0"
          class="selectable mt-5 max-h-48 space-y-2 overflow-y-auto rounded-lg bg-surface-dark-elevated px-4 py-3"
        >
          <li v-for="(f, i) in request.files" :key="i" class="min-w-0">
            <p class="flex items-baseline gap-2 text-[15px] leading-snug text-on-dark">
              <span class="min-w-0 flex-1 break-words">{{ f.name }}</span>
              <span class="shrink-0 text-[13px] text-on-dark-soft">{{ formatSize(f.size) }}</span>
            </p>
            <p class="font-mono text-[12px] break-all text-on-dark-soft">{{ fileFrom(f) }}</p>
          </li>
        </ul>
        <p v-if="overflowing" class="mt-2 text-[12px] text-accent-amber">
          There are more files than fit in the box. Scroll through all of them before you send.
        </p>
      </template>

      <p class="mt-4 text-[13px] text-on-dark-soft">
        Nothing is sent unless you click Send. Not sent in {{ left }}.
      </p>

      <div class="mt-6 flex justify-end gap-2">
        <button ref="dontSend" class="btn-on-dark" @click="answer(false)">Don't send</button>
        <button class="btn-primary disabled:opacity-50" :disabled="!armed" @click="armed && answer(true)">Send</button>
      </div>
    </div>
  </div>
</template>
