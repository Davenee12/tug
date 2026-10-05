<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, type Component } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { Bell, Check, CloudSun, Info, Minus, Plus, ShieldCheck, SlidersHorizontal, Smartphone, X } from "lucide-vue-next";
import { useTugStore, type SettingsSection } from "../stores/tug";
import { useWeatherStore } from "../stores/weather";
import { stepZoom } from "../lib/zoom";
import PhoneSetup from "./PhoneSetup.vue";
import SettingsRow from "./SettingsRow.vue";
import SettingsSwitch from "./SettingsSwitch.vue";

const tug = useTugStore();
const weather = useWeatherStore();

const SECTIONS: Array<{ id: SettingsSection; label: string; icon: Component }> = [
  { id: "general", label: "General", icon: SlidersHorizontal },
  { id: "iphone", label: "iPhone", icon: Smartphone },
  { id: "notifications", label: "Notifications", icon: Bell },
  { id: "weather", label: "Weather", icon: CloudSun },
  { id: "privacy", label: "Data & privacy", icon: ShieldCheck },
  { id: "about", label: "About", icon: Info },
];
const current = computed(() => SECTIONS.find((s) => s.id === tug.settingsSection) ?? SECTIONS[0]);

// Esc goes back to where you were (unless a dialog is up; it handles its own Esc).
function onKey(e: KeyboardEvent) {
  if (e.key === "Escape" && !tug.overlayOpen) {
    e.preventDefault();
    tug.closeSettings();
  }
}
const version = ref<string | null>(null);
onMounted(async () => {
  window.addEventListener("keydown", onKey);
  version.value = await getVersion().catch(() => null);
});
onUnmounted(() => window.removeEventListener("keydown", onKey));

// ---- General ----
const toasts = computed({ get: () => tug.settings.toasts, set: (v) => void tug.setSetting("toasts", v) });
const dnd = computed({ get: () => tug.settings.doNotDisturb, set: (v) => void tug.setSetting("doNotDisturb", v) });
const closeToTray = computed({ get: () => tug.settings.closeToTray, set: (v) => void tug.setSetting("closeToTray", v) });
const advertise = computed({ get: () => tug.advertiseEnabled, set: (v) => void tug.setAdvertising(v) });
const zoomPct = computed(() => `${Math.round(tug.zoom * 100)}%`);

const SHORTCUTS: Array<[string, string[]]> = [
  ["Search, or type an action", ["Ctrl", "K"]],
  ["New message", ["Ctrl", "N"]],
  ["Copy the latest code", ["Ctrl", "Shift", "C"]],
  ["Settings", ["Ctrl", ","]],
  ["Zoom in · out · reset", ["Ctrl", "+  −  0"]],
];

// ---- iPhone: the three switches on the phone, checked live ----
type Check = { label: string; where: string; state: "ok" | "off" | "unknown"; fix: string };
const checks = computed<Check[]>(() => {
  const s = tug.status;
  const connected = s.connection === "connected";
  const where = "Settings › Bluetooth › ⓘ next to this PC";
  return [
    {
      label: "Share System Notifications",
      where,
      state: !connected ? "unknown" : s.services.notifications ? "ok" : "off",
      fix: "Turn it on so your notifications reach tug.",
    },
    {
      label: "Show Notifications",
      where,
      state: s.messagesError ? "off" : s.services.messages ? "ok" : "unknown",
      fix: "Turn it on so tug can read and send your texts.",
    },
    {
      label: "Sync Contacts",
      where,
      state: s.contactsError ? "off" : tug.contacts.length > 0 ? "ok" : "unknown",
      fix: "Turn it on so tug shows names instead of numbers.",
    },
  ];
});

// ---- Weather ----
function changePlace() {
  weather.reset();
  tug.closeSettings();
  tug.view = "feed";
}

// ---- Data ----
const confirmClear = ref(false);
let clearTimer: number | undefined;
async function clearHistory() {
  if (!confirmClear.value) {
    confirmClear.value = true;
    window.clearTimeout(clearTimer);
    clearTimer = window.setTimeout(() => (confirmClear.value = false), 4000);
    return;
  }
  confirmClear.value = false;
  await tug.clearHistory();
  tug.notify("info", "History cleared");
}
</script>

