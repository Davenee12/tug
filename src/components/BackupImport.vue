<script setup lang="ts">
// Settings › Messages › Bring in your full history: pick a backup → its password if it's
// encrypted → choose texts and photos → import with progress and Cancel → what came in. The
// steps follow the backend's status (stores/backup); the words come from lib/backupImport.
import { computed, nextTick, ref, watch } from "vue";
import { CircleAlert, FolderOpen, HardDrive, Lock, RefreshCw, X } from "lucide-vue-next";
import { useBackupStore } from "../stores/backup";
import { useFocusTrap } from "../lib/focusTrap";
import {
  backupDetails,
  backupUnusable,
  HOW_TO_BACK_UP,
  photosChoice,
  problemMessage,
  progressLine,
  scanLine,
  summaryLine,
  summaryNote,
  wizardStep,
} from "../lib/backupImport";
import type { BackupInfo } from "../types/protocol";

const backup = useBackupStore();
const root = ref<HTMLElement | null>(null);
const step = computed(() => wizardStep(backup.status, !!backup.chosen?.encrypted));
const working = computed(() => step.value === "working");

// Esc closes, except while it works (Cancel is the way out then, so a stray Esc can't stop an import).
useFocusTrap(root, () => {
  if (!working.value) void backup.closeWizard();
});

const password = ref("");
const passwordBox = ref<HTMLInputElement | null>(null);
watch(
  step,
  async (s) => {
    if (s !== "password") password.value = "";
    else {
      await nextTick();
      passwordBox.value?.focus();
    }
  },
  { immediate: true },
);
function unlock() {
  if (!password.value) return;
  const pw = password.value;
  // Not kept here either: the box empties as soon as it's sent.
  password.value = "";
  void backup.open(pw);
}

const texts = ref(true);
const photos = ref(true);
watch(
  () => backup.status.scan,
  (scan) => {
    if (!scan) return;
    texts.value = scan.texts > 0;
    photos.value = scan.photos + scan.videos > 0;
  },
  { immediate: true },
);
const nothingNew = computed(() => {
  const s = backup.status.scan;
  return !!s && s.texts === 0 && s.photos + s.videos === 0;
});
const canStart = computed(() => (texts.value && (backup.status.scan?.texts ?? 0) > 0) || (photos.value && !!backup.status.scan && backup.status.scan.photos + backup.status.scan.videos > 0));

const percent = computed(() => Math.round((backup.status.progress ?? 0) * 100));
const backupName = computed(() => backup.status.backup?.deviceName ?? backup.chosen?.deviceName ?? "your iPhone");
const choosable = (b: BackupInfo) => !backupUnusable(b);

