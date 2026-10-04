<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from "vue";
import { CircleAlert, LoaderCircle, RefreshCw, X } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import type { DiscoveredDevice } from "../types/protocol";

defineProps<{ closable: boolean }>();
const emit = defineEmits<{ close: [] }>();

const tug = useTugStore();
const s = computed(() => tug.status);
const scanning = ref(false);
const busyId = ref<string | null>(null);

async function startScan() {
  scanning.value = true;
  await tug.startDiscovery();
}
async function stopScan() {
  scanning.value = false;
  await tug.stopDiscovery();
}

// Scan while there's no iPhone chosen; stop as soon as one is.
watch(
  () => s.value.device,
  (device) => (device ? void stopScan() : void startScan()),
  { immediate: true },
);
onUnmounted(() => {
  if (scanning.value) void tug.stopDiscovery();
});

async function choose(d: DiscoveredDevice) {
  busyId.value = d.id;
  if (d.paired) await tug.useDevice(d.id);
  else await tug.pair(d.id);
  busyId.value = null;
}

const confirmForget = ref(false);
const confirmClear = ref(false);
let resetTimer: number | undefined;
function arm(which: typeof confirmForget) {
  which.value = true;
  window.clearTimeout(resetTimer);
  resetTimer = window.setTimeout(() => {
    confirmForget.value = false;
    confirmClear.value = false;
  }, 4000);
}
async function forget() {
  if (!confirmForget.value) return arm(confirmForget);
  confirmForget.value = false;
  await tug.forget();
}
async function clear() {
  if (!confirmClear.value) return arm(confirmClear);
  confirmClear.value = false;
  await tug.clearHistory();
  tug.notify("info", "History cleared");
}

const advertisingLabel = computed(
  () => ({ off: "Off", starting: "Starting…", on: "On", error: "Failed" })[s.value.advertising],
);
</script>

