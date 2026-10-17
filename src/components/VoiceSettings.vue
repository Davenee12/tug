<script setup lang="ts">
// Settings › General › Voice typing: on/off, the hold-to-talk shortcut, the language, and what
// to do if Windows is in the way. Words from lib/voice; state from stores/voice.
import { computed, onMounted } from "vue";
import { CircleAlert } from "lucide-vue-next";
import { useVoiceStore } from "../stores/voice";
import { hotkeyLabel } from "../lib/codeFill";
import { VOICE_COMMANDS, VOICE_HOTKEYS, languageName, voiceDescription, voiceHotkeyAlternatives } from "../lib/voice";
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

// The installed languages, and a fresh look at the microphone switch, each time Settings opens.
onMounted(() => void voice.refresh());
</script>

<template>
  <div class="rounded-xl bg-surface-card">
    <SettingsRow label="Voice typing" :description="voiceDescription(voice.keys)">
      <SettingsSwitch v-model="enabled" label="Voice typing" />
    </SettingsRow>

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
        Also pascal, kebab and constant case. In code editors and terminals, tug leaves out sentence capitals, and in a terminal
        “new line” is skipped so nothing runs by itself. Spoken commands work in English.
      </p>
    </div>
  </div>
</template>
