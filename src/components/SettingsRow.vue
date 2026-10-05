<script setup lang="ts">
// One line of a settings card: what it is on the left, its control on the right.
defineProps<{ label: string; description?: string }>();
</script>

<template>
  <div class="flex items-center gap-6 px-5 py-4">
    <div class="min-w-0 flex-1">
      <p class="text-[14px] font-medium text-ink">{{ label }}</p>
      <p v-if="description" class="mt-0.5 text-[13px] leading-snug text-muted">{{ description }}</p>
      <slot name="below" />
    </div>
    <div class="shrink-0"><slot /></div>
  </div>
</template>
