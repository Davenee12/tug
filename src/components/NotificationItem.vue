<script setup lang="ts">
import { computed, ref } from "vue";
import { Bell, BellOff } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { appLabel, clockTime, notificationTime } from "../lib/format";
import type { PhoneNotification } from "../types/protocol";
import AppAvatar from "./AppAvatar.vue";

// `compact`: inside an expanded app stack, where the avatar and app name are already shown.
const props = defineProps<{ n: PhoneNotification; compact?: boolean }>();
const tug = useTugStore();

const label = computed(() => appLabel(props.n));
const time = computed(() => clockTime(notificationTime(props.n)));
const muted = computed(() => tug.settings.mutedApps.includes(props.n.appId));
const expanded = ref(false);
const busy = ref<"positive" | "negative" | null>(null);
const isCall = computed(() => props.n.category === "incomingCall");

async function act(positive: boolean) {
  busy.value = positive ? "positive" : "negative";
  await tug.performAction(props.n.id, positive);
  busy.value = null;
}
</script>

<template>
  <article
    :class="['flex gap-4 rounded-xl transition-colors', compact ? 'px-3 py-2.5' : 'px-4 py-4', n.removedAt ? 'opacity-70' : '']"
  >
    <AppAvatar v-if="!compact" :app-id="n.appId" :label="label" />
    <div class="min-w-0 flex-1">
      <div class="flex items-baseline gap-2">
        <span v-if="!compact" class="text-[13px] font-medium text-muted">{{ label }}</span>
        <span v-if="n.flags.important" class="pill bg-surface-card px-2 py-0 text-[11px] text-ink">Important</span>
        <span v-if="n.removedAt" class="text-[12px] text-muted-soft">· cleared on iPhone</span>
        <span class="ml-auto shrink-0 font-mono text-[12px] text-muted-soft">{{ time }}</span>
      </div>
      <p v-if="n.title" :class="['text-ink', compact ? 'text-[14px] font-medium' : 'mt-0.5 text-[15px] font-medium']">{{ n.title }}</p>
      <p v-if="n.subtitle" class="text-[14px] text-body-strong">{{ n.subtitle }}</p>
      <p
        v-if="n.message"
        :class="['selectable mt-0.5 cursor-text whitespace-pre-line text-[14px] text-body', expanded ? '' : 'line-clamp-4']"
        @click="expanded = !expanded"
      >
        {{ n.message }}
      </p>

      <div :class="['flex items-center gap-2', compact ? 'mt-1.5' : 'mt-2.5']">
        <template v-if="n.live">
          <button
            v-if="n.flags.positiveAction"
            :class="[isCall ? 'btn-primary' : 'btn-secondary', 'btn-sm']"
            :disabled="busy !== null"
            @click="act(true)"
          >
            {{ n.category === "missedCall" ? "Call back" : n.positiveLabel || "Accept" }}
          </button>
          <button v-if="n.flags.negativeAction" class="btn-secondary btn-sm" :disabled="busy !== null" @click="act(false)">
            {{ n.negativeLabel || "Dismiss" }}
          </button>
        </template>
        <button
          class="ml-auto rounded-md p-1.5 text-muted-soft active:bg-surface-card"
          :title="muted ? `Unmute ${label} alerts` : `Mute ${label} alerts on this PC`"
          @click="tug.toggleMuted(n.appId)"
        >
          <BellOff v-if="muted" :size="15" />
          <Bell v-else :size="15" />
        </button>
      </div>
    </div>
  </article>
</template>
