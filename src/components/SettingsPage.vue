<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch, type Component } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { Bell, Check, CircleAlert, ClipboardList, CloudSun, Copy, FolderOpen, Info, Minus, Music, Plus, ShieldCheck, SlidersHorizontal, Smartphone, X } from "lucide-vue-next";
import { api, errorMessage } from "../lib/ipc";
import { useTugStore, type SettingsSection } from "../stores/tug";
import { useWeatherStore } from "../stores/weather";
import { connectionHealth, errorAge, type HealthLink, type LinkState } from "../lib/health";
import { stepZoom } from "../lib/zoom";
import { escClosesSettings } from "../lib/escape";
import PhoneSetup from "./PhoneSetup.vue";
import SettingsRow from "./SettingsRow.vue";
import SettingsSwitch from "./SettingsSwitch.vue";

const tug = useTugStore();
const weather = useWeatherStore();

const SECTIONS: Array<{ id: SettingsSection; label: string; icon: Component }> = [
  { id: "general", label: "General", icon: SlidersHorizontal },
  { id: "iphone", label: "iPhone", icon: Smartphone },
  { id: "notifications", label: "Notifications", icon: Bell },
  { id: "spotify", label: "Spotify", icon: Music },
  { id: "weather", label: "Weather", icon: CloudSun },
  { id: "privacy", label: "Data & privacy", icon: ShieldCheck },
  { id: "about", label: "About", icon: Info },
];
const current = computed(() => SECTIONS.find((s) => s.id === tug.settingsSection) ?? SECTIONS[0]);

// Esc goes back to where you were (unless a dialog or setup is up, or already used the Esc to close itself).
function onKey(e: KeyboardEvent) {
  if (escClosesSettings(e, tug.overlayOpen || tug.showSetup)) {
    e.preventDefault();
    tug.closeSettings();
  }
}
const version = ref<string | null>(null);
// A slow clock so the last-error "3m ago" stays roughly current while Settings is open.
const now = ref(Date.now());
let clock: number | undefined;
onMounted(async () => {
  window.addEventListener("keydown", onKey);
  clock = window.setInterval(() => (now.value = Date.now()), 30_000);
  version.value = await getVersion().catch(() => null);
});
onUnmounted(() => {
  window.removeEventListener("keydown", onKey);
  window.clearInterval(clock);
});

