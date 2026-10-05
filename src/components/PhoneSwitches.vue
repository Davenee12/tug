<script setup lang="ts">
// A copy of the iPhone's Settings › Bluetooth › ⓘ screen for this PC; its switches follow the
// real ones. Each is on, off, or unknown (no signal yet) — tug never shows a switch as off
// before the phone has answered. When one is off, it names what to do and where.
import type { PhoneSwitch } from "../lib/phoneSwitches";
defineProps<{ switches: PhoneSwitch[] }>();
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
          <!-- unknown: a neutral, waiting toggle; off: gray; on: green -->
          <span
            :class="[
              'relative h-[26px] w-[44px] shrink-0 rounded-full transition-colors duration-500',
              x.state === 'on' ? 'bg-success' : x.state === 'unknown' ? 'bg-surface-cream-strong opacity-60' : 'bg-surface-cream-strong',
            ]"
          >
            <span
              :class="[
                'absolute top-[2px] left-0 size-[22px] rounded-full bg-canvas shadow-sm transition-transform duration-500',
                x.state === 'on' ? 'translate-x-[20px]' : 'translate-x-[2px]',
                x.state === 'unknown' ? 'animate-pulse motion-reduce:animate-none' : '',
              ]"
            />
          </span>
        </div>
        <!-- Name exactly what to do when a switch is off, and where to find it. -->
        <p v-if="x.state === 'off'" class="mt-1.5 text-[12px] text-body">{{ x.fix }}</p>
        <p v-else-if="x.state === 'unknown'" class="mt-1.5 text-[12px] text-muted-soft">Checking…</p>
      </li>
    </ul>
  </div>
</template>
