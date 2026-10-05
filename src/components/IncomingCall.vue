<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref } from "vue";
import { Phone, PhoneOff, X } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { useFocusTrap } from "../lib/focusTrap";
import { appLabel, cleanName, duration, initials } from "../lib/format";
import type { PhoneNotification } from "../types/protocol";

// The phone is ringing: a card over everything, answered or declined on the iPhone (ANCS
// actions). It goes as soon as the phone stops ringing, from here or anywhere else.
const props = defineProps<{ call: PhoneNotification }>();
const tug = useTugStore();
const root = ref<HTMLElement | null>(null);
const answerButton = ref<HTMLButtonElement | null>(null);
const busy = ref<"answer" | "decline" | null>(null);

const name = computed(() => cleanName(props.call.title) || "Unknown caller");
const source = computed(() => appLabel(props.call));
// iOS puts "mobile", "iPhone" and the like here; a bare "Incoming Call" says nothing new.
const detail = computed(() => {
  const d = props.call.subtitle || props.call.message;
  return d && !/^incoming (voice |video )?call$/i.test(d.trim()) ? d : "";
});
const canAnswer = computed(() => props.call.flags.positiveAction);
const canDecline = computed(() => props.call.flags.negativeAction);

// How long it's been ringing, ticking.
const now = ref(Date.now());
const ticker = window.setInterval(() => (now.value = Date.now()), 1000);
const ringingFor = computed(() => duration(Math.max(0, (now.value - props.call.receivedAt) / 1000)));

async function respond(answer: boolean) {
  if (busy.value) return;
  busy.value = answer ? "answer" : "decline";
  await tug.respondToCall(props.call, answer);
  busy.value = null;
}

// Esc only hides the card (the phone keeps ringing), as Esc closes everything else in tug: a
// reflex must never hang up on someone. Declining is the Decline button. Enter answers once the
// Answer button has focus, which it gets after a beat: a keypress meant for whatever you were
// typing when the call came in mustn't pick it up.
useFocusTrap(root, () => tug.hideCall(props.call.id));
let armTimer: number | undefined;
onMounted(async () => {
  await nextTick();
  root.value?.focus();
  armTimer = window.setTimeout(() => (canAnswer.value ? answerButton.value : root.value)?.focus(), 700);
});
onUnmounted(() => {
  window.clearInterval(ticker);
  window.clearTimeout(armTimer);
});
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-ink/50 px-6 backdrop-blur-sm">
    <div
      ref="root"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="call-name"
      aria-describedby="call-detail"
      tabindex="-1"
      class="animate-call-in relative w-full max-w-[380px] overflow-hidden rounded-3xl bg-surface-dark px-8 pt-9 pb-7 text-center text-on-dark outline-none"
    >
      <div class="call-glow pointer-events-none absolute inset-x-0 top-0 h-72" aria-hidden="true" />
      <button
        class="absolute top-4 right-4 rounded-md p-1.5 text-on-dark-soft active:bg-surface-dark-elevated"
        aria-label="Hide (your iPhone keeps ringing)"
        title="Hide here; your iPhone keeps ringing"
        @click="tug.hideCall(call.id)"
      >
        <X :size="16" />
      </button>

      <p class="caption-upper relative text-on-dark-soft">{{ source }} · Incoming call</p>

      <div class="relative mx-auto mt-9 grid size-28 place-items-center" aria-hidden="true">
        <span class="animate-call-ring absolute inset-0 rounded-full border-2 border-primary/60" />
        <span class="animate-call-ring absolute inset-0 rounded-full border-2 border-primary/60 [animation-delay:1.2s]" />
        <span class="relative grid size-24 place-items-center rounded-full bg-primary/20 font-display text-[38px] text-on-dark">
          {{ initials(name) }}
        </span>
      </div>

      <p id="call-name" class="relative mt-8 font-display text-[44px] leading-[1.05] break-words text-on-dark" style="letter-spacing: -0.02em">
        {{ name }}
      </p>
      <p id="call-detail" class="relative mt-2 text-[14px] text-on-dark-soft">
        <template v-if="detail">{{ detail }} · </template>ringing on your iPhone
        <span class="font-mono text-[12px]">{{ ringingFor }}</span>
      </p>

      <div class="relative mt-9 flex justify-center gap-3">
        <button
          v-if="canDecline"
          class="btn-on-dark h-12 rounded-full px-6"
          :disabled="busy !== null"
          @click="respond(false)"
        >
          <PhoneOff :size="17" class="text-error" />
          {{ busy === "decline" ? "Declining…" : "Decline" }}
        </button>
        <button
          v-if="canAnswer"
          ref="answerButton"
          class="btn-primary h-12 rounded-full px-7"
          :disabled="busy !== null"
          @click="respond(true)"
        >
          <Phone :size="17" />
          {{ busy === "answer" ? "Answering…" : "Answer" }}
        </button>
      </div>
      <p class="relative mt-5 font-mono text-[11px] text-on-dark-soft">
        <template v-if="canAnswer">Enter answers · </template>Esc hides
      </p>
    </div>
  </div>
</template>
