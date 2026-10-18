<script setup lang="ts">
// Settings › General › Voice typing: on/off, the hold-to-talk shortcut, the language, the speech
// engine (Accurate with its one-time model download, or Basic), and what to do if Windows is in
// the way. Words from lib/voice; state from stores/voice.
import { computed, onMounted } from "vue";
import { CircleAlert } from "lucide-vue-next";
import { useVoiceStore } from "../stores/voice";
import { hotkeyLabel } from "../lib/codeFill";
import {
  VOICE_COMMANDS,
  VOICE_HOTKEYS,
  downloadPercent,
  engineOptions,
  languageName,
  modelLine,
  modelOfferText,
  phoneMicNote,
  showModelOffer,
  voiceDescription,
  voiceHotkeyAlternatives,
} from "../lib/voice";
import type { VoiceEngine } from "../types/protocol";
import SettingsRow from "./SettingsRow.vue";
import SettingsSwitch from "./SettingsSwitch.vue";

const voice = useVoiceStore();

const enabled = computed({ get: () => voice.enabled, set: (v) => void voice.set({ enabled: v }) });
const keys = computed({ get: () => voice.keys, set: (v) => void voice.set({ keys: v }) });
const language = computed({
  get: () => voice.status?.language ?? "",
  set: (v) => void voice.set({ language: v || null }),
});
const languages = computed(() => voice.status?.languages ?? []);
const currentLanguage = computed(() => languageName(voice.status));

const engine = computed<VoiceEngine>({
  get: () => voice.status?.engine ?? "accurate",
  set: (v) => void voice.setEngine(v),
});
const options = computed(() => (voice.status ? engineOptions(voice.status) : []));
const line = computed(() => (voice.status ? modelLine(voice.status) : null));
const percent = computed(() => (voice.status ? downloadPercent(voice.status.model) : 0));
const offer = computed(() => (voice.status && showModelOffer(voice.status) ? modelOfferText(voice.status) : null));
const micNote = computed(() => phoneMicNote(voice.status));

function runLineAction() {
  const action = line.value?.action;
  if (action === "cancel") void voice.cancelDownload();
  else if (action === "download" || action === "resume") void voice.downloadModel();
}

// The installed languages, and a fresh look at the microphone, each time Settings opens.
onMounted(() => void voice.refresh());
</script>

