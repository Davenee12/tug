<script setup lang="ts">
defineProps<{ label: string; description?: string; disabled?: boolean }>();
const model = defineModel<boolean>({ required: true });
</script>

<template>
  <button
    type="button"
    role="switch"
    :aria-checked="model"
    :disabled="disabled"
    class="flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left transition-colors active:bg-surface-dark-elevated disabled:opacity-50"
    @click="model = !model"
  >
    <span class="min-w-0 flex-1">
      <span class="block text-[14px] font-medium text-on-dark">{{ label }}</span>
      <span v-if="description" class="block text-[12px] leading-snug text-on-dark-soft">{{ description }}</span>
    </span>
    <span
      :class="[model ? 'bg-accent-teal' : 'bg-surface-dark-elevated', 'relative h-5 w-9 shrink-0 rounded-full transition-colors']"
    >
      <span
        :class="[model ? 'translate-x-4' : 'translate-x-0.5', 'absolute top-0.5 size-4 rounded-full bg-on-dark transition-transform']"
      />
    </span>
  </button>
</template>
