<script setup lang="ts">
// The settings page's on/off switch (the left rail has a dark twin in ToggleRow).
defineProps<{ label: string; disabled?: boolean }>();
const model = defineModel<boolean>({ required: true });
</script>

<template>
  <button
    type="button"
    role="switch"
    :aria-checked="model"
    :aria-label="label"
    :disabled="disabled"
    :class="[
      'relative h-6 w-11 rounded-full transition-colors disabled:opacity-40',
      model ? 'bg-accent-teal' : 'bg-surface-cream-strong',
    ]"
    @click="model = !model"
  >
    <span
      :class="[
        'absolute top-0.5 left-0 size-5 rounded-full bg-canvas shadow-sm transition-transform',
        model ? 'translate-x-[22px]' : 'translate-x-0.5',
      ]"
    />
  </button>
</template>
