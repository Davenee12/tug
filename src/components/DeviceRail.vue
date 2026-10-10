<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { Battery, BatteryFull, BatteryLow, BatteryMedium, BluetoothOff, QrCode } from "lucide-vue-next";
import { connectionBusy, connectionLabel as connectionLabelFor } from "../lib/connectionStatus";
import { useTugStore } from "../stores/tug";
import { useTugboatStore } from "../stores/tugboat";
import { phoneModel } from "../lib/phoneModel";
import { canWiggle, shouldTug } from "../lib/logoTricks";
import { logoSpins } from "../lib/logoSpin";
import NowPlayingCard from "./NowPlayingCard.vue";
import PhoneArt from "./PhoneArt.vue";
import ToggleRow from "./ToggleRow.vue";
import TugMark from "./TugMark.vue";

const tug = useTugStore();
const tugboat = useTugboatStore();
const s = computed(() => tug.status);

// The sidebar never scrolls. On a short window, tighten the section gaps, drop the Quick toggles'
// one-line descriptions (the labels stay) and have the Now Playing card drop its second row of extra
// controls (it tucks them beside the title and between the times), so the card still fits at the
// bottom. The threshold sits above 768 — the roomy layout with the two-row card needs ~780 px, so a
// 1366×768 laptop (the common case) must use the compact layout to avoid clipping the bottom row,
// which can't scroll into view.
const vh = ref(window.innerHeight);
const onResize = () => (vh.value = window.innerHeight);
onMounted(() => window.addEventListener("resize", onResize));
onUnmounted(() => window.removeEventListener("resize", onResize));
const compact = computed(() => vh.value < 800);
// Below this even the compact card has no room for the volume bar's row (at the 600 px minimum the
// one-row card only just fits), so the card keeps its volume buttons in the transport row instead.
const tight = computed(() => vh.value < 640);

// The logo's one-shot moments: a tug on the rope when the iPhone connects (each time, not on a
// status refresh that changes nothing), a wiggle on hover (at most every couple of seconds) and a
// spin from search. One short CSS animation each, removed when it ends: nothing runs in between.
// With reduced motion, none of them play.
type Trick = "tug" | "wiggle" | "spin";
const TRICK_CLASS: Record<Trick, string> = { tug: "animate-logo-tug", wiggle: "animate-logo-wiggle", spin: "animate-logo-spin" };
const trick = ref<Trick | null>(null);
/** Keys the logo, so each play is a fresh element and the same animation can start over. */
const plays = ref(0);
const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
function play(kind: Trick) {
  if (reducedMotion.matches) return;
  if (trick.value === "spin" && kind === "wiggle") return; // let the big one finish
  trick.value = kind;
  plays.value++;
}
watch(
  () => (tug.statusKnown ? s.value.connection : null),
  (next, prev) => {
    if (shouldTug(prev ?? null, next)) play("tug");
  },
);
let wiggledAt: number | null = null;
function onLogoHover() {
  if (trick.value || !canWiggle(wiggledAt, Date.now())) return;
  wiggledAt = Date.now();
  play("wiggle");
}
watch(logoSpins, () => play("spin"));

// The phone card pictures the user's exact iPhone (model read over Bluetooth, kept while it's away).
// The model line only shows for a model tug knows; otherwise the picture is a generic iPhone.
const model = computed(() => phoneModel(s.value.device?.model));

const connectionLabel = computed(() => connectionLabelFor(s.value));

const dotClass = computed(() => {
  if (s.value.connection === "connected") return "bg-accent-teal";
  return connectionBusy(s.value) ? "bg-accent-amber animate-pulse motion-reduce:animate-none" : "bg-on-dark-soft/50";
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
      <span :key="plays" :class="['inline-flex', trick && TRICK_CLASS[trick]]" @mouseenter="onLogoHover" @animationend="trick = null">
        <TugMark :size="30" class="text-on-dark" />
      </span>
      <span class="font-display text-[30px] leading-none text-on-dark" style="letter-spacing: -0.03em">tug</span>
    </div>

    <!-- Phone card, like a device card: the phone pictured on the left; its name, model, connection
         and battery on the right. Takes the place of the old "Your iPhone" caption, so it's no taller. -->
    <section :class="[compact ? 'mt-5' : 'mt-7', 'px-2']" aria-label="Your iPhone">
      <div class="flex items-center gap-3.5">
        <PhoneArt
          :face="model.face"
          :size="model.size"
          :height="compact ? 52 : 56"
          :lit="s.connection === 'connected'"
          :class="s.device ? '' : 'opacity-50'"
        />
        <div class="min-w-0 flex-1">
          <p class="truncate font-display text-[26px] leading-tight text-on-dark" style="letter-spacing: -0.02em">
            {{ s.device?.name ?? "No iPhone yet" }}
          </p>
          <p v-if="s.device && model.known" class="truncate text-[12px] leading-snug text-on-dark-soft">{{ model.name }}</p>
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

      <div class="mt-3 flex flex-wrap items-center gap-1.5">
        <span
          v-for="svc in services"
          :key="svc.label"
          :class="['pill bg-surface-dark-elevated px-2.5 py-0.5 text-[12px]', svc.on ? 'text-on-dark' : 'text-on-dark-soft/60']"
        >
          <span :class="['size-1.5 rounded-full', svc.on ? 'bg-accent-teal' : 'bg-on-dark-soft/30']" />
          {{ svc.label }}
        </span>
        <!-- Tugboat: files and text over Wi-Fi, so it works even while the Bluetooth link is down.
             It sits at the end of this row (which wraps anyway) so the sidebar, which never
             scrolls, doesn't grow. -->
        <button
          class="ml-auto inline-flex items-center gap-1.5 rounded-full border border-on-dark-soft/30 px-2.5 py-0.5 text-[12px] font-medium text-on-dark transition-colors active:bg-surface-dark-elevated"
          title="Tugboat: send photos, files and text between your phone and this PC"
          @click="tugboat.show()"
        >
          <QrCode :size="13" class="text-primary" />
          Tugboat
          <span v-if="tugboat.status.phase !== 'off'" class="size-1.5 rounded-full bg-accent-teal" aria-hidden="true" />
          <span v-if="tugboat.status.phase !== 'off'" class="sr-only">(on)</span>
        </button>
      </div>
    </section>

    <section :class="compact ? 'mt-5' : 'mt-6'">
      <div class="caption-upper mb-1 px-2 text-on-dark-soft">Quick toggles</div>
      <ToggleRow v-model="advertise" label="Visible to iPhone" :description="compact ? undefined : 'Lets your phone find and reconnect to this PC'" />
      <ToggleRow v-model="toasts" label="Windows pop-ups" :description="compact ? undefined : 'Pop up new notifications on this PC'" />
      <ToggleRow v-model="dnd" label="Do not disturb" :description="compact ? undefined : 'Keep collecting, stop popping up'" :disabled="!tug.settings.toasts" />
    </section>

    <div :class="compact ? 'mt-auto pt-4' : 'mt-auto pt-6'">
      <NowPlayingCard :compact="compact" :tight="tight" />
    </div>
  </aside>
</template>
