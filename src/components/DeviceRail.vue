<script setup lang="ts">
import { computed } from "vue";
import { Battery, BatteryFull, BatteryLow, BatteryMedium, BluetoothOff, Smartphone, Zap } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import NowPlayingCard from "./NowPlayingCard.vue";
import ToggleRow from "./ToggleRow.vue";
import TugMark from "./TugMark.vue";

const tug = useTugStore();
const s = computed(() => tug.status);

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
  <aside class="dark-scroll flex h-full flex-col overflow-y-auto bg-surface-dark px-4 py-6 text-on-dark">
    <div class="flex items-center gap-2.5 px-2">
      <TugMark :size="30" class="text-on-dark" />
      <span class="font-display text-[30px] leading-none text-on-dark" style="letter-spacing: -0.03em">tug</span>
    </div>

    <section class="mt-8 px-2">
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
              <Zap v-if="s.charging" :size="13" class="-ml-1 fill-accent-amber text-accent-amber" aria-label="Charging" />
            </template>
          </div>
        </div>
      </div>

      <div v-if="s.radio === 'off' || s.radio === 'unavailable'" class="mt-4 flex items-center gap-2 rounded-lg bg-error/15 px-3 py-2 text-[13px] text-on-dark">
        <BluetoothOff :size="15" class="text-error" />
        {{ s.radio === "off" ? "Bluetooth is off in Windows" : "No Bluetooth adapter found" }}
      </div>

      <div class="mt-4 flex flex-wrap gap-1.5">
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

    <section class="mt-8">
      <div class="caption-upper mb-1 px-2 text-on-dark-soft">Quick toggles</div>
      <ToggleRow v-model="advertise" label="Visible to iPhone" description="Lets your phone find and reconnect to this PC" />
      <ToggleRow v-model="toasts" label="Windows alerts" description="Pop up new notifications on this PC" />
      <ToggleRow v-model="dnd" label="Do not disturb" description="Keep collecting, stop popping up" :disabled="!tug.settings.toasts" />
    </section>

    <div class="mt-auto pt-8">
      <NowPlayingCard />
    </div>
  </aside>
</template>
