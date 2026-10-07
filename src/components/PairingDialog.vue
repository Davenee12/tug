<script setup lang="ts">
import { nextTick, onMounted, ref } from "vue";
import { useTugStore } from "../stores/tug";
import { useFocusTrap } from "../lib/focusTrap";

const tug = useTugStore();
const root = ref<HTMLElement | null>(null);
const cancel = ref<HTMLButtonElement | null>(null);
// Esc cancels; focus starts on Cancel so a stray Enter can't approve a pairing.
useFocusTrap(root, () => void tug.confirmPairing(false));
onMounted(async () => {
  await nextTick();
  cancel.value?.focus();
});
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-ink/40 backdrop-blur-[2px]">
    <div
      ref="root"
      role="dialog"
      aria-modal="true"
      aria-labelledby="pairing-title"
      class="w-[400px] rounded-xl bg-surface-dark p-8 text-on-dark"
    >
      <p class="caption-upper text-on-dark-soft">Pairing request</p>
      <!-- ConfirmOnly: Windows has accepted; the user confirms on the iPhone. Nothing to do here. -->
      <template v-if="tug.pairingRequest?.confirmOnPhone">
        <p id="pairing-title" class="mt-2 font-display text-[30px] leading-tight" style="letter-spacing: -0.02em">
          Tap Pair on your iPhone
        </p>
        <p class="mt-1 text-[14px] text-on-dark-soft">{{ tug.pairingRequest?.deviceName }}</p>
        <p class="mt-4 text-[13px] text-on-dark-soft">Confirm the pairing on your iPhone — it continues on its own.</p>
        <div class="mt-6 flex justify-end">
          <button ref="cancel" class="btn-on-dark" @click="tug.confirmPairing(false)">Close</button>
        </div>
      </template>
      <template v-else>
        <p id="pairing-title" class="mt-2 font-display text-[30px] leading-tight" style="letter-spacing: -0.02em">
          {{ tug.pairingRequest?.pin ? "Do the codes match?" : "Pair with this device?" }}
        </p>
        <p class="mt-1 text-[14px] text-on-dark-soft">{{ tug.pairingRequest?.deviceName }}</p>

        <p v-if="tug.pairingRequest?.pin" class="my-6 text-center font-mono text-[44px] tracking-[0.2em] text-on-dark">
          {{ tug.pairingRequest.pin }}
        </p>
        <p class="text-[13px] text-on-dark-soft">
          {{
            tug.pairingRequest?.pin
              ? "Check it matches the code on your iPhone, then click Pair here and tap it on the phone."
              : "Click Pair here, then confirm on your iPhone."
          }}
        </p>

        <div class="mt-6 flex justify-end gap-2">
          <button ref="cancel" class="btn-on-dark" @click="tug.confirmPairing(false)">Cancel</button>
          <button class="btn-primary" @click="tug.confirmPairing(true)">Pair</button>
        </div>
      </template>
    </div>
  </div>
</template>
