<script setup lang="ts">
import { useTugStore } from "../stores/tug";

const tug = useTugStore();
</script>

<template>
  <Transition enter-from-class="opacity-0" leave-to-class="opacity-0" enter-active-class="transition-opacity" leave-active-class="transition-opacity">
    <div v-if="tug.pairingRequest" class="fixed inset-0 z-50 flex items-center justify-center bg-ink/40 backdrop-blur-[2px]">
      <div role="dialog" aria-modal="true" class="w-[400px] rounded-xl bg-surface-dark p-8 text-on-dark">
        <p class="caption-upper text-on-dark-soft">Pairing request</p>
        <p class="mt-2 font-display text-[30px] leading-tight" style="letter-spacing: -0.02em">
          {{ tug.pairingRequest.pin ? "Do the codes match?" : "Pair with this device?" }}
        </p>
        <p class="mt-1 text-[14px] text-on-dark-soft">{{ tug.pairingRequest.deviceName }}</p>

        <p v-if="tug.pairingRequest.pin" class="my-6 text-center font-mono text-[44px] tracking-[0.2em] text-on-dark">
          {{ tug.pairingRequest.pin }}
        </p>
        <p class="text-[13px] text-on-dark-soft">
          {{
            tug.pairingRequest.pin
              ? "Check it matches the code on your iPhone, then tap Pair here and on the phone."
              : "Tap Pair here, then confirm on your iPhone."
          }}
        </p>

        <div class="mt-6 flex justify-end gap-2">
          <button class="btn-on-dark" @click="tug.confirmPairing(false)">Cancel</button>
          <button class="btn-primary" @click="tug.confirmPairing(true)">Pair</button>
        </div>
      </div>
    </div>
  </Transition>
</template>
