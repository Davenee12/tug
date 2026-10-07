<script setup lang="ts">
// A copy of the iPhone's Settings › Bluetooth › ⓘ screen for this PC; its switches follow the
// real ones (lib/phoneSwitches). Each is on, off, checking (briefly, pulsing) or waiting (can't be
// read right now, with the reason) — tug never shows a switch as off before the phone has
// answered, and never "Checking…" forever. When one is off, it names what to do.
import type { PhoneSwitch } from "../lib/phoneSwitches";
defineProps<{ switches: PhoneSwitch[] }>();
defineEmits<{ recheck: [] }>();
</script>

<template>
  <div class="mx-auto mt-8 w-full max-w-[380px] overflow-hidden rounded-2xl border border-hairline bg-canvas shadow-sm">
    <p class="border-b border-hairline-soft px-5 py-3 text-center text-[13px] font-semibold text-ink">This PC</p>
    <ul class="divide-y divide-hairline-soft">
      <li v-for="x in switches" :key="x.label" class="px-5 py-3.5">
        <div class="flex items-center gap-3">
          <span class="min-w-0 flex-1">
            <span class="block text-[14px] text-ink">{{ x.label }}</span>
            <span class="block text-[12px] text-muted">{{ x.why }}{{ x.required ? "" : " · optional" }}</span>
          </span>
          <!-- checking: a neutral, pulsing toggle; waiting: neutral, still; off: gray; on: green -->
          <span
            :class="[
              'relative h-[26px] w-[44px] shrink-0 rounded-full transition-colors duration-500',
              x.state === 'on' ? 'bg-success' : x.state === 'off' ? 'bg-surface-cream-strong' : 'bg-surface-cream-strong opacity-60',
            ]"
          >
            <span
              :class="[
                'absolute top-[2px] left-0 size-[22px] rounded-full bg-canvas shadow-sm transition-transform duration-500',
                x.state === 'on' ? 'translate-x-[20px]' : 'translate-x-[2px]',
                x.state === 'checking' ? 'animate-pulse motion-reduce:animate-none' : '',
              ]"
            />
          </span>
        </div>
        <!-- Off: exactly what to do. Checking/waiting: say so, and why. -->
        <p v-if="x.note" :class="['mt-1.5 text-[12px]', x.state === 'off' ? 'text-body' : 'text-muted-soft']">
          {{ x.note }}
          <button
            v-if="x.recheck"
            class="ml-1 font-medium text-muted underline-offset-2 hover:underline"
            @click="$emit('recheck')"
          >
            Check again
          </button>
        </p>
      </li>
    </ul>
  </div>
</template>
