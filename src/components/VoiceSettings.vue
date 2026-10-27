<script setup lang="ts">
// Settings › General › Voice typing: on/off, the hold-to-talk shortcut, the language, the speech
// engine (Basic, Accurate or Most accurate, the last two with a one-time download that starts by
// itself), the person's own words (contacts' names and a personal dictionary), the space the
// models take, and what to do if Windows is in the way. Words from lib/voice; state from
// stores/voice.
import { computed, onMounted, ref } from "vue";
import { CircleAlert, Pencil, X } from "lucide-vue-next";
import { useVoiceStore } from "../stores/voice";
import { hotkeyLabel } from "../lib/codeFill";
import {
  SPOKEN_LANGUAGES,
  VOICE_COMMANDS,
  VOICE_HOTKEYS,
  dictionaryEntry,
  engineOptions,
  languageName,
  modelLines,
  modelsDiskUsed,
  phoneMicNote,
  sizeLabel,
  spokenLanguageNote,
  voiceDescription,
  voiceHotkeyAlternatives,
  type ModelLine,
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
const spoken = computed({
  get: () => voice.status?.spokenLanguage ?? "auto",
  set: (v) => void voice.setSpokenLanguage(v),
});
const contactHints = computed({
  get: () => voice.status?.contactHints ?? true,
  set: (v) => void voice.setContactHints(v),
});
const options = computed(() => (voice.status ? engineOptions(voice.status) : []));
const lines = computed(() => (voice.status ? modelLines(voice.status) : []));
const diskUsed = computed(() => (voice.status ? modelsDiskUsed(voice.status) : 0));
const micNote = computed(() => phoneMicNote(voice.status));

function runLineAction(l: ModelLine) {
  if (l.action === "cancel") void voice.cancelDownload(l.model);
  else if (l.action === "download" || l.action === "resume") void voice.downloadModel(l.model);
}

// Deleting the models asks once more, in place.
const confirmDelete = ref(false);
async function deleteModels() {
  confirmDelete.value = false;
  await voice.deleteModels();
}

// The personal dictionary: add, edit (the word goes back into the box) and remove.
const draft = ref("");
const editing = ref<string | null>(null);
const wordProblem = ref<string | null>(null);
async function saveWord() {
  const words = voice.dictionary;
  const checked = dictionaryEntry(draft.value, words, editing.value);
  if ("problem" in checked) {
    wordProblem.value = checked.problem;
    return;
  }
  const next = editing.value ? words.map((w) => (w === editing.value ? checked.word : w)) : [...words, checked.word];
  const saved = await voice.saveDictionary(next);
  // The backend tidies entries (emoji and other symbols go); say so if it couldn't use this one.
  if (!saved.some((w) => w.toLowerCase() === checked.word.toLowerCase().replace(/[^\p{L}\p{N}\s'.+#&-]/gu, "").trim())) {
    wordProblem.value = "tug couldn't use that one. Try letters and numbers.";
    return;
  }
  draft.value = "";
  editing.value = null;
  wordProblem.value = null;
}
function editWord(word: string) {
  editing.value = word;
  draft.value = word;
  wordProblem.value = null;
}
function cancelEdit() {
  editing.value = null;
  draft.value = "";
  wordProblem.value = null;
}
async function removeWord(word: string) {
  if (editing.value === word) cancelEdit();
  await voice.saveDictionary(voice.dictionary.filter((w) => w !== word));
}

// The installed languages, a fresh look at the microphone and the dictionary, each time Settings opens.
onMounted(() => {
  void voice.refresh();
  void voice.loadDictionary();
});
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

      <label v-if="engine === 'mostAccurate'" class="mt-3 flex items-center gap-2 text-[13px] text-body">
        Most accurate listens in
        <select v-model="spoken" class="rounded-md border border-hairline bg-canvas px-2 py-1 text-[13px] text-ink">
          <option v-for="l in SPOKEN_LANGUAGES" :key="l.code" :value="l.code">{{ l.name }}</option>
        </select>
      </label>
      <p v-if="engine === 'mostAccurate'" class="mt-1 text-[12px] text-muted">{{ spokenLanguageNote(spoken) }}</p>

      <div v-for="l in lines" :key="`${l.model}-${l.action}`" class="mt-3" role="status" aria-live="polite">
        <div class="flex items-center gap-3">
          <p class="min-w-0 flex-1 text-[13px]" :class="l.error ? 'text-error' : 'text-body'">{{ l.text }}</p>
          <button v-if="l.action" class="btn-secondary btn-sm shrink-0" @click="runLineAction(l)">{{ l.actionLabel }}</button>
        </div>
        <div
          v-if="l.progress"
          class="mt-2 h-1.5 overflow-hidden rounded-full bg-surface-cream-strong"
          role="progressbar"
          aria-label="Speech model download"
          :aria-valuenow="l.percent"
          aria-valuemin="0"
          aria-valuemax="100"
        >
          <div class="h-full rounded-full bg-ink" :style="{ width: `${l.percent}%` }" />
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

    <!-- The person's own words: Accurate and Most accurate spell these their way. All on this PC. -->
    <div v-if="voice.enabled" class="border-t border-hairline-soft">
      <SettingsRow
        label="Use your contacts' names"
        description="Accurate and Most accurate spell the names of the people you text most the way your contacts do. The names stay on this PC."
      >
        <SettingsSwitch v-model="contactHints" label="Use your contacts' names" />
      </SettingsRow>
      <div class="px-5 pb-4">
        <p id="voice-dictionary-label" class="text-[14px] font-medium text-ink">Personal dictionary</p>
        <p class="mt-0.5 text-[13px] leading-snug text-muted">
          Words and names you want spelled your way, like a product, a place or a nickname. Used by Accurate and Most accurate,
          and kept on this PC.
        </p>
        <form class="mt-2 flex flex-wrap items-center gap-2" aria-labelledby="voice-dictionary-label" @submit.prevent="saveWord">
          <input
            v-model="draft"
            class="input min-w-0 flex-1 py-1 text-[13px]"
            :placeholder="editing ? 'Change the word' : 'Add a word or a name'"
            :aria-label="editing ? `Change ${editing}` : 'Add a word or a name'"
            maxlength="60"
            @keydown.esc.prevent="cancelEdit"
          />
          <button type="submit" class="btn-secondary btn-sm">{{ editing ? "Save" : "Add" }}</button>
          <button v-if="editing" type="button" class="btn-secondary btn-sm" @click="cancelEdit">Cancel</button>
        </form>
        <p v-if="wordProblem" class="mt-1.5 text-[12px] text-error" role="alert">{{ wordProblem }}</p>
        <ul v-if="voice.dictionary.length" class="mt-2.5 flex flex-wrap gap-1.5" aria-label="Your words">
          <li
            v-for="w in voice.dictionary"
            :key="w"
            class="flex items-center gap-1 rounded-full border border-hairline bg-canvas py-0.5 pr-1 pl-2.5 text-[13px] text-ink"
            :class="{ 'border-ink': editing === w }"
          >
            <span>{{ w }}</span>
            <button type="button" class="rounded-full p-0.5 text-muted hover:text-ink" :aria-label="`Change ${w}`" @click="editWord(w)">
              <Pencil :size="12" />
            </button>
            <button type="button" class="rounded-full p-0.5 text-muted hover:text-ink" :aria-label="`Remove ${w}`" @click="removeWord(w)">
              <X :size="12" />
            </button>
          </li>
        </ul>
      </div>
    </div>

    <!-- The space the models take, whether voice typing is on or not, with a way to get it back. -->
    <div v-if="diskUsed > 0" class="flex flex-wrap items-center gap-3 border-t border-hairline-soft px-5 py-3">
      <p class="min-w-0 flex-1 text-[13px] text-body">
        <template v-if="confirmDelete">
          Delete {{ sizeLabel(diskUsed) }} of speech models? tug uses Basic until you download one again.
        </template>
        <template v-else>Speech models use {{ sizeLabel(diskUsed) }} on this PC.</template>
      </p>
      <template v-if="confirmDelete">
        <button class="btn-secondary btn-sm shrink-0" @click="deleteModels">Delete</button>
        <button class="btn-secondary btn-sm shrink-0" @click="confirmDelete = false">Keep them</button>
      </template>
      <button v-else class="btn-secondary btn-sm shrink-0" @click="confirmDelete = true">Delete downloaded models</button>
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