// ---- iPhone: Connection health ----
const health = computed<HealthLink[]>(() =>
  connectionHealth(tug.status, { contacts: tug.contacts.length, calls: tug.calls.length }),
);
const lastErrorAge = computed(() => errorAge(tug.status.lastErrorAt, now.value));
const STATE_META: Record<LinkState, { dot: string; label: string }> = {
  ok: { dot: "bg-accent-teal", label: "Connected" },
  off: { dot: "bg-accent-amber", label: "Needs attention" },
  error: { dot: "bg-error", label: "Problem" },
  waiting: { dot: "bg-muted-soft", label: "Waiting" },
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
const dnd = computed({ get: () => tug.settings.doNotDisturb, set: (v) => void tug.setSetting("doNotDisturb", v) });
const closeToTray = computed({ get: () => tug.settings.closeToTray, set: (v) => void tug.setSetting("closeToTray", v) });
const startWithWindows = computed({ get: () => tug.autostartEnabled, set: (v) => void tug.setAutostart(v) });
const appIcons = computed({ get: () => tug.settings.appIcons, set: (v) => void tug.setSetting("appIcons", v) });
const filterUnknown = computed({ get: () => tug.settings.filterUnknown, set: (v) => void tug.setSetting("filterUnknown", v) });
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

// ---- iPhone: experimental calling, on only after the hands-free check passes ----
const checkingCalls = ref(false);
async function checkCalls() {
  checkingCalls.value = true;
  await tug.checkDialing();
  checkingCalls.value = false;
}

// ---- Spotify ----
const clientIdInput = ref("");
// Fill the field from the saved Client ID once it loads, but never clobber what's being typed.
watch(
  () => tug.spotify.clientId,
  (v) => {
    if (v != null && !clientIdInput.value) clientIdInput.value = v;
  },
  { immediate: true },
);
const savingClientId = ref(false);
async function saveClientId() {
  savingClientId.value = true;
  await tug.setSpotifyClientId(clientIdInput.value.trim());
  savingClientId.value = false;
  tug.notify("info", "Client ID saved.");
}
async function disconnectSpotify() {
  await tug.disconnectSpotify();
  tug.notify("info", "Disconnected from Spotify.");
}
async function copyRedirect() {
  try {
    await api.copyText(tug.spotify.redirectUri);
    tug.notify("info", "Redirect URI copied.");
  } catch {
    tug.notify("error", "Couldn't copy to the clipboard.");
  }
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
            <SettingsRow label="Low phone battery" description="Pop up when your iPhone drops to 20% and again at 10%.">
              <SettingsSwitch v-model="lowBattery" label="Low phone battery" :disabled="!tug.settings.toasts" />
            </SettingsRow>
            <SettingsRow
              label="Keep running when closed"
              description="Closing the window keeps tug in the tray, still mirroring your iPhone. Quit from the tray menu."
            >
              <SettingsSwitch v-model="closeToTray" label="Keep running when closed" />
            </SettingsRow>
            <SettingsRow
              label="Start with Windows"
              description="Open tug when you sign in, hidden in the tray so it's mirroring your iPhone from the start."
            >
              <SettingsSwitch v-model="startWithWindows" label="Start with Windows" />
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

          <p class="caption-upper mb-2 px-1 text-muted">Calls</p>
          <div class="mb-6 divide-y divide-hairline-soft rounded-xl bg-surface-card">
            <SettingsRow
              label="Call from tug"
              description="Places calls on your iPhone over its hands-free link, from recent calls, conversations and contacts. You talk on the phone. Call buttons only appear once this PC passes the check."
            >
              <template #below>
                <p class="mt-1 text-[12px] text-muted-soft">Experimental: not yet tried with a real iPhone.</p>
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

          <PhoneSetup />

          <button class="mt-6 text-[13px] text-muted underline active:text-ink" @click="tug.setupRequested = true">
            Run setup again
          </button>
        </template>

        <!-- Notifications -->
        <template v-else-if="current.id === 'notifications'">
          <div class="mb-4 rounded-xl bg-surface-card">
            <SettingsRow
              label="Filter unknown senders"
              description="Texts from numbers that aren't in your contacts, and that you've never texted, wait in their own list in Messages: no badge, no pop-up. Texts with a code still pop up."
            >
              <SettingsSwitch v-model="filterUnknown" label="Filter unknown senders" />
            </SettingsRow>
          </div>
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

        <!-- Spotify -->
        <template v-else-if="current.id === 'spotify'">
          <div class="mb-4 rounded-xl bg-surface-card px-5 py-4">
            <p class="text-[14px] font-medium text-ink">Play your Spotify from tug</p>
            <p class="mt-1 text-[13px] text-muted">
              Spotify doesn't offer repeat, shuffle or album art over the iPhone's media link. Connect your own free Spotify
              app and tug adds working repeat and shuffle, a Like button, album art on Now Playing, and your playlists to start
              on your iPhone. Needs Spotify Premium.
            </p>
          </div>

          <div v-if="tug.spotify.connected" class="divide-y divide-hairline-soft rounded-xl bg-surface-card">
            <SettingsRow label="Connected" :description="tug.spotify.account ? `Signed in as ${tug.spotify.account}` : 'Signed in to Spotify'">
              <span class="flex items-center gap-1.5 text-[13px] font-medium text-ink"><Check :size="15" class="text-accent-teal" /> On</span>
            </SettingsRow>
            <SettingsRow label="Disconnect" description="Removes tug's access to your Spotify. Your account isn't changed.">
              <button class="btn-secondary btn-sm" @click="disconnectSpotify">Disconnect</button>
            </SettingsRow>
          </div>

          <template v-else>
            <div class="rounded-xl bg-surface-card px-5 py-4">
              <p class="caption-upper text-muted">Create your free Spotify app</p>
              <ol class="mt-2 list-decimal space-y-2 pl-5 text-[13px] text-body">
                <li>
                  Open
                  <button class="font-medium text-ink underline active:text-primary" @click="api.openUrl('https://developer.spotify.com/dashboard')">
                    developer.spotify.com/dashboard
                  </button>, log in, and click <strong class="text-body-strong">Create app</strong>.
                </li>
                <li>Give it any name and description.</li>
                <li>
                  For <strong class="text-body-strong">Redirect URI</strong>, enter exactly this and click <strong class="text-body-strong">Add</strong>:
                  <span class="mt-1.5 flex items-center gap-2">
                    <code class="selectable rounded bg-canvas px-2 py-1 font-mono text-[12px] text-ink">{{ tug.spotify.redirectUri }}</code>
                    <button class="btn-secondary btn-sm" @click="copyRedirect"><Copy :size="13" /> Copy</button>
                  </span>
                  <span class="mt-1 block text-[12px] text-muted-soft">Copy it exactly, port included: tug listens for Spotify's sign-in at this address.</span>
                </li>
                <li>Under <strong class="text-body-strong">Which API/SDKs are you planning to use?</strong>, tick <strong class="text-body-strong">Web API</strong>.</li>
                <li>Click <strong class="text-body-strong">Save</strong>, open the app's <strong class="text-body-strong">Settings</strong>, and copy its <strong class="text-body-strong">Client ID</strong>.</li>
              </ol>
            </div>

            <div class="mt-4 rounded-xl bg-surface-card px-5 py-4">
              <label for="spotify-client-id" class="text-[14px] font-medium text-ink">Client ID</label>
              <div class="mt-2 flex gap-2">
                <input
                  id="spotify-client-id"
                  v-model="clientIdInput"
                  class="h-9 flex-1 rounded-lg border border-hairline bg-canvas px-3 font-mono text-[13px] text-ink outline-none placeholder:text-muted-soft focus:border-muted-soft"
                  placeholder="Paste your Client ID"
                  spellcheck="false"
                  autocomplete="off"
                />
                <button class="btn-secondary btn-sm" :disabled="savingClientId || !clientIdInput.trim()" @click="saveClientId">Save</button>
              </div>
              <button class="btn-primary btn-sm mt-3" :disabled="!tug.spotify.clientId || tug.spotifyConnecting" @click="tug.connectSpotify()">
                {{ tug.spotifyConnecting ? "Waiting for sign-in…" : "Connect Spotify" }}
              </button>
              <p class="mt-2 text-[12px] text-muted-soft">
                Opens your browser to sign in. A development-mode app allows up to 5 listeners and needs the owner (you) to have
                Spotify Premium.
              </p>
            </div>
          </template>
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
              description="Weather, only if you turn it on: the place you pick (rounded to about 1 km) goes to the forecast service, and 'Use my location' asks a lookup service for the town's name. App icons: each app's ID (like com.google.Gmail, never what it sent you) goes to Apple's App Store once."
            />
            <SettingsRow
              label="App icons"
              description="Show each app's real icon in the Feed instead of its initials. Fetched once per app from Apple's App Store and kept on this PC."
            >
              <SettingsSwitch v-model="appIcons" label="App icons" />
            </SettingsRow>
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
