<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, type Component } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { Bell, Check, CircleAlert, ClipboardList, CloudSun, FolderOpen, Info, Minus, Music, Plug, Plus, ShieldCheck, SlidersHorizontal, Smartphone, Sparkles, SquareTerminal, X } from "lucide-vue-next";
import { api, errorMessage } from "../lib/ipc";
import { useTugStore, type SettingsSection } from "../stores/tug";
import { useWeatherStore } from "../stores/weather";
import { connectionHealth, errorAge, type HealthLink, type LinkState } from "../lib/health";
import { stepZoom } from "../lib/zoom";
import { escClosesSettings } from "../lib/escape";
import { formatAddress } from "../lib/format";
import { normalizeAddress } from "../lib/address";
import { SPOTIFY_BETA_LABEL, SPOTIFY_BETA_NOTE, SPOTIFY_RECONNECT } from "../lib/spotify";
import AppAvatar from "./AppAvatar.vue";
import ConnectPanel from "./ConnectPanel.vue";
import DeveloperSettings from "./DeveloperSettings.vue";
import SettingsRow from "./SettingsRow.vue";
import SettingsSwitch from "./SettingsSwitch.vue";

const tug = useTugStore();
const weather = useWeatherStore();

const SECTIONS: Array<{ id: SettingsSection; label: string; icon: Component }> = [
  { id: "general", label: "General", icon: SlidersHorizontal },
  { id: "iphone", label: "iPhone", icon: Smartphone },
  { id: "notifications", label: "Notifications", icon: Bell },
  { id: "connectors", label: "Connectors", icon: Plug },
  { id: "weather", label: "Weather", icon: CloudSun },
  { id: "developer", label: "Developer tools", icon: SquareTerminal },
  { id: "privacy", label: "Data & privacy", icon: ShieldCheck },
  { id: "about", label: "About", icon: Info },
];
const current = computed(() => SECTIONS.find((s) => s.id === tug.settingsSection) ?? SECTIONS[0]);

// Esc goes back to where you were (unless a dialog or setup is up, or already used the Esc to close itself).
function onKey(e: KeyboardEvent) {
  if (escClosesSettings(e, tug.overlayOpen)) {
    e.preventDefault();
    tug.closeSettings();
  }
}
const version = ref<string | null>(null);
// A slow clock so the last-error "3m ago" stays roughly current while Settings is open.
const now = ref(Date.now());
let clock: number | undefined;
// Windows can turn tug's pop-ups off on its own side; checked on open and on coming back
// (from Windows Settings, say) so the warning goes once they're on again.
const popupsBlocked = ref(false);
const checkPopups = async () => {
  popupsBlocked.value = await api.popupsBlocked().catch(() => false);
};
onMounted(async () => {
  window.addEventListener("keydown", onKey);
  window.addEventListener("focus", checkPopups);
  clock = window.setInterval(() => (now.value = Date.now()), 30_000);
  void checkPopups();
  version.value = await getVersion().catch(() => null);
});
onUnmounted(() => {
  window.removeEventListener("keydown", onKey);
  window.removeEventListener("focus", checkPopups);
  window.clearInterval(clock);
});

// ---- iPhone: Connection health ----
const health = computed<HealthLink[]>(() =>
  connectionHealth(tug.status, { contacts: tug.contacts.length, calls: tug.calls.length }, tug.switchContext),
);
const lastErrorAge = computed(() => errorAge(tug.status.lastErrorAt, now.value));
const STATE_META: Record<LinkState, { dot: string; label: string }> = {
  ok: { dot: "bg-accent-teal", label: "Connected" },
  off: { dot: "bg-accent-amber", label: "Needs attention" },
  error: { dot: "bg-error", label: "Problem" },
  waiting: { dot: "bg-muted-soft", label: "Waiting" },
  unavailable: { dot: "bg-muted-soft", label: "Not available" },
};

const diagnostics = ref<"idle" | "copying" | "done">("idle");
async function copyDiagnostics() {
  diagnostics.value = "copying";
  try {
    await api.copyDiagnostics();
    diagnostics.value = "done";
    tug.notify("info", "Diagnostics copied. Paste them into your support message.");
    window.setTimeout(() => (diagnostics.value = "idle"), 2500);
  } catch (e) {
    diagnostics.value = "idle";
    tug.notify("error", errorMessage(e));
  }
}
async function openLogs() {
  try {
    await api.openLogsFolder();
  } catch (e) {
    tug.notify("error", errorMessage(e));
  }
}

