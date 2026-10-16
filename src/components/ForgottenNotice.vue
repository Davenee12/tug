<script setup lang="ts">
import { LoaderCircle } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { FORGOT_ACTION, FORGOT_BODY, FORGOT_TITLE } from "../lib/connectionStatus";

// "Your iPhone forgot this PC": the phone dropped its pairing while Windows kept this side's, so
// nothing reconnects until it's paired again. Pair again removes the dead pairing and brings up the
// pairing steps. One component (and lib/connectionStatus's words), so every panel says it the same way.
const tug = useTugStore();
</script>

<template>
  <div role="status" class="flex flex-col gap-2 rounded-xl border border-error/30 bg-canvas px-4 py-3">
    <p class="text-[13px] font-medium text-body-strong">{{ FORGOT_TITLE }}</p>
    <p class="text-[13px] text-body">{{ FORGOT_BODY }}</p>
    <button class="btn-primary btn-sm self-start" :disabled="tug.pairingAgain" @click="tug.pairAgain()">
      <LoaderCircle v-if="tug.pairingAgain" :size="13" class="animate-spin" />
      {{ FORGOT_ACTION }}
    </button>
  </div>
</template>
