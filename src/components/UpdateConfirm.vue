<script setup lang="ts">
// "Restart tug to update?": the only way an update is applied while tug runs. Focus starts on
// Later and Esc means Later, so a stray key press never restarts tug. While a call rings, Tugboat
// is moving files or voice typing is busy, the backend holds the restart and this says what to
// finish first; the Update button stays either way.
import { computed, nextTick, onMounted, ref } from "vue";
import { ArrowUp, CircleAlert, LoaderCircle } from "lucide-vue-next";
import { useUpdateStore } from "../stores/update";
import { useFocusTrap } from "../lib/focusTrap";
import { blockerText } from "../lib/update";

const update = useUpdateStore();
const root = ref<HTMLElement | null>(null);
const laterButton = ref<HTMLButtonElement | null>(null);
useFocusTrap(root, () => update.later());
onMounted(async () => {
  await nextTick();
  laterButton.value?.focus();
});

const c = computed(() => (update.confirm.open ? update.confirm : null));
</script>

<template>
  <div v-if="c" class="fixed inset-0 z-50 flex items-center justify-center bg-ink/40 backdrop-blur-[2px]">
    <div
      ref="root"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="update-title"
      aria-describedby="update-notes"
      class="w-[420px] rounded-xl bg-surface-dark p-8 text-on-dark"
    >
      <p class="caption-upper flex items-center gap-2 text-on-dark-soft">
        <!-- caption-upper capitalises, and "tug" is always lowercase: so no app name here. -->
        <ArrowUp :size="14" /> Version {{ c.version }}
      </p>
      <p id="update-title" class="mt-2 font-display text-[26px] leading-tight" style="letter-spacing: -0.02em">
        Restart tug to update?
      </p>
      <p id="update-notes" class="mt-3 text-[14px] leading-snug text-on-dark-soft">
        <template v-if="c.headline">{{ c.headline }}</template>
        <template v-else>tug closes for a moment and comes back on this screen.</template>
      </p>
      <button class="mt-2 text-[13px] text-accent-amber underline-offset-2 hover:underline" @click="update.openReleasePage()">
        Release notes
      </button>

      <p v-if="c.held" role="status" class="mt-4 flex items-center gap-2 rounded-lg bg-surface-dark-elevated px-3 py-2 text-[13px] text-on-dark">
        <CircleAlert :size="14" class="shrink-0 text-accent-amber" />
        {{ blockerText(c.held) }}. The update will wait.
      </p>
      <p v-else-if="c.error" role="status" class="mt-4 flex items-center gap-2 rounded-lg bg-error/15 px-3 py-2 text-[13px] text-on-dark">
        <CircleAlert :size="14" class="shrink-0 text-error" />
        {{ c.error }}
      </p>

      <div class="mt-6 flex justify-end gap-2">
        <button ref="laterButton" class="btn-on-dark" @click="update.later()">Later</button>
        <button class="btn-primary disabled:opacity-50" :disabled="c.applying" @click="update.restart()">
          <LoaderCircle v-if="c.applying" :size="14" class="animate-spin" />
          {{ c.applying ? "Restarting…" : "Restart now" }}
        </button>
      </div>
    </div>
  </div>
</template>