// ---- General ----
const toasts = computed({ get: () => tug.settings.toasts, set: (v) => void tug.setSetting("toasts", v) });
const lowBattery = computed({ get: () => tug.settings.lowBattery, set: (v) => void tug.setSetting("lowBattery", v) });
const popupSound = computed({ get: () => tug.settings.popupSound, set: (v) => void tug.setSetting("popupSound", v) });
const dnd = computed({ get: () => tug.settings.doNotDisturb, set: (v) => void tug.setSetting("doNotDisturb", v) });
const closeToTray = computed({ get: () => tug.settings.closeToTray, set: (v) => void tug.setSetting("closeToTray", v) });
const startWithWindows = computed({ get: () => tug.autostartEnabled, set: (v) => void tug.setAutostart(v) });
const appIcons = computed({ get: () => tug.settings.appIcons, set: (v) => void tug.setSetting("appIcons", v) });
const filterUnknown = computed({ get: () => tug.settings.filterUnknown, set: (v) => void tug.setSetting("filterUnknown", v) });
const advertise = computed({ get: () => tug.advertiseEnabled, set: (v) => void tug.setAdvertising(v) });
const zoomPct = computed(() => `${Math.round(tug.zoom * 100)}%`);

// ---- Notifications ----
// Quiet hours: one schedule, held like Do not disturb (see lib/popup). Empty day list = every day.
const qh = computed(() => tug.settings.quietHours);
const setQuiet = (patch: Partial<typeof tug.settings.quietHours>) => void tug.setSetting("quietHours", { ...tug.settings.quietHours, ...patch });
const quietEnabled = computed({ get: () => qh.value.enabled, set: (v) => setQuiet({ enabled: v }) });
const DAYS: Array<[string, number, string]> = [
  ["S", 0, "Sunday"],
  ["M", 1, "Monday"],
  ["T", 2, "Tuesday"],
  ["W", 3, "Wednesday"],
  ["T", 4, "Thursday"],
  ["F", 5, "Friday"],
  ["S", 6, "Saturday"],
];
const dayOn = (d: number) => qh.value.days.length === 0 || qh.value.days.includes(d);
function toggleDay(d: number) {
  const current = qh.value.days.length ? qh.value.days : [0, 1, 2, 3, 4, 5, 6];
  const next = current.includes(d) ? current.filter((x) => x !== d) : [...current, d].sort((a, b) => a - b);
  setQuiet({ days: next.length === 7 ? [] : next });
}
const muteCalls = computed({ get: () => tug.settings.muteCalls, set: (v) => void tug.setSetting("muteCalls", v) });

// Mute pop-ups per app: every app tug has seen in the Feed, with a toggle each.
const seenApps = computed(() => {
  const ids = new Set<string>();
  const out: Array<{ appId: string; name: string }> = [];
  for (const n of tug.notifications) {
    if (!ids.has(n.appId)) {
      ids.add(n.appId);
      out.push({ appId: n.appId, name: tug.appNameFor(n.appId) });
    }
  }
  return out.sort((a, b) => a.name.localeCompare(b.name));
});
const isMuted = (appId: string) => tug.settings.mutedApps.includes(appId);

// Always let through (VIPs): picked from contacts by name or number.
const vipQuery = ref("");
const vipMatches = computed(() => {
  const q = vipQuery.value.trim().toLowerCase();
  if (!q) return [];
  const digits = q.replace(/\D/g, "");
  const have = new Set(tug.settings.vips);
  return tug.contacts
    .filter(
      (c) =>
        !have.has(normalizeAddress(c.address)) &&
        (c.name.toLowerCase().includes(q) || (digits.length >= 3 && c.address.replace(/\D/g, "").includes(digits))),
    )
    .slice(0, 6);
});
const vipList = computed(() =>
  tug.settings.vips.map((address) => {
    const c = tug.contacts.find((c) => normalizeAddress(c.address) === address);
    return { address, name: c?.name ?? formatAddress(address) };
  }),
);
function addVip(c: { address: string }) {
  tug.addVip(c.address);
  vipQuery.value = "";
}

