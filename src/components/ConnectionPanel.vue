<script setup lang="ts">
import PhoneSetup from "./PhoneSetup.vue";

// Shown beside the feed on wide windows while there's no connected iPhone: first-run
// setup and reconnecting. Everything else lives in Settings.
</script>

<template>
  <aside class="flex h-full flex-col overflow-y-auto bg-surface-soft">
    <header class="flex items-center px-6 pt-6 pb-2">
      <h2 class="headline text-[28px] leading-none">Connection</h2>
    </header>
    <div class="px-6 pt-4 pb-8">
      <PhoneSetup />
    </div>
  </aside>
</template>