<template>
  <aside class="flex h-full flex-col overflow-y-auto bg-surface-soft">
    <header class="flex items-center px-6 pt-6 pb-2">
      <h2 class="headline text-[28px] leading-none">Connection</h2>
      <button v-if="closable" class="ml-auto rounded-md p-1.5 text-muted active:bg-surface-card" aria-label="Close" @click="emit('close')">
        <X :size="18" />
      </button>
    </header>

    <div class="flex flex-col gap-6 px-6 pt-4 pb-8">
      <div v-if="s.lastError && s.connection !== 'connected'" class="flex gap-2.5 rounded-xl border border-error/30 bg-canvas px-4 py-3 text-[13px] text-body-strong">
        <CircleAlert :size="16" class="mt-0.5 shrink-0 text-error" />
        <span class="selectable">{{ s.lastError }}</span>
      </div>

      <!-- Paired: device card -->
      <section v-if="s.device" class="rounded-xl border border-hairline bg-canvas p-5">
        <p class="caption-upper text-muted">Paired iPhone</p>
        <p class="headline mt-1 text-[24px]">{{ s.device.name }}</p>
        <p class="mt-1 text-[13px] text-muted">
          <template v-if="s.connection === 'connected'">Connected. tug reconnects by itself when you come back in range.</template>
          <template v-else>
            Waiting for your iPhone. Keep Bluetooth on and the phone nearby; iOS reconnects on its own once it's bonded.
          </template>
        </p>
        <button class="btn-secondary btn-sm mt-4" @click="forget">
          {{ confirmForget ? "Tap again to forget" : "Forget this iPhone" }}
        </button>
        <p class="mt-2 text-[12px] text-muted-soft">
          Also tap <em>Forget This Device</em> under Settings › Bluetooth on the iPhone before pairing again.
        </p>
      </section>

      <!-- Not paired: setup steps -->
      <section v-else>
        <ol class="flex flex-col gap-5">
          <li class="flex gap-3">
            <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-cream-strong text-[12px] font-semibold text-ink">1</span>
            <div class="min-w-0">
              <p class="text-[15px] font-medium text-ink">Make this PC visible</p>
              <p class="text-[13px] text-muted">
                Advertising: <span class="font-medium text-body-strong">{{ advertisingLabel }}</span>
                <template v-if="!tug.advertiseEnabled"> · turn on <em>Visible to iPhone</em> in the left rail</template>
              </p>
              <p v-if="s.peripheralSupported === false" class="mt-1 text-[13px] text-error">
                This PC's Bluetooth adapter can't act as a peripheral, so the iPhone can't connect to it. A Bluetooth 5 USB adapter fixes this.
              </p>
            </div>
          </li>
          <li class="flex gap-3">
            <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-cream-strong text-[12px] font-semibold text-ink">2</span>
            <div class="min-w-0 text-[13px] text-muted">
              <p class="text-[15px] font-medium text-ink">Connect from your iPhone</p>
              <p>
                iOS only lets accessories connect from the phone's side. Install the free <strong class="font-medium text-body-strong">LightBlue</strong> app,
                open it, and tap the entry named after this PC (look for the one advertising a <span class="font-mono text-[12px]">6E4C3A10…</span> service).
                Accept the pairing prompt when it appears, and switch on <strong class="font-medium text-body-strong">Share System Notifications</strong>
                if iOS asks. Without it the phone won't share notifications with this PC.
              </p>
              <p class="mt-1.5">You only do this once. Afterwards iOS reconnects to this PC by itself.</p>
            </div>
          </li>
          <li class="flex gap-3">
            <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-surface-cream-strong text-[12px] font-semibold text-ink">3</span>
            <div class="min-w-0 flex-1">
              <p class="text-[15px] font-medium text-ink">Choose your iPhone</p>
              <p class="text-[13px] text-muted">It jumps to the top marked <em>Connected now</em>. Phones already paired in Windows Settings also show here.</p>
            </div>
          </li>
        </ol>

        <div class="mt-4 flex items-center justify-between">
          <span class="flex items-center gap-2 text-[13px] text-muted">
            <LoaderCircle v-if="scanning" :size="14" class="animate-spin" />
            {{ scanning ? "Scanning nearby devices" : "Scan stopped" }}
          </span>
          <button class="btn-secondary btn-sm" @click="scanning ? stopScan() : startScan()">
            <RefreshCw v-if="!scanning" :size="13" />
            {{ scanning ? "Stop" : "Scan" }}
          </button>
        </div>

        <ul class="mt-3 flex flex-col gap-2">
          <li
            v-for="d in tug.discovered"
            :key="d.id"
            :class="['flex items-center gap-3 rounded-xl border bg-canvas px-4 py-3', d.connected ? 'border-accent-teal/60' : 'border-hairline']"
          >
            <div class="min-w-0 flex-1">
              <p class="truncate text-[14px] font-medium text-ink">{{ d.name }}</p>
              <p class="flex gap-2 text-[12px] text-muted">
                <span v-if="d.connected" class="font-medium text-ink">Connected now</span>
                <span v-if="d.paired">Paired</span>
                <span v-if="d.transport === 'classic'">Windows Settings pairing</span>
              </p>
            </div>
            <button
              :class="[d.connected ? 'btn-primary' : 'btn-secondary', 'btn-sm']"
              :disabled="busyId !== null || (!d.paired && !d.canPair)"
              @click="choose(d)"
            >
              <LoaderCircle v-if="busyId === d.id" :size="13" class="animate-spin" />
              {{ d.paired ? "Use" : "Pair" }}
            </button>
          </li>
          <li v-if="scanning && tug.discovered.length === 0" class="rounded-xl border border-dashed border-hairline px-4 py-6 text-center text-[13px] text-muted-soft">
            Looking for devices…
          </li>
        </ul>
      </section>

      <section>
        <p class="caption-upper mb-2 text-muted">Muted on this PC</p>
        <p v-if="tug.settings.mutedApps.length === 0" class="text-[13px] text-muted-soft">
          No muted apps. Use the bell on any notification to stop its Windows alerts.
        </p>
        <ul v-else class="flex flex-wrap gap-1.5">
          <li v-for="app in tug.settings.mutedApps" :key="app">
            <button class="pill bg-surface-card text-ink active:bg-surface-cream-strong" :title="`Unmute ${app}`" @click="tug.toggleMuted(app)">
              {{ app.split(".").pop() }}
              <X :size="12" />
            </button>
          </li>
        </ul>
      </section>

      <section>
        <p class="caption-upper mb-2 text-muted">History</p>
        <p class="text-[13px] text-muted">Stored only on this PC. Your iPhone keeps its own copy.</p>
        <button class="btn-secondary btn-sm mt-3" @click="clear">
          {{ confirmClear ? "Tap again to delete everything" : "Clear history" }}
        </button>
      </section>

      <section class="rounded-xl bg-surface-card p-4 text-[12px] leading-relaxed text-muted">
        <p class="font-medium text-body-strong">What tug can and can't do</p>
        <p class="mt-1">
          Works over Bluetooth LE with no app on the phone: notifications and their actions (Apple's ANCS), media controls
          (AMS), battery level.
        </p>
        <p class="mt-1">Not yet: replying to messages, calls, clipboard, photos. iOS keeps those out of reach without extra work.</p>
      </section>
    </div>
  </aside>
</template>