function back() {
  backup.chosen = null;
  void backup.cancel();
}
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-start justify-center bg-ink/30 px-6 pt-[10vh] backdrop-blur-[2px]">
    <div
      ref="root"
      role="dialog"
      aria-modal="true"
      aria-labelledby="backup-title"
      class="flex max-h-[78vh] w-full max-w-[520px] flex-col overflow-hidden rounded-2xl border border-hairline bg-canvas shadow-xl"
    >
      <header class="flex items-start gap-3 px-6 pt-6 pb-3">
        <div class="min-w-0 flex-1">
          <h2 id="backup-title" class="headline text-[24px] leading-tight text-ink">Bring in your full history</h2>
          <p class="mt-1 text-[13px] text-muted">
            Every text, sent ones included, and your photos, from a backup of your iPhone on this PC. It all stays on this PC.
          </p>
        </div>
        <button
          v-if="!working"
          class="rounded-md p-1.5 text-muted active:bg-surface-card"
          aria-label="Close"
          @click="backup.closeWizard()"
        >
          <X :size="16" />
        </button>
      </header>

      <div class="min-h-0 flex-1 overflow-y-auto px-6 pb-6">
        <!-- 1. Pick a backup -->
        <template v-if="step === 'pick'">
          <p v-if="backup.listing" class="py-6 text-[13px] text-muted">Looking for backups on this PC…</p>
          <template v-else-if="backup.backups && backup.backups.length">
            <p class="caption-upper mt-2 mb-2 text-muted">Backups on this PC</p>
            <ul class="space-y-2">
              <li v-for="b in backup.backups" :key="b.id">
                <button
                  class="flex w-full items-start gap-3 rounded-xl border border-hairline px-4 py-3 text-left enabled:active:bg-surface-soft disabled:opacity-60"
                  :disabled="!choosable(b)"
                  @click="backup.choose(b)"
                >
                  <HardDrive :size="18" class="mt-0.5 shrink-0 text-muted" />
                  <span class="min-w-0 flex-1">
                    <span class="flex items-center gap-1.5 text-[14px] font-medium text-ink">
                      <span class="truncate">{{ b.deviceName }}</span>
                      <Lock v-if="b.encrypted" :size="12" class="shrink-0 text-muted" aria-label="Encrypted" />
                    </span>
                    <span class="block text-[12px] text-muted">{{ backupDetails(b) }}</span>
                    <span v-if="backupUnusable(b)" class="mt-1 block text-[12px] text-error">{{ backupUnusable(b) }}</span>
                  </span>
                </button>
              </li>
            </ul>
          </template>
          <template v-else>
            <p class="mt-3 text-[14px] font-medium text-ink">No iPhone backup on this PC yet</p>
            <ol class="mt-2 list-decimal space-y-1.5 pl-5 text-[13px] text-body">
              <li v-for="line in HOW_TO_BACK_UP" :key="line">{{ line }}</li>
            </ol>
          </template>
          <p v-if="backup.listError" class="mt-3 flex items-start gap-2 text-[13px] text-error">
            <CircleAlert :size="14" class="mt-0.5 shrink-0" /> {{ backup.listError }}
          </p>
          <div class="mt-5 flex items-center gap-2">
            <button class="btn-secondary btn-sm" @click="backup.pickFolder()"><FolderOpen :size="13" /> Choose a folder…</button>
            <button class="btn-secondary btn-sm" :disabled="backup.listing" @click="backup.refreshList()">
              <RefreshCw :size="13" /> Look again
            </button>
          </div>
        </template>

        <!-- 2. The password of an encrypted backup -->
        <form v-else-if="step === 'password'" class="mt-2" @submit.prevent="unlock">
          <p class="text-[14px] text-ink">
            <Lock :size="13" class="mr-1 inline align-[-1px]" />{{ backupName }}'s backup is encrypted.
          </p>
          <label class="mt-3 block text-[13px] text-muted" for="backup-password">Backup password</label>
          <input
            id="backup-password"
            ref="passwordBox"
            v-model="password"
            type="password"
            autocomplete="off"
            spellcheck="false"
            class="input mt-1 w-full"
          />
          <p v-if="backup.status.problem === 'wrongPassword'" class="mt-2 flex items-start gap-2 text-[13px] text-error" role="alert">
            <CircleAlert :size="14" class="mt-0.5 shrink-0" /> {{ problemMessage("wrongPassword") }}
          </p>
          <p class="mt-2 text-[12px] text-muted-soft">
            The one you chose when you turned on “Encrypt local backup”. tug uses it once, to open the backup, and never saves it.
          </p>
          <div class="mt-5 flex items-center gap-2">
            <button type="submit" class="btn-primary btn-sm" :disabled="!password">Unlock</button>
            <button type="button" class="btn-secondary btn-sm" @click="back">Back</button>
          </div>
        </form>

        <!-- 3. Working: unlocking, reading, importing -->
        <template v-else-if="step === 'working'">
          <p class="mt-3 text-[14px] text-ink" role="status" aria-live="polite">{{ progressLine(backup.status) }}</p>
          <div
            class="mt-3 h-1.5 overflow-hidden rounded-full bg-surface-card"
            role="progressbar"
            :aria-valuenow="backup.status.progress === null ? undefined : percent"
            aria-valuemin="0"
            aria-valuemax="100"
          >
            <div
              v-if="backup.status.progress !== null"
              class="h-full rounded-full bg-ink"
              :style="{ width: `${percent}%` }"
            />
            <div v-else class="h-full w-1/3 rounded-full bg-muted-soft" />
          </div>
          <p v-if="backup.status.phase === 'unlocking'" class="mt-3 text-[12px] text-muted-soft">
            This takes a minute on purpose: it's what keeps an encrypted backup safe.
          </p>
          <p v-else-if="backup.status.phase === 'importing'" class="mt-3 text-[12px] text-muted-soft">
            You can keep using tug. If you cancel, what's in already stays, and running it again carries on.
          </p>
          <div class="mt-5">
            <button class="btn-secondary btn-sm" @click="backup.cancel()">Cancel</button>
          </div>
        </template>

        <!-- 4. What to bring in -->
        <template v-else-if="step === 'choose' && backup.status.scan">
          <p class="mt-2 text-[14px] text-ink">{{ scanLine(backup.status.scan) }}</p>
          <p v-if="backup.status.scan.already" class="mt-1 text-[12px] text-muted">
            {{ backup.status.scan.already.toLocaleString("en-US") }} already in tug won't be added twice.
          </p>
          <template v-if="!nothingNew">
            <fieldset class="mt-4 space-y-2.5">
              <legend class="caption-upper mb-2 text-muted">Bring in</legend>
              <label class="flex items-center gap-2.5 text-[14px] text-ink">
                <input v-model="texts" type="checkbox" class="size-4 accent-ink" :disabled="!backup.status.scan.texts" />
                Texts{{ backup.status.scan.texts ? ` · ${backup.status.scan.texts.toLocaleString("en-US")}` : " (none new)" }}
              </label>
              <label class="flex items-center gap-2.5 text-[14px] text-ink">
                <input
                  v-model="photos"
                  type="checkbox"
                  class="size-4 accent-ink"
                  :disabled="!(backup.status.scan.photos + backup.status.scan.videos)"
                />
                {{ photosChoice(backup.status.scan) }}
              </label>
            </fieldset>
            <p class="mt-3 text-[12px] text-muted-soft">
              Photos and videos are copied into tug's own folder on this PC, not your Pictures.
              <template v-if="backup.status.backup?.encrypted">
                Once in tug, they're kept like the rest of your history on this PC, no longer behind the backup's password.
              </template>
            </p>
          </template>
          <div class="mt-5 flex items-center gap-2">
            <button v-if="!nothingNew" class="btn-primary btn-sm" :disabled="!canStart" @click="backup.start(texts, photos)">
              Bring it in
            </button>
            <button class="btn-secondary btn-sm" @click="nothingNew ? backup.closeWizard() : back()">
              {{ nothingNew ? "Done" : "Back" }}
            </button>
          </div>
        </template>

        <!-- 5. Done -->
        <template v-else-if="step === 'done' && backup.status.summary">
          <p class="mt-2 text-[15px] font-medium text-ink" role="status">{{ summaryLine(backup.status.summary) }}</p>
          <p v-if="summaryNote(backup.status.summary)" class="mt-1 text-[13px] text-muted">{{ summaryNote(backup.status.summary) }}</p>
          <p class="mt-2 text-[13px] text-muted">They're in Messages with the rest of your conversations.</p>
          <div class="mt-5"><button class="btn-primary btn-sm" @click="backup.closeWizard()">Done</button></div>
        </template>

        <!-- Stopped -->
        <template v-else-if="step === 'stopped'">
          <p class="mt-2 text-[14px] text-ink">Stopped. What was brought in stays; run it again any time to carry on.</p>
          <div class="mt-5"><button class="btn-secondary btn-sm" @click="backup.closeWizard()">Close</button></div>
        </template>

        <!-- Something went wrong -->
        <template v-else-if="step === 'failed'">
          <p class="mt-2 flex items-start gap-2 text-[14px] text-ink" role="alert">
            <CircleAlert :size="16" class="mt-0.5 shrink-0 text-error" /> {{ problemMessage(backup.status.problem) }}
          </p>
          <div class="mt-5 flex items-center gap-2">
            <button class="btn-secondary btn-sm" @click="back">Try again</button>
            <button class="btn-secondary btn-sm" @click="backup.closeWizard()">Close</button>
          </div>
        </template>
      </div>
    </div>
  </div>
</template>