const SHORTCUTS: Array<[string, string[]]> = [
  ["Search, or type an action", ["Ctrl", "K"]],
  ["New message", ["Ctrl", "N"]],
  ["Copy the latest code", ["Ctrl", "Shift", "C"]],
  ["Settings", ["Ctrl", ","]],
  ["Zoom in · out · reset", ["Ctrl", "+  −  0"]],
];

// ---- iPhone: experimental calling, on only after the hands-free check passes ----
const checkingCalls = ref(false);
async function checkCalls() {
  checkingCalls.value = true;
  await tug.checkDialing();
  checkingCalls.value = false;
}

// ---- Connectors ----
async function disconnectSpotify() {
  await tug.disconnectSpotify();
  tug.notify("info", "Disconnected from Spotify.");
}

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
  // The store clears notifications; texts and recent calls went too.
  tug.messages = [];
  tug.calls = [];
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

    <!-- The window frame and the section list never scroll. The layout is tightened so most
         sections fit; a section with genuinely more content scrolls inside this content area only. -->
    <main class="min-w-0 flex-1 overflow-y-auto">
      <div class="mx-auto max-w-3xl px-8 pt-6 pb-8">
        <header class="mb-5 flex items-center">
          <h1 class="headline text-[36px] leading-none">{{ current.label }}</h1>
          <button class="btn-secondary btn-sm ml-auto" title="Back (Esc)" @click="tug.closeSettings()">
            <X :size="14" /> Done
          </button>
        </header>

        <!-- General: two columns of rows on wide windows so the whole section fits without scrolling. -->
        <template v-if="current.id === 'general'">
          <div
            v-if="popupsBlocked"
            class="mb-4 flex items-start gap-2.5 rounded-xl border border-error/30 bg-canvas px-4 py-3 text-[13px] text-body-strong"
          >
            <CircleAlert :size="16" class="mt-0.5 shrink-0 text-error" />
            <div class="min-w-0 flex-1">
              <p class="font-medium">Windows is blocking tug's pop-ups</p>
              <p class="mt-0.5 text-muted">Notifications still collect in the Feed. Turn on notifications for tug in Windows to see pop-ups.</p>
            </div>
            <button class="btn-secondary btn-sm shrink-0" @click="api.openWindowsSettings('notifications')">Open notification settings</button>
          </div>
          <div class="grid grid-cols-1 gap-px overflow-hidden rounded-xl bg-hairline-soft lg:grid-cols-2">
            <SettingsRow class="bg-surface-card" label="Windows pop-ups" description="Pop up new notifications on this PC.">
              <SettingsSwitch v-model="toasts" label="Windows pop-ups" />
            </SettingsRow>
            <SettingsRow class="bg-surface-card" label="Pop-up sound" description="Play the Windows sound with tug's pop-ups. Your iPhone already chimes.">
              <SettingsSwitch v-model="popupSound" label="Pop-up sound" :disabled="!tug.settings.toasts" />
            </SettingsRow>
            <SettingsRow class="bg-surface-card" label="Do not disturb" description="Keep collecting, stop popping up.">
              <SettingsSwitch v-model="dnd" label="Do not disturb" :disabled="!tug.settings.toasts" />
            </SettingsRow>
            <SettingsRow class="bg-surface-card" label="Low phone battery" description="Pop up when your iPhone drops to 20% and again at 10%.">
              <SettingsSwitch v-model="lowBattery" label="Low phone battery" :disabled="!tug.settings.toasts" />
            </SettingsRow>
            <SettingsRow
              class="bg-surface-card"
              label="Keep running when closed"
              description="Closing the window keeps tug in the tray, still mirroring your iPhone. Quit from the tray menu."
            >
              <SettingsSwitch v-model="closeToTray" label="Keep running when closed" />
            </SettingsRow>
            <SettingsRow
              class="bg-surface-card"
              label="Start with Windows"
              description="Open tug when you sign in, hidden in the tray so it's mirroring your iPhone from the start."
            >
              <SettingsSwitch v-model="startWithWindows" label="Start with Windows" />
            </SettingsRow>
            <SettingsRow class="bg-surface-card" label="Zoom" description="Ctrl + and Ctrl − work from anywhere.">
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

          <p class="caption-upper mt-6 mb-2 px-1 text-muted">Keyboard</p>
          <div class="grid grid-cols-1 gap-px overflow-hidden rounded-xl bg-hairline-soft lg:grid-cols-2">
            <div v-for="[what, keys] in SHORTCUTS" :key="what" class="flex items-center bg-surface-card px-5 py-2.5 text-[14px] text-ink">
              {{ what }}
              <span class="ml-auto flex gap-1">
                <kbd v-for="k in keys" :key="k" class="rounded border border-hairline bg-canvas px-1.5 font-mono text-[12px] text-body">{{ k }}</kbd>
              </span>
            </div>
          </div>
        </template>

        <!-- iPhone -->
        <template v-else-if="current.id === 'iphone'">
          <!-- Connection: each link's state, a plain-English reason/fix, and the diagnostics tools -->
          <div class="mb-6">
            <p class="caption-upper mb-2 px-1 text-muted">Connection</p>
            <div class="divide-y divide-hairline-soft rounded-xl bg-surface-card">
              <div v-for="l in health" :key="l.key" class="flex items-start gap-3 px-5 py-3">
                <span class="mt-[7px] size-2 shrink-0 rounded-full" :class="STATE_META[l.state].dot" aria-hidden="true" />
                <div class="min-w-0 flex-1">
                  <p class="text-[14px] font-medium text-ink">{{ l.label }}</p>
                  <p class="mt-0.5 text-[13px] leading-snug text-muted">{{ l.detail }}</p>
                </div>
                <span class="shrink-0 pt-0.5 text-[12px] text-muted-soft">{{ STATE_META[l.state].label }}</span>
              </div>
            </div>

            <div
              v-if="tug.status.lastError"
              class="mt-3 flex items-start gap-2.5 rounded-xl border border-error/30 bg-canvas px-4 py-3 text-[13px] text-body-strong"
            >
              <CircleAlert :size="16" class="mt-0.5 shrink-0 text-error" />
              <div class="min-w-0 flex-1">
                <p class="selectable">{{ tug.status.lastError }}</p>
                <p v-if="lastErrorAge" class="mt-0.5 text-[12px] text-muted">Last error {{ lastErrorAge }}</p>
              </div>
            </div>

            <div class="mt-3 flex flex-wrap gap-2">
              <button class="btn-secondary btn-sm" :disabled="diagnostics === 'copying'" @click="copyDiagnostics">
                <ClipboardList :size="14" /> {{ diagnostics === "done" ? "Copied" : "Copy diagnostics" }}
              </button>
              <button class="btn-secondary btn-sm" @click="openLogs">
                <FolderOpen :size="14" /> Open logs folder
              </button>
            </div>
            <p class="mt-2 px-1 text-[12px] text-muted-soft">
              Copy diagnostics puts your connection state and recent logs on the clipboard for support, with phone numbers, names and
              message contents removed.
            </p>
          </div>

          <!-- Texts pairing missing, or broken (Windows' half no longer works): say what to do -->
          <div
            v-if="tug.status.device && (tug.status.textsPairing === 'missing' || tug.status.textsPairing === 'broken')"
            class="mb-6 flex items-start gap-3 rounded-xl border border-accent-amber/40 bg-canvas px-4 py-3.5"
          >
            <div class="min-w-0 flex-1 text-[13px] text-body">
              <p class="text-[14px] font-medium text-ink">
                {{ tug.status.textsPairing === "broken" ? "Texts stopped connecting" : "Texts aren't set up" }}
              </p>
              <p v-if="tug.status.textsPairing === 'broken'" class="mt-1">
                Windows' texts pairing with {{ tug.status.textsDevice ?? "your iPhone" }} no longer works (notifications are fine). In
                Bluetooth settings, remove <strong class="font-medium text-body-strong">{{ tug.status.textsDevice ?? "your iPhone" }}</strong>, then
                choose <strong class="font-medium text-body-strong">Add device › Bluetooth</strong> and pick your iPhone again.
              </p>
              <p v-else class="mt-1">
                Reading and replying to texts needs a second pairing, made from this PC: with your iPhone's Settings › Bluetooth open, choose
                <strong class="font-medium text-body-strong">Add device › Bluetooth</strong> here and pick your iPhone.
              </p>
            </div>
            <button class="btn-secondary btn-sm shrink-0" @click="api.openWindowsSettings('bluetooth')">Open Bluetooth settings</button>
          </div>

          <div class="mb-6 divide-y divide-hairline-soft rounded-xl bg-surface-card">
            <SettingsRow label="Visible to iPhone" description="Lets your phone find and reconnect to this PC.">
              <SettingsSwitch v-model="advertise" label="Visible to iPhone" />
            </SettingsRow>
          </div>

          <p class="caption-upper mb-2 px-1 text-muted">Calls</p>
          <div class="mb-6 divide-y divide-hairline-soft rounded-xl bg-surface-card">
            <SettingsRow
              label="Call from tug"
              description="Places calls on your iPhone over its hands-free link, from recent calls, conversations and contacts. You talk on the phone. Call buttons only appear once this PC passes the check."
            >
              <template #below>
                <p class="mt-1 text-[12px] text-muted-soft">Experimental. Call buttons only appear once this PC passes the check.</p>
              </template>
              <div v-if="tug.canDial" class="flex items-center gap-3">
                <span class="flex items-center gap-1.5 text-[13px] font-medium text-ink"><Check :size="15" class="text-accent-teal" /> On</span>
                <button class="btn-secondary btn-sm" @click="tug.setSetting('dialing', false)">Turn off</button>
              </div>
              <button v-else class="btn-secondary btn-sm" :disabled="checkingCalls || !tug.status.device" @click="checkCalls">
                {{ checkingCalls ? "Checking…" : "Check" }}
              </button>
            </SettingsRow>
          </div>

          <p class="caption-upper mb-2 px-1 text-muted">Your iPhone</p>
          <ConnectPanel context="settings" />
        </template>

        <!-- Notifications -->
        <template v-else-if="current.id === 'notifications'">
          <div class="mb-4 divide-y divide-hairline-soft rounded-xl bg-surface-card">
            <SettingsRow
              label="Filter unknown senders"
              description="Texts from numbers that aren't in your contacts, and that you've never texted, wait in their own list in Messages: no badge, no pop-up. Texts with a code still pop up."
            >
              <SettingsSwitch v-model="filterUnknown" label="Filter unknown senders" />
            </SettingsRow>
          </div>

          <!-- Quiet hours: a schedule that holds pop-ups, like Do not disturb. -->
          <div class="mb-4 rounded-xl bg-surface-card">
            <SettingsRow label="Quiet hours" description="Hold Windows pop-ups on a schedule. Notifications still collect in the Feed. People you always let through still get through.">
              <SettingsSwitch v-model="quietEnabled" label="Quiet hours" :disabled="!tug.settings.toasts" />
            </SettingsRow>
            <SettingsRow class="border-t border-hairline-soft" label="Mute calls"
              description="No pop-up for incoming calls at any time, except from people you always let through. With this off, calls ring through quiet hours and Do not disturb."
            >
              <SettingsSwitch v-model="muteCalls" label="Mute calls" :disabled="!tug.settings.toasts" />
            </SettingsRow>
            <div v-if="qh.enabled" class="flex flex-wrap items-center gap-x-6 gap-y-3 border-t border-hairline-soft px-5 py-4">
              <label class="flex items-center gap-2 text-[13px] text-body">
                From
                <input
                  type="time"
                  :value="qh.start"
                  class="rounded-md border border-hairline bg-canvas px-2 py-1 font-mono text-[13px] text-ink"
                  @change="setQuiet({ start: ($event.target as HTMLInputElement).value })"
                />
                to
                <input
                  type="time"
                  :value="qh.end"
                  class="rounded-md border border-hairline bg-canvas px-2 py-1 font-mono text-[13px] text-ink"
                  @change="setQuiet({ end: ($event.target as HTMLInputElement).value })"
                />
              </label>
              <div class="flex items-center gap-1.5">
                <button
                  v-for="([letter, d, dayName], i) in DAYS"
                  :key="i"
                  type="button"
                  :class="[
                    'size-7 rounded-full text-[12px] font-medium transition-colors',
                    dayOn(d) ? 'bg-ink text-on-dark' : 'bg-canvas text-muted active:bg-surface-cream-strong',
                  ]"
                  :aria-pressed="dayOn(d)"
                  :aria-label="`${dayName}: ${dayOn(d) ? 'on' : 'off'}`"
                  :title="`${dayName}: ${dayOn(d) ? 'on' : 'off'}`"
                  @click="toggleDay(d)"
                >
                  {{ letter }}
                </button>
              </div>
            </div>
          </div>

          <!-- Always let through (VIPs). -->
          <div class="mb-4 rounded-xl bg-surface-card px-5 py-4">
            <p class="text-[14px] font-medium text-ink">Always let through</p>
            <p class="mt-0.5 text-[13px] text-muted">
              These people's texts and calls pop up even during quiet hours or Do not disturb.
            </p>
            <ul v-if="vipList.length" class="mt-3 flex flex-wrap gap-1.5">
              <li v-for="v in vipList" :key="v.address">
                <button class="pill bg-canvas text-ink active:bg-surface-cream-strong" :title="`Remove ${v.name}`" @click="tug.removeVip(v.address)">
                  {{ v.name }}
                  <X :size="12" />
                </button>
              </li>
            </ul>
            <div class="relative mt-3 max-w-xs">
              <input
                v-model="vipQuery"
                type="text"
                placeholder="Add someone by name or number"
                class="input"
                aria-label="Add someone to always let through"
              />
              <ul
                v-if="vipMatches.length"
                class="absolute z-10 mt-1 w-full overflow-hidden rounded-lg border border-hairline bg-canvas shadow-lg"
              >
                <li v-for="c in vipMatches" :key="c.address">
                  <button class="flex w-full items-center gap-3 px-3 py-2 text-left text-[14px] text-ink active:bg-surface-card" @click="addVip(c)">
                    <AppAvatar app-id="com.apple.MobileSMS" :label="c.name" :photo-key="c.address" person size="sm" />
                    <span class="min-w-0 flex-1 truncate">{{ c.name }}</span>
                    <span class="shrink-0 font-mono text-[12px] text-muted-soft">{{ formatAddress(c.address) }}</span>
                  </button>
                </li>
              </ul>
            </div>
            <p v-if="tug.contacts.length === 0" class="mt-2 text-[12px] text-muted-soft">
              Your iPhone's contacts load once it's connected and sharing them.
            </p>
          </div>

          <!-- Mute pop-ups per app: every app seen in the Feed, with a toggle. -->
          <div class="rounded-xl bg-surface-card px-5 py-4">
            <p class="text-[14px] font-medium text-ink">Mute pop-ups</p>
            <p class="mt-0.5 text-[13px] text-muted">
              A muted app still collects in tug and on your phone; it just doesn't pop up here.
            </p>
            <p v-if="seenApps.length === 0" class="mt-3 text-[13px] text-muted-soft">No apps yet.</p>
            <ul v-else class="mt-3 max-h-64 divide-y divide-hairline-soft overflow-y-auto">
              <li v-for="app in seenApps" :key="app.appId" class="flex items-center gap-3 py-2">
                <AppAvatar :app-id="app.appId" :label="app.name" size="sm" />
                <span class="min-w-0 flex-1 truncate text-[14px] text-ink">{{ app.name }}</span>
                <SettingsSwitch :model-value="isMuted(app.appId)" :label="`Mute ${app.name}`" @update:model-value="tug.toggleMuted(app.appId)" />
              </li>
            </ul>
          </div>
        </template>

        <!-- Connectors -->
        <template v-else-if="current.id === 'connectors'">
          <div class="divide-y divide-hairline-soft rounded-xl bg-surface-card">
            <div class="flex items-center gap-4 px-5 py-4">
              <span class="flex size-10 shrink-0 items-center justify-center rounded-lg bg-[#1db954] text-white">
                <Music :size="20" />
              </span>
              <div class="min-w-0 flex-1">
                <p class="flex flex-wrap items-center gap-2 text-[14px] font-medium text-ink">
                  Spotify
                  <span class="pill bg-surface-cream-strong px-2 py-0 text-[11px] text-body">{{ SPOTIFY_BETA_LABEL }}</span>
                </p>
                <p class="mt-0.5 text-[13px] text-muted">
                  {{
                    tug.spotify.connected
                      ? tug.spotify.account
                        ? `Connected as ${tug.spotify.account}`
                        : "Connected"
                      : "Your playlists, likes, repeat, shuffle and album art."
                  }}
                </p>
                <p v-if="!tug.spotify.connected" class="mt-0.5 text-[12px] text-muted-soft">{{ SPOTIFY_BETA_NOTE }}</p>
                <p v-else-if="tug.spotify.needsReconnect" class="mt-0.5 text-[12px] text-body">{{ SPOTIFY_RECONNECT }}</p>
              </div>
              <button
                v-if="tug.spotify.connected && tug.spotify.needsReconnect"
                class="btn-primary btn-sm"
                :disabled="tug.spotifyConnecting"
                @click="tug.connectSpotify()"
              >
                {{ tug.spotifyConnecting ? "Connecting…" : "Reconnect" }}
              </button>
              <button v-if="tug.spotify.connected" class="btn-secondary btn-sm" @click="disconnectSpotify">Disconnect</button>
              <button v-else class="btn-primary btn-sm" :disabled="tug.spotifyConnecting" @click="tug.connectSpotify()">
                {{ tug.spotifyConnecting ? "Connecting…" : "Connect" }}
              </button>
            </div>
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
              <template #below>
                <p v-if="tug.showConnect" class="mt-1 text-[12px] text-muted-soft">Shows on the Feed once your iPhone is connected.</p>
              </template>
              <button v-if="weather.place === 'off'" class="btn-secondary btn-sm" :disabled="tug.showConnect" @click="changePlace">Show</button>
              <div v-else class="flex gap-2">
                <button class="btn-secondary btn-sm" :disabled="tug.showConnect" @click="changePlace">
                  {{ weather.place ? "Change place" : "Set up" }}
                </button>
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

        <!-- Developer tools: AI tools (MCP) and the tug command -->
        <template v-else-if="current.id === 'developer'">
          <DeveloperSettings />
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
              description="Weather, only if you turn it on: the place you pick (rounded to about 1 km) goes to the forecast service, and 'Use my location' asks a lookup service for the town's name. App icons: each app's ID (like com.google.Gmail, never what it sent you) goes to Apple's App Store once."
            />
            <SettingsRow
              label="Spotify"
              description="Connecting signs you in to Spotify; tug then asks Spotify for what's playing, your playlists and cover art."
            />
            <SettingsRow
              label="Tugboat"
              description="Only while Tugboat is open: an encrypted link on your Wi-Fi. Nothing goes over the internet. Received files go to Pictures › Tugboat."
            />
            <SettingsRow
              label="App icons"
              description="Show each app's real icon in the Feed instead of its initials. Fetched once per app from Apple's App Store and kept on this PC."
            >
              <SettingsSwitch v-model="appIcons" label="App icons" />
            </SettingsRow>
            <SettingsRow
              label="Clear history"
              description="Deletes tug's copy of your notifications, texts and recent calls; they won't come back from your phone. Your iPhone keeps its own, and your settings, pairing and contacts stay."
            >
              <button
                :class="['btn-secondary btn-sm', confirmClear ? 'text-error' : '']"
                @click="clearHistory"
              >
                {{ confirmClear ? "Click again to delete" : "Clear history" }}
              </button>
            </SettingsRow>
          </div>
        </template>

        <!-- About -->
        <template v-else>
          <div class="divide-y divide-hairline-soft rounded-xl bg-surface-card">
            <SettingsRow label="tug" :description="version ? `Version ${version}` : 'Version unavailable'" />
            <SettingsRow label="What's new" description="See what changed in this and earlier updates.">
              <button class="btn-secondary btn-sm" @click="tug.openWhatsNew()">
                <Sparkles :size="14" /> What's new
              </button>
            </SettingsRow>
            <SettingsRow
              label="Credits"
              description="Forecasts by Open-Meteo (CC BY 4.0). Place names by BigDataCloud. Icons by Lucide."
            />
            <SettingsRow
              label="Copy diagnostics"
              description="Connection state and recent logs on the clipboard for support, with phone numbers, names and message contents removed."
            >
              <button class="btn-secondary btn-sm" :disabled="diagnostics === 'copying'" @click="copyDiagnostics">
                <ClipboardList :size="14" /> {{ diagnostics === "done" ? "Copied" : "Copy diagnostics" }}
              </button>
            </SettingsRow>
          </div>
        </template>
      </div>
    </main>
  </div>
</template>
