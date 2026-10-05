<script setup lang="ts">
// A copy of the iPhone's Settings › Bluetooth › ⓘ screen for this PC; its switches follow the real ones.
defineProps<{ switches: Array<{ label: string; why: string; on: boolean; required: boolean }> }>();
</script>

<template>
  <div class="mx-auto mt-8 w-full max-w-[380px] overflow-hidden rounded-2xl border border-hairline bg-canvas shadow-sm">
    <p class="border-b border-hairline-soft px-5 py-3 text-center text-[13px] font-semibold text-ink">This PC</p>
    <ul class="divide-y divide-hairline-soft">
      <li v-for="x in switches" :key="x.label" class="flex items-center gap-3 px-5 py-3.5">
        <span class="min-w-0 flex-1">
          <span class="block text-[14px] text-ink">{{ x.label }}</span>
          <span class="block text-[12px] text-muted">{{ x.why }}{{ x.required ? "" : " · optional" }}</span>
        </span>
        <span :class="['relative h-[26px] w-[44px] shrink-0 rounded-full transition-colors duration-500', x.on ? 'bg-success' : 'bg-surface-cream-strong']">
          <span
            :class="[
              'absolute top-[2px] left-0 size-[22px] rounded-full bg-canvas shadow-sm transition-transform duration-500',
              x.on ? 'translate-x-[20px]' : 'translate-x-[2px]',
            ]"
          />
        </span>
      </li>
    </ul>
  </div>
</template>
