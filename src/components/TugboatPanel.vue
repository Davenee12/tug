<script setup lang="ts">
// Tugboat: a QR code the iPhone's camera opens as a small tug page in Safari. Files and text go
// both ways over the home Wi-Fi while this is open; closing it switches Tugboat off.
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import {
  ArrowDownToLine,
  Check,
  Copy,
  FileText,
  FolderOpen,
  LoaderCircle,
  QrCode,
  Send,
  Smartphone,
  Upload,
  WifiOff,
  X,
} from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { useTugboatStore } from "../stores/tugboat";
import { useFocusTrap } from "../lib/focusTrap";
import { copyText } from "../lib/clipboard";
import { capitalised, closeWarning, formatSize, percent, showConnectHelp, skippedMessage } from "../lib/tugboat";

const tug = useTugStore();
const tugboat = useTugboatStore();
const s = computed(() => tugboat.status);
const root = ref<HTMLElement | null>(null);

useFocusTrap(root, () => requestClose());

// The "Can't connect?" help appears once the code has been up ~30 s with no phone.
const now = ref(Date.now());
let clock: number | undefined;
onMounted(() => (clock = window.setInterval(() => (now.value = Date.now()), 1000)));
onUnmounted(() => window.clearInterval(clock));
const help = computed(() => showConnectHelp(s.value, tugboat.shownAt, now.value));

// The phone as its page reported it ("iPhone", "Android phone"…); just "phone" until one connects.
const phoneName = computed(() => s.value.phone ?? "phone");
/** Once a phone is connected the code steps aside (it can be shown again). After a network
 * change the PC waits for a fresh scan, so the panel is back to "waiting" and shows the code. */
const showCode = ref(false);
watch(
  () => s.value.phase,
  (p) => {
    if (p !== "connected") showCode.value = false;
  },
);

// Closing while files are moving either way, or with offered files the phone hasn't saved yet,
// asks first; otherwise it just closes. The question goes away once nothing is pending.
const confirmClose = ref(false);
const pending = computed(() => closeWarning(s.value));
watch(pending, (w) => {
  if (!w) confirmClose.value = false;
});
function requestClose() {
  if (pending.value && !confirmClose.value) {
    confirmClose.value = true;
    return;
  }
  void tugboat.close();
}

// The backdrop closes the panel only for a click that started on it too, so a text selection
// dragged past the dialog's edge doesn't close Tugboat.
const downOnBackdrop = ref(false);
function onBackdropDown(e: PointerEvent) {
  downOnBackdrop.value = e.target === e.currentTarget;
}
function onBackdropClick(e: MouseEvent) {
  if (downOnBackdrop.value && e.target === e.currentTarget) requestClose();
  downOnBackdrop.value = false;
}

const copiedLink = ref(false);
async function copyLink() {
  // Copied in Rust, kept out of Windows' clipboard history and sync: the link carries the secret.
  if (await tugboat.copyLink()) {
    copiedLink.value = true;
    window.setTimeout(() => (copiedLink.value = false), 1500);
  } else tug.notify("error", "Couldn't copy the link.");
}

const copiedText = ref<number | null>(null);
async function copyPhoneText(id: number, text: string) {
  if (await copyText(text)) copiedText.value = id;
  else tug.notify("error", "Couldn't copy that text.");
}

async function choose() {
  const msg = skippedMessage(await tugboat.pickFiles());
  if (msg) tug.notify("info", msg);
}

const draft = ref("");
async function sendText() {
  const text = draft.value.trim();
  if (!text) return;
  if (await tugboat.sendText(text)) draft.value = "";
  else tug.notify("error", "Couldn't send that text.");
}

