<script setup lang="ts">
import { computed, ref } from "vue";
import { Bell, BellOff, ExternalLink } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { actionLabels, appLabel, clockTime, MESSAGES_APP, notificationTime } from "../lib/format";
import { textContent } from "../lib/messageType";
import { mayHaveWebsite, webLinkFor, type WebLink } from "../lib/weblinks";
import { useCallButtonGate } from "../lib/useCallButtonGate";
import type { PhoneNotification } from "../types/protocol";
import { findCode } from "../lib/codes";
import { smartChips } from "../lib/smartActions";
import AppAvatar from "./AppAvatar.vue";
import CodeChip from "./CodeChip.vue";
import MediaChip from "./MediaChip.vue";
import SmartChip from "./SmartChip.vue";

// `compact`: inside an expanded app stack, where the avatar and app name are already shown.
const props = defineProps<{ n: PhoneNotification; compact?: boolean }>();
const tug = useTugStore();

const label = computed(() => appLabel(props.n));
const time = computed(() => clockTime(notificationTime(props.n)));
const muted = computed(() => tug.settings.mutedApps.includes(props.n.appId));
// A web page we can open for this app (Gmail deep search, LinkedIn notifications…), or null.
// Every app can be opened: its best page when tug knows one, else the app's own website.
const webLink = computed<WebLink | null>(() => {
  if (!(true)) return null;
  const known = webLinkFor(props.n);
  if (known) return known;
  const site = mayHaveWebsite(props.n.appId) ? tug.websiteFor(props.n.appId) : null;
  return site ? { url: site, label: appLabel(props.n) } : null;
});
// A text's photo ("Attachment: 1 Photo") reads as a chip under any caption (lib/attachments).
const content = computed(() => (props.n.appId === MESSAGES_APP && props.n.message ? textContent(props.n.message) : null));
const hasChip = computed(() => !!content.value && (!!content.value.media || content.value.unreadable));
// A one-time code in any app's notification (a WhatsApp or bank code in a stack): one click to copy.
const code = computed(() => findCode(props.n.message || props.n.subtitle));
const messageText = computed(() => (hasChip.value ? (content.value?.text ?? "") : props.n.message));
// A link, number, address or parcel number in it (lib/smartActions): up to two chips, never the
// page the Open button already goes to (and never the code: CodeChip has that).
const smart = computed(() =>
  smartChips([props.n.subtitle, messageText.value].filter(Boolean).join("\n"), { limit: 2, skipUrls: [webLink.value?.url] }),
);
const expanded = ref(false);
const busy = ref<"positive" | "negative" | null>(null);
const isCall = computed(() => props.n.category === "incomingCall");
// A ringing call's buttons: Space never presses them; Enter only once it has rung a moment untouched.
const ringing = computed(() => isCall.value && props.n.live);
const { onCallKey } = useCallButtonGate(ringing);
function onActionKey(e: KeyboardEvent, positive: boolean) {
  if (ringing.value) onCallKey(e, positive ? "answer" : "decline");
}
const labels = computed(() => actionLabels(props.n));

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
      <p v-if="n.title" :class="['preview-private text-ink', compact ? 'text-[14px] font-medium' : 'mt-0.5 text-[15px] font-medium']">{{ n.title }}</p>
      <p v-if="n.subtitle" class="preview-private text-[14px] text-body-strong">{{ n.subtitle }}</p>
      <p
        v-if="messageText"
        :class="['preview-private selectable mt-0.5 cursor-text whitespace-pre-line text-[14px] text-body', expanded ? '' : 'line-clamp-4']"
        @click="expanded = !expanded"
      >
        {{ messageText }}
      </p>
      <MediaChip v-if="hasChip && content" class="mt-1 text-[14px] text-body" :media="content.media" :unreadable="content.unreadable" />


      <div :class="['flex flex-wrap items-center gap-2', compact ? 'mt-1.5' : 'mt-2.5']">
        <CodeChip v-if="code" :code="code.code" :from="[n]" />
        <SmartChip v-for="a in smart" :key="`${a.kind}:${a.start}`" :action="a" />
        <template v-if="n.live">
          <button
            v-if="n.flags.positiveAction"
            :class="[isCall ? 'btn-primary' : 'btn-secondary', 'btn-sm']"
            :disabled="busy !== null"
            :data-call-button="ringing ? '' : undefined"
            @keydown="onActionKey($event, true)"
            @keyup="onActionKey($event, true)"
            @click="act(true)"
          >
            {{ labels.positive }}
          </button>
          <button
            v-if="n.flags.negativeAction"
            class="btn-secondary btn-sm"
            :disabled="busy !== null"
            :data-call-button="ringing ? '' : undefined"
            @keydown="onActionKey($event, false)"
            @keyup="onActionKey($event, false)"
            @click="act(false)"
          >
            {{ labels.negative }}
          </button>
        </template>
        <button
          v-if="webLink"
          class="btn-secondary btn-sm"
          :title="`Open in ${webLink.label} on the web`"
          @click="tug.openUrl(webLink.url)"
        >
          <ExternalLink :size="13" />
          Open
        </button>
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