<template>
  <div class="flex h-full min-h-0">
    <nav class="flex w-60 shrink-0 flex-col border-r border-hairline bg-surface-soft px-3 py-6" aria-label="Settings sections">
      <p class="caption-upper px-3 pb-2 text-muted">Settings</p>
      <button
        v-for="s in SECTIONS"
        :key="s.id"
        :class="[
          'flex items-center gap-3 rounded-lg px-3 py-2 text-left text-[14px] transition-colors',
          s.id === current.id ? 'bg-surface-card font-medium text-ink' : 'text-body active:bg-surface-card',
        ]"
        :aria-current="s.id === current.id ? 'page' : undefined"
        @click="tug.settingsSection = s.id"
      >
        <component :is="s.icon" :size="16" class="shrink-0" />
        {{ s.label }}
      </button>
      <p class="mt-auto px-3 font-mono text-[11px] text-muted-soft">tug {{ version ?? "" }}</p>
    </nav>

    <main class="min-w-0 flex-1 overflow-y-auto">
      <div class="mx-auto max-w-2xl px-10 pt-8 pb-16">
        <header class="mb-6 flex items-center">
          <h1 class="headline text-[36px] leading-none">{{ current.label }}</h1>
          <button class="btn-secondary btn-sm ml-auto" title="Back (Esc)" @click="tug.closeSettings()">
            <X :size="14" /> Done
          </button>
        </header>

        <!-- General -->
        <template v-if="current.id === 'general'">
          <div class="divide-y divide-hairline-soft rounded-xl bg-surface-card">
            <SettingsRow label="Windows alerts" description="Pop up new notifications on this PC.">
              <SettingsSwitch v-model="toasts" label="Windows alerts" />
            </SettingsRow>
            <SettingsRow label="Do not disturb" description="Keep collecting, stop popping up.">
              <SettingsSwitch v-model="dnd" label="Do not disturb" :disabled="!tug.settings.toasts" />
            </SettingsRow>
            <SettingsRow
              label="Keep running when closed"
              description="Closing the window keeps tug in the tray, still mirroring your iPhone. Quit from the tray menu."
            >
              <SettingsSwitch v-model="closeToTray" label="Keep running when closed" />
            </SettingsRow>
            <SettingsRow label="Zoom" description="Ctrl + and Ctrl − work from anywhere.">
              <div class="flex items-center gap-1">
                <button class="btn-secondary btn-sm w-8 px-0" aria-label="Zoom out" @click="tug.setZoom(stepZoom(tug.zoom, -1))">
                  <Minus :size="14" />
                </button>
                <button class="w-14 rounded-md py-1 text-center font-mono text-[13px] text-ink active:bg-surface-cream-strong" title="Reset to 100%" @click="tug.setZoom(1)">
                  {{ zoomPct }}
                </button>
                <button class="btn-secondary btn-sm w-8 px-0" aria-label="Zoom in" @click="tug.setZoom(stepZoom(tug.zoom, 1))">
                  <Plus :size="14" />
                </button>
              </div>
            </SettingsRow>
          </div>

          <p class="caption-upper mt-8 mb-2 px-1 text-muted">Keyboard</p>
          <div class="divide-y divide-hairline-soft rounded-xl bg-surface-card">
            <div v-for="[what, keys] in SHORTCUTS" :key="what" class="flex items-center px-5 py-3 text-[14px] text-ink">
              {{ what }}
              <span class="ml-auto flex gap-1">
                <kbd v-for="k in keys" :key="k" class="rounded border border-hairline bg-canvas px-1.5 font-mono text-[12px] text-body">{{ k }}</kbd>
              </span>
            </div>
          </div>
        </template>

        <!-- iPhone -->
        <template v-else-if="current.id === 'iphone'">
          <div v-if="tug.status.device" class="mb-6">
            <p class="caption-upper mb-2 px-1 text-muted">On your iPhone</p>
            <div class="divide-y divide-hairline-soft rounded-xl bg-surface-card">
              <SettingsRow v-for="c in checks" :key="c.label" :label="c.label" :description="c.where">
                <template v-if="c.state === 'off'" #below>
                  <p class="mt-1 text-[13px] font-medium text-body-strong">{{ c.fix }}</p>
                </template>
                <span
                  v-if="c.state === 'ok'"
                  class="flex items-center gap-1.5 text-[13px] font-medium text-ink"
                ><Check :size="15" class="text-accent-teal" /> On</span>
                <span v-else-if="c.state === 'off'" class="pill bg-accent-amber/20 text-[12px] text-ink">Needs turning on</span>
                <span v-else class="text-[13px] text-muted-soft">Checks when connected</span>
              </SettingsRow>
            </div>
          </div>

          <div class="mb-6 divide-y divide-hairline-soft rounded-xl bg-surface-card">
            <SettingsRow label="Visible to iPhone" description="Lets your phone find and reconnect to this PC.">
              <SettingsSwitch v-model="advertise" label="Visible to iPhone" />
            </SettingsRow>
          </div>

          <PhoneSetup />

          <button class="mt-6 text-[13px] text-muted underline active:text-ink" @click="tug.setupRequested = true">
            Run setup again
          </button>
        </template>

        <!-- Notifications -->
        <template v-else-if="current.id === 'notifications'">
          <div class="rounded-xl bg-surface-card px-5 py-4">
            <p class="text-[14px] font-medium text-ink">Muted on this PC</p>
            <p class="mt-0.5 text-[13px] text-muted">
              Muted apps still collect in tug and on your phone; they just don't pop up here. Mute an app with the bell on any of its
              notifications.
            </p>
            <p v-if="tug.settings.mutedApps.length === 0" class="mt-3 text-[13px] text-muted-soft">Nothing muted.</p>
            <ul v-else class="mt-3 flex flex-wrap gap-1.5">
              <li v-for="app in tug.settings.mutedApps" :key="app">
                <button
                  class="pill bg-canvas text-ink active:bg-surface-cream-strong"
                  :title="`Unmute ${tug.appNameFor(app)}`"
                  @click="tug.toggleMuted(app)"
                >
                  {{ tug.appNameFor(app) }}
                  <X :size="12" />
                </button>
              </li>
            </ul>
          </div>
        </template>

        <!-- Weather -->
        <template v-else-if="current.id === 'weather'">
          <div class="divide-y divide-hairline-soft rounded-xl bg-surface-card">
            <SettingsRow
              label="Weather on the Feed"
              :description="
                weather.place === 'off'
                  ? 'Hidden.'
                  : weather.place
                    ? `Showing ${weather.place.name}.`
                    : 'Not set up yet: pick a place on the Feed.'
              "
            >
              <button v-if="weather.place === 'off'" class="btn-secondary btn-sm" @click="changePlace">Show</button>
              <div v-else class="flex gap-2">
                <button class="btn-secondary btn-sm" @click="changePlace">{{ weather.place ? "Change place" : "Set up" }}</button>
                <button v-if="weather.place" class="btn-secondary btn-sm" @click="weather.hide()">Hide</button>
              </div>
            </SettingsRow>
            <SettingsRow label="Units" description="Also switchable right on the card.">
              <div class="flex rounded-lg bg-canvas p-0.5">
                <button
                  v-for="u in ['f', 'c'] as const"
                  :key="u"
                  :class="['rounded-md px-3 py-1 text-[13px]', weather.unit === u ? 'bg-surface-cream-strong font-medium text-ink' : 'text-muted']"
                  :aria-pressed="weather.unit === u"
                  @click="weather.unit !== u && weather.toggleUnit()"
                >
                  °{{ u.toUpperCase() }}
                </button>
              </div>
            </SettingsRow>
          </div>
        </template>

        <!-- Data & privacy -->
        <template v-else-if="current.id === 'privacy'">
          <div class="divide-y divide-hairline-soft rounded-xl bg-surface-card">
            <SettingsRow
              label="Stays on this PC"
              description="Your notification history, texts and contacts are stored only here, in tug's folder in your Windows profile. tug has no servers and no account."
            />
            <SettingsRow
              label="What goes online"
              description="Only for weather, and only if you turn it on: the place you pick (rounded to about 1 km) goes to the forecast service, and 'Use my location' asks a lookup service for the town's name."
            />
            <SettingsRow label="Clear history" description="Deletes tug's copy of everything. Your iPhone keeps its own.">
              <button
                :class="['btn-secondary btn-sm', confirmClear ? 'text-error' : '']"
                @click="clearHistory"
              >
                {{ confirmClear ? "Tap again to delete" : "Clear history" }}
              </button>
            </SettingsRow>
          </div>
        </template>

        <!-- About -->
        <template v-else>
          <div class="divide-y divide-hairline-soft rounded-xl bg-surface-card">
            <SettingsRow label="tug" :description="version ? `Version ${version}` : 'Development build'" />
            <SettingsRow
              label="Credits"
              description="Forecasts by Open-Meteo (CC BY 4.0). Place names by BigDataCloud. Icons by Lucide."
            />
          </div>
        </template>
      </div>
    </main>
  </div>
</template>
