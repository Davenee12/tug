<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { Battery, BatteryFull, BatteryLow, BatteryMedium, BluetoothOff, Smartphone } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import NowPlayingCard from "./NowPlayingCard.vue";
import ToggleRow from "./ToggleRow.vue";
import TugMark from "./TugMark.vue";

const tug = useTugStore();
const s = computed(() => tug.status);

// The sidebar never scrolls. On a short window, tighten the section gaps, drop the Quick toggles'
// one-line descriptions (the labels stay) and fold the Now Playing card's extra controls into its
// transport row, so the card still fits at the bottom. The threshold sits above 768 — the two-row
// card needs ~780 px, so a 1366×768 laptop (the common case) must use the compact layout to avoid
// clipping the bottom row, which can't scroll into view.
const vh = ref(window.innerHeight);
const onResize = () => (vh.value = window.innerHeight);
onMounted(() => window.addEventListener("resize", onResize));
onUnmounted(() => window.removeEventListener("resize", onResize));
const compact = computed(() => vh.value < 800);

const connectionLabel = computed(() => {
  switch (s.value.connection) {
    case "connected":
      return "Connected";
    case "connecting":
      return "Connecting…";
    case "disconnected":
      return "Waiting for iPhone";
    default:
      return "Not set up";
  }
});

const dotClass = computed(() => {
  switch (s.value.connection) {
    case "connected":
      return "bg-accent-teal";
    case "connecting":
      return "bg-accent-amber animate-pulse";
    default:
      return "bg-on-dark-soft/50";
  }
});

const batteryIcon = computed(() => {
  const b = s.value.battery ?? 0;
  if (b >= 80) return BatteryFull;
  if (b >= 40) return BatteryMedium;
  if (b >= 15) return BatteryLow;
  return Battery;
});

const services = computed(() => [
  { label: "Notifications", on: s.value.services.notifications },
  { label: "Media", on: s.value.services.media },
  { label: "Battery", on: s.value.services.battery },
]);

const advertise = computed({
  get: () => tug.advertiseEnabled,
  set: (v: boolean) => void tug.setAdvertising(v),
});
const toasts = computed({
  get: () => tug.settings.toasts,
  set: (v: boolean) => void tug.setSetting("toasts", v),
});
const dnd = computed({
  get: () => tug.settings.doNotDisturb,
  set: (v: boolean) => void tug.setSetting("doNotDisturb", v),
});
</script>

<template>
  <aside :class="['flex h-full flex-col overflow-hidden bg-surface-dark px-4 text-on-dark', compact ? 'py-4' : 'py-6']">
    <div class="flex items-center gap-2.5 px-2">
      <TugMark :size="30" class="text-on-dark" />
      <span class="font-display text-[30px] leading-none text-on-dark" style="letter-spacing: -0.03em">tug</span>
    </div>

    <section :class="[compact ? 'mt-5' : 'mt-7', 'px-2']">
      <div class="caption-upper text-on-dark-soft">Your iPhone</div>
      <div class="mt-2 flex items-start gap-3">
        <Smartphone :size="20" class="mt-1.5 shrink-0 text-on-dark-soft" />
        <div class="min-w-0 flex-1">
          <p class="truncate font-display text-[26px] leading-tight text-on-dark" style="letter-spacing: -0.02em">
            {{ s.device?.name ?? "No iPhone yet" }}
          </p>
          <div class="mt-1 flex items-center gap-2 text-[13px] text-on-dark-soft">
            <span :class="['size-2 rounded-full', dotClass]" />
            {{ connectionLabel }}
            <template v-if="s.battery != null">
              <span class="text-on-dark-soft/50">·</span>
              <component :is="batteryIcon" :size="15" :class="s.battery < 15 ? 'text-error' : ''" />
              <span class="font-mono text-[12px]">{{ s.battery }}%</span>
            </template>
          </div>
        </div>
      </div>

      <div v-if="s.radio === 'off' || s.radio === 'unavailable'" class="mt-4 flex items-center gap-2 rounded-lg bg-error/15 px-3 py-2 text-[13px] text-on-dark">
        <BluetoothOff :size="15" class="text-error" />
        {{ s.radio === "off" ? "Bluetooth is off in Windows" : "No Bluetooth adapter found" }}
      </div>

      <div class="mt-3 flex flex-wrap gap-1.5">
        <span
          v-for="svc in services"
          :key="svc.label"
          :class="['pill bg-surface-dark-elevated px-2.5 py-0.5 text-[12px]', svc.on ? 'text-on-dark' : 'text-on-dark-soft/60']"
        >
          <span :class="['size-1.5 rounded-full', svc.on ? 'bg-accent-teal' : 'bg-on-dark-soft/30']" />
          {{ svc.label }}
        </span>
      </div>
    </section>

    <section :class="compact ? 'mt-5' : 'mt-6'">
      <div class="caption-upper mb-1 px-2 text-on-dark-soft">Quick toggles</div>
      <ToggleRow v-model="advertise" label="Visible to iPhone" :description="compact ? undefined : 'Lets your phone find and reconnect to this PC'" />
      <ToggleRow v-model="toasts" label="Windows alerts" :description="compact ? undefined : 'Pop up new notifications on this PC'" />
      <ToggleRow v-model="dnd" label="Do not disturb" :description="compact ? undefined : 'Keep collecting, stop popping up'" :disabled="!tug.settings.toasts" />
    </section>

    <div :class="compact ? 'mt-auto pt-4' : 'mt-auto pt-6'">
      <NowPlayingCard :compact="compact" />
    </div>
  </aside>
</template>
