<script setup lang="ts">
// The Update button, like Teams': it only appears once a new version has downloaded and been
// checked (beside tug's name, "Update"), or on the portable exe once one is out (its own line under
// the name, "Update available — Download", which is too long to sit beside it). Pressing Update
// asks before anything restarts (UpdateConfirm); tug never restarts by itself.
import { computed } from "vue";
import { ArrowUp } from "lucide-vue-next";
import { useUpdateStore } from "../stores/update";

const props = defineProps<{ action: "confirm" | "download" }>();
const update = useUpdateStore();
const pill = computed(() => (update.pill?.action === props.action ? update.pill : null));
</script>

<template>
  <button
    v-if="pill"
    class="inline-flex shrink-0 items-center gap-1 rounded-full bg-primary px-2 py-0.5 text-[12px] font-medium text-on-primary transition-colors active:bg-primary-active"
    :title="pill.title"
    :aria-label="pill.title"
    @click="update.pressPill()"
  >
    <ArrowUp :size="13" aria-hidden="true" />
    {{ pill.label }}
  </button>
</template>
