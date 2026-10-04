<script setup lang="ts">
import { computed } from "vue";
import { avatarTone, initials } from "../lib/format";

const props = defineProps<{ appId: string; label: string; size?: "sm" | "md" }>();
const tone = computed(() => avatarTone(props.appId));
</script>

<template>
  <div
    :class="[
      tone,
      size === 'sm' ? 'size-8 text-[11px]' : 'size-10 text-[13px]',
      'flex shrink-0 items-center justify-center rounded-lg font-semibold tracking-wide',
    ]"
    :title="label"
  >
    {{ initials(label) }}
  </div>
</template>