<template>
  <div class="rounded-xl bg-surface-card">
    <SettingsRow label="Voice typing" :description="voiceDescription(voice.keys)">
      <SettingsSwitch v-model="enabled" label="Voice typing" />
    </SettingsRow>

    <!-- The first time it's on: offer the accurate engine's model, once. -->
    <div v-if="offer" class="border-t border-hairline-soft px-5 py-4" role="region" aria-label="Speech model">
      <p class="text-[13px] font-medium text-ink">{{ offer.title }}</p>
      <p class="mt-0.5 text-[13px] text-muted">{{ offer.body }}</p>
      <div class="mt-3 flex flex-wrap gap-2">
        <button class="btn-primary btn-sm" @click="voice.answerOffer(true)">{{ offer.button }}</button>
        <button class="btn-secondary btn-sm" @click="voice.answerOffer(false)">Not now</button>
      </div>
    </div>

    <div v-if="voice.enabled" class="flex flex-wrap items-center gap-x-6 gap-y-3 border-t border-hairline-soft px-5 py-3">
      <label class="flex items-center gap-2 text-[13px] text-body">
        Hold to talk
        <select v-model="keys" class="rounded-md border border-hairline bg-canvas px-2 py-1 font-mono text-[13px] text-ink">
          <option v-for="h in VOICE_HOTKEYS" :key="h" :value="h">{{ hotkeyLabel(h) }}</option>
        </select>
      </label>
      <label v-if="languages.length > 1" class="flex items-center gap-2 text-[13px] text-body">
        Language
        <select v-model="language" class="rounded-md border border-hairline bg-canvas px-2 py-1 text-[13px] text-ink">
          <option v-for="l in languages" :key="l.id" :value="l.id">{{ l.name }}</option>
        </select>
      </label>
      <p v-else-if="currentLanguage" class="text-[13px] text-body">Language: {{ currentLanguage }}</p>
    </div>

    <div v-if="voice.enabled" class="border-t border-hairline-soft px-5 py-3">
      <p id="voice-engine-label" class="text-[13px] font-medium text-ink">Speech engine</p>
      <div class="mt-2 flex flex-col gap-2" role="radiogroup" aria-labelledby="voice-engine-label">
        <label v-for="o in options" :key="o.id" class="flex items-start gap-2.5 text-[13px]">
          <input v-model="engine" type="radio" name="voice-engine" :value="o.id" class="mt-0.5 size-4 shrink-0 accent-ink" />
          <span>
            <span class="font-medium text-ink">{{ o.label }}</span>
            <span class="text-muted"> · {{ o.detail }}</span>
          </span>
        </label>
      </div>

      <div v-if="line && !offer" class="mt-3" role="status" aria-live="polite">
        <div class="flex items-center gap-3">
          <p class="min-w-0 flex-1 text-[13px]" :class="line.error ? 'text-error' : 'text-body'">{{ line.text }}</p>
          <button v-if="line.action" class="btn-secondary btn-sm shrink-0" @click="runLineAction">{{ line.actionLabel }}</button>
        </div>
        <div
          v-if="line.progress"
          class="mt-2 h-1.5 overflow-hidden rounded-full bg-surface-cream-strong"
          role="progressbar"
          aria-label="Speech model download"
          :aria-valuenow="percent"
          aria-valuemin="0"
          aria-valuemax="100"
        >
          <div class="h-full rounded-full bg-ink" :style="{ width: `${percent}%` }" />
        </div>
      </div>

      <p v-if="micNote" class="mt-3 text-[12px] text-muted">{{ micNote }}</p>
    </div>

    <div v-if="voice.problem" class="flex items-start gap-2.5 border-t border-hairline-soft px-5 py-3 text-[13px]" role="alert">
      <CircleAlert :size="16" class="mt-0.5 shrink-0 text-error" />
      <div class="min-w-0 flex-1">
        <p class="font-medium text-body-strong">{{ voice.problem.title }}</p>
        <p class="mt-0.5 text-muted">{{ voice.problem.body }}</p>
        <div v-if="voice.problem.kind === 'inUse'" class="mt-2 flex flex-wrap gap-1.5">
          <button
            v-for="h in voiceHotkeyAlternatives(voice.keys)"
            :key="h"
            class="btn-secondary btn-sm"
            :aria-label="`Use ${hotkeyLabel(h)}`"
            @click="keys = h"
          >
            <span class="font-mono text-[12px]">{{ hotkeyLabel(h) }}</span>
          </button>
        </div>
      </div>
      <button v-if="voice.problem.button" class="btn-secondary btn-sm shrink-0" @click="voice.openFix()">
        {{ voice.problem.button }}
      </button>
    </div>

    <div v-if="voice.enabled" class="border-t border-hairline-soft px-5 py-3">
      <p class="text-[13px] font-medium text-ink">What you can say</p>
      <div class="mt-2 grid grid-cols-1 gap-x-8 gap-y-1 lg:grid-cols-2">
        <p v-for="c in VOICE_COMMANDS" :key="c.say" class="flex items-baseline gap-2 text-[13px] text-body">
          <span class="text-muted">“{{ c.say }}”</span>
          <span class="ml-auto shrink-0 font-mono text-[12px] whitespace-nowrap text-ink">{{ c.types }}</span>
        </p>
      </div>
      <p class="mt-2 text-[12px] text-muted-soft">
        Also pascal, kebab and constant case. In code editors and terminals, tug leaves out sentence capitals. “New line” starts a
        new line only in Notepad, WordPad, Word, Outlook, OneNote, Teams, Slack and Discord; everywhere else, including code
        editors, terminals and browsers, it types a space, so nothing is sent or run by itself. Spoken commands work in English.
      </p>
    </div>
  </div>
</template>
