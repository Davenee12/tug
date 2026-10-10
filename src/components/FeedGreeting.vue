<script setup lang="ts">
// The Feed's one-line hello (lib/greeting). Its own small component so a new line re-renders just
// this, and the time comes from the weather store's coarse clock: no timer of its own.
import { computed } from "vue";
import { useWeatherStore } from "../stores/weather";
import { greeting } from "../lib/greeting";
import { withCurrentHour } from "../lib/weather";

const w = useWeatherStore();
const text = computed(() => {
  const f = w.place && w.place !== "off" && w.forecast ? withCurrentHour(w.forecast, w.now) : null;
  return greeting(new Date(w.now), f, w.unit);
});
</script>

<template>
  <p class="truncate px-1 text-[13px] text-muted">{{ text }}</p>
</template>