const hasIncoming = computed(() => s.value.incoming.length > 0 || s.value.texts.length > 0);
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-start justify-center bg-ink/30 px-6 pt-[6vh] backdrop-blur-[2px]" @pointerdown="onBackdropDown" @click="onBackdropClick">
    <div
      ref="root"
      role="dialog"
      aria-modal="true"
      aria-labelledby="tugboat-title"
      class="flex max-h-[86vh] w-full max-w-[860px] flex-col overflow-hidden rounded-2xl border border-hairline bg-canvas shadow-xl"
    >
      <header class="flex items-center gap-3 border-b border-hairline-soft px-6 py-4">
        <span class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-surface-card text-primary">
          <QrCode :size="18" />
        </span>
        <div class="min-w-0 flex-1">
          <h2 id="tugboat-title" class="headline text-[22px] leading-tight">Tugboat</h2>
        </div>
        <button class="rounded-md p-1.5 text-muted active:bg-surface-card" aria-label="Close Tugboat" @click="requestClose">
          <X :size="18" />
        </button>
      </header>

      <div v-if="confirmClose" class="flex items-center gap-3 border-b border-hairline-soft bg-surface-soft px-6 py-3" role="alert">
        <p v-if="pending === 'unsaved'" class="flex-1 text-[14px] text-ink">Files you offered haven't been saved on your {{ phoneName }} yet. Close anyway?</p>
        <p v-else class="flex-1 text-[14px] text-ink">Files are still moving between your {{ phoneName }} and this PC. Close Tugboat and stop them?</p>
        <button class="btn-secondary btn-sm" @click="confirmClose = false">Keep open</button>
        <button class="btn-primary btn-sm" @click="requestClose">Close</button>
      </div>

      <div class="grid min-h-0 flex-1 overflow-y-auto md:grid-cols-[300px_1fr] md:overflow-hidden">
        <!-- Left: the code, or the connected phone -->
        <section class="flex flex-col border-hairline-soft px-6 py-5 md:overflow-y-auto md:border-r" aria-label="Connect your phone">
          <!-- Turned itself off -->
          <div v-if="s.phase === 'off'" class="flex flex-1 flex-col items-center justify-center gap-3 py-8 text-center">
            <template v-if="tugboat.starting">
              <LoaderCircle :size="22" class="animate-spin text-muted" />
              <p class="text-[14px] text-muted">Starting Tugboat…</p>
            </template>
            <template v-else>
              <p class="text-[14px] text-body">
                {{
                  tugboat.error ??
                  (s.ended === "idle" ? "Tugboat turned off after 10 minutes without use." : "Tugboat is off.")
                }}
              </p>
              <button class="btn-primary btn-sm" @click="tugboat.start()">Start again</button>
            </template>
          </div>

          <!-- No network a phone could use -->
          <div v-else-if="s.phase === 'noNetwork'" class="flex flex-1 flex-col items-center justify-center gap-3 py-8 text-center">
            <WifiOff :size="24" class="text-muted" />
            <p class="text-[14px] text-body">Connect this PC to your Wi-Fi (or a network cable) to use Tugboat.</p>
            <p class="text-[12px] text-muted-soft">The code appears here as soon as it is.</p>
          </div>

          <!-- Connected: the phone, with the code tucked away -->
          <div v-else-if="s.phase === 'connected' && !showCode" class="flex flex-col items-center gap-3 py-6 text-center">
            <span class="flex size-16 items-center justify-center rounded-2xl bg-surface-dark text-on-dark">
              <Smartphone :size="28" />
            </span>
            <p class="font-display text-[22px] leading-tight text-ink">{{ capitalised(phoneName) }} connected</p>
            <p class="flex items-center gap-2 text-[13px] text-muted">
              <span :class="['size-2 rounded-full', s.phoneActive ? 'bg-accent-teal' : 'bg-muted-soft']" />
              {{ s.phoneActive ? `Tugboat is open on your ${phoneName}` : `Open Tugboat on your ${phoneName} to send` }}
            </p>
            <button class="mt-1 text-[13px] font-medium text-muted underline-offset-2 active:text-ink" @click="showCode = true">
              Show the code again
            </button>
          </div>

          <!-- The code -->
          <template v-else-if="s.qr">
            <div class="mx-auto rounded-xl bg-canvas p-3 shadow-sm ring-1 ring-hairline">
              <svg
                :viewBox="`-2 -2 ${s.qr.size + 4} ${s.qr.size + 4}`"
                class="block size-[212px]"
                shape-rendering="crispEdges"
                role="img"
                aria-label="QR code to open Tugboat on your phone"
              >
                <path :d="s.qr.path" class="fill-ink" />
              </svg>
            </div>
            <p class="mt-4 text-center text-[15px] font-medium text-ink">Scan with your phone's camera to send photos, files and text.</p>

            <div v-if="help" class="mt-4 rounded-lg bg-surface-soft px-3.5 py-3 text-[13px] text-body" role="status">
              <p class="font-medium text-ink">Can't connect?</p>
              <ul class="mt-1.5 list-disc space-y-1 pl-4">
                <li>Make sure your phone is on the same Wi-Fi as this PC.</li>
                <li>If Windows asked about tug, allow it on Private networks.</li>
                <li>Guest Wi-Fi, a VPN on the phone, or iCloud Private Relay on an iPhone can block it.</li>
              </ul>
            </div>

            <details class="mt-4 text-[12px] text-muted">
              <summary class="cursor-pointer select-none">Type the link instead</summary>
              <p class="selectable mt-2 font-mono text-[11px] break-all text-body">{{ s.url }}</p>
              <button class="btn-secondary btn-sm mt-2" @click="copyLink">
                <Check v-if="copiedLink" :size="14" class="text-success" />
                <Copy v-else :size="14" />
                {{ copiedLink ? "Copied" : "Copy link" }}
              </button>
            </details>
          </template>

          <p v-if="s.phase !== 'off'" class="mt-auto pt-5 text-center text-[12px] text-muted-soft">Encrypted. Use it on Wi-Fi you trust, like at home.</p>
        </section>

        <!-- Right: what's moving -->
        <div class="min-h-0 px-6 py-5 md:overflow-y-auto">
          <section aria-labelledby="tugboat-from">
            <h3 id="tugboat-from" class="caption-upper text-muted-soft">From your {{ phoneName }}</h3>
            <p v-if="!hasIncoming" class="mt-2 text-[14px] text-muted">Photos, files and text you send from the phone show up here.</p>

            <ul v-if="s.texts.length" class="mt-3 space-y-2">
              <li v-for="(t, i) in s.texts" :key="t.id" class="rounded-lg bg-surface-soft px-3.5 py-3">
                <p class="selectable line-clamp-4 text-[14px] break-words whitespace-pre-wrap text-ink">{{ t.text }}</p>
                <div class="mt-2 flex items-center gap-3">
                  <span v-if="i === 0 && copiedText === null" class="text-[12px] text-muted">Put on your clipboard</span>
                  <button class="btn-secondary btn-sm ml-auto" @click="copyPhoneText(t.id, t.text)">
                    <Check v-if="copiedText === t.id" :size="14" class="text-success" />
                    <Copy v-else :size="14" />
                    {{ copiedText === t.id ? "Copied" : "Copy" }}
                  </button>
                </div>
              </li>
            </ul>

            <ul v-if="s.incoming.length" class="mt-3 divide-y divide-hairline-soft">
              <li v-for="f in s.incoming" :key="f.id" class="flex items-center gap-3 py-2.5">
                <FileText :size="18" class="shrink-0 text-muted" />
                <div class="min-w-0 flex-1">
                  <p class="truncate text-[14px] text-ink">{{ f.name }}</p>
                  <p class="text-[12px] text-muted">
                    {{ f.done ? formatSize(f.size) : `${formatSize(f.received)} of ${formatSize(f.size)}` }}
                  </p>
                  <div v-if="!f.done" class="mt-1.5 h-1 overflow-hidden rounded-full bg-surface-cream-strong">
                    <div class="h-full rounded-full bg-primary transition-[width] duration-300" :style="{ width: `${percent(f.received, f.size)}%` }" />
                  </div>
                </div>
                <button v-if="f.done" class="btn-secondary btn-sm shrink-0" @click="tugboat.openFolder(f.path)">Show</button>
              </li>
            </ul>
          </section>

          <section class="mt-7" aria-labelledby="tugboat-to">
            <h3 id="tugboat-to" class="caption-upper text-muted-soft">To your {{ phoneName }}</h3>
            <div
              :class="[
                'mt-3 flex flex-col items-center gap-2 rounded-xl border border-dashed px-4 py-5 text-center transition-colors',
                tugboat.dragging ? 'border-primary bg-primary/5' : 'border-hairline',
              ]"
            >
              <Upload :size="20" class="text-muted" />
              <p class="text-[14px] text-body">Drag files onto tug to tug them over, or</p>
              <button class="btn-secondary btn-sm" :disabled="s.phase === 'off'" @click="choose">Choose files</button>
              <p class="text-[12px] text-muted-soft">Up to 1 GB each. They show up in Tugboat on the phone to save.</p>
            </div>

            <ul v-if="s.outgoing.length" class="mt-3 divide-y divide-hairline-soft">
              <li v-for="o in s.outgoing" :key="o.id" class="flex items-center gap-3 py-2.5">
                <ArrowDownToLine :size="18" class="shrink-0 text-muted" />
                <div class="min-w-0 flex-1">
                  <p class="truncate text-[14px] text-ink">{{ o.name }}</p>
                  <p class="flex items-center gap-1 text-[12px] text-muted">
                    {{ formatSize(o.size) }}
                    <template v-if="o.downloads > 0">
                      · <Check :size="12" class="text-success" /> On your {{ phoneName }}
                    </template>
                  </p>
                </div>
                <button class="rounded-md p-1.5 text-muted active:bg-surface-card" :aria-label="`Stop offering ${o.name}`" @click="tugboat.removeOffer(o.id)">
                  <X :size="15" />
                </button>
              </li>
            </ul>

            <div class="mt-4">
              <label for="tugboat-text" class="text-[13px] font-medium text-body">Text to copy on your {{ phoneName }}</label>
              <div class="mt-1.5 flex items-start gap-2">
                <textarea
                  id="tugboat-text"
                  v-model="draft"
                  rows="2"
                  class="input h-auto min-h-[64px] resize-y py-2.5"
                  placeholder="A link, an address, a note…"
                  :disabled="s.phase === 'off'"
                  @keydown.ctrl.enter="sendText"
                />
                <button class="btn-primary btn-sm shrink-0" :disabled="!draft.trim() || s.phase === 'off'" @click="sendText">
                  <Send :size="14" />
                  Send
                </button>
              </div>
              <p v-if="s.sentText" class="mt-2 truncate text-[12px] text-muted">On your {{ phoneName }} now: “{{ s.sentText }}”</p>
            </div>
          </section>
        </div>
      </div>

      <footer class="flex items-center gap-3 border-t border-hairline-soft px-6 py-3 text-[12px] text-muted">
        <span class="min-w-0 truncate">Received files go to Pictures › Tugboat</span>
        <button class="btn-secondary btn-sm shrink-0" @click="tugboat.openFolder()">
          <FolderOpen :size="14" />
          Open folder
        </button>
        <span class="ml-auto text-right text-muted-soft">Turns off when you close this or hide tug, or after 10 minutes unused.</span>
      </footer>
    </div>
  </div>
</template>
