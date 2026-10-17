<script setup lang="ts">
// The page the iPhone opens from tug's QR code. Phone → PC: photos, videos, files, and text pasted
// into "Paste to PC". PC → phone: the PC's clipboard ("Send clipboard to iPhone"), files dragged onto
// tug, and text to copy. Plain words, big targets, light/dark.
// When tug's game asks, the page becomes its controller (Controller.vue) until the game closes.
import { computed, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import { Check, Copy, Download, FileUp, Gamepad2, Image as ImageIcon, LoaderCircle, Send, WifiOff, X } from "lucide-vue-next";
import TugMark from "../components/TugMark.vue";
import Controller from "./Controller.vue";
import { TugboatError, type TugboatApi, type PageClip, type PageOffer, type PageState } from "./client";
import { MAX_DOWNLOAD, MAX_UPLOAD, canRetryUpload, formatSize, isFatal, messageFor } from "./chunks";
import { ackOutcome, copyWith, isNewClip, needsAck, showClip, type ShownClip } from "./clip";
import { copyEnv, selectText } from "./copy";

const props = defineProps<{ api: TugboatApi | null }>();

const POLL_MS = 2000;

type Connection = "connecting" | "ok" | "network";
const connection = ref<Connection>("connecting");
/** A problem that ends the page (Tugboat closed, link expired, in use elsewhere). */
const fatal = ref<string | null>(props.api ? null : "no-secret");

// --- Polling the PC for files on offer and text ---
const offers = ref<PageOffer[]>([]);
const pcText = ref<PageState["text"]>(null);
let pollTimer: number | undefined;
let polling = false;

async function poll() {
  if (!props.api || fatal.value || polling) return;
  polling = true;
  try {
    const s = await props.api.state();
    offers.value = s.offers;
    forgetWithdrawn(s.offers);
    if (s.text?.id !== pcText.value?.id) copied.value = false;
    pcText.value = s.text;
    game.value = s.game;
    // The PC's clipboard: show a new one; keep telling the PC we have it until it has heard.
    if (isNewClip(clip.value ?? (dismissedClip === null ? null : { id: dismissedClip }), s.clip)) receiveClip(s.clip);
    else if (needsAck(clip.value)) void ackClip();
    guest.value = s.guest === true;
    // The game closed: next time it asks, the controller opens again by itself.
    if (!s.game) padDismissed.value = false;
    connection.value = "ok";
  } catch (e) {
    const code = e instanceof TugboatError ? e.code : "network";
    if (isFatal(code) && code !== "too-big" && code !== "no-space" && code !== "blocked") fatal.value = code;
    else connection.value = "network";
  } finally {
    polling = false;
  }
  schedule();
}

function schedule() {
  window.clearTimeout(pollTimer);
  // Only while the page is on screen (Safari would pause it in the background anyway), and not
  // while it's a controller: the controller's own replies say when the game closes.
  if (document.visibilityState === "visible" && !fatal.value && !controller.value) pollTimer = window.setTimeout(poll, POLL_MS);
}

// --- tug's game wants a controller ---
const game = ref(false);
/** This phone is a two-player race's second controller: no files or text, only the race. */
const guest = ref(false);
/** "Stop using as a controller" was tapped: stay on this page until the game asks again. */
const padDismissed = ref(false);
const controller = computed(() => props.api !== null && game.value && !padDismissed.value && !fatal.value);
watch(controller, (on) => {
  if (on) window.clearTimeout(pollTimer);
  else void poll();
});
function leftController(code: string | null) {
  if (code === null) padDismissed.value = true;
  else if (code === "no-game") game.value = false;
  else fatal.value = code;
}

function onVisibility() {
  if (document.visibilityState === "visible") void poll();
  else window.clearTimeout(pollTimer);
}

onMounted(() => {
  document.addEventListener("visibilitychange", onVisibility);
  void poll();
});
onUnmounted(() => {
  document.removeEventListener("visibilitychange", onVisibility);
  window.clearTimeout(pollTimer);
  for (const d of Object.values(downloads)) if (d.url) URL.revokeObjectURL(d.url);
  if (clip.value?.url) URL.revokeObjectURL(clip.value.url);
});

// --- Sending files to the PC ---
interface Upload {
  key: number;
  file: File;
  sent: number;
  state: "waiting" | "sending" | "saved" | "failed";
  savedAs?: string;
  error?: string;
  /** A failed upload that "Try again" can send again. */
  retry?: boolean;
  ctrl: AbortController;
}
const uploads = ref<Upload[]>([]);
let nextKey = 1;
let pumping = false;

function pick(e: Event) {
  const input = e.target as HTMLInputElement;
  const files = Array.from(input.files ?? []);
  input.value = ""; // so picking the same file again still counts
  for (const file of files) {
    const u: Upload = { key: nextKey++, file, sent: 0, state: "waiting", ctrl: new AbortController() };
    if (file.size > MAX_UPLOAD) Object.assign(u, { state: "failed", error: "Over 8 GB, too big to send." });
    uploads.value.unshift(u);
  }
  void pump();
}

/** Send waiting files one at a time, oldest first. */
async function pump() {
  if (pumping || !props.api) return;
  pumping = true;
  try {
    for (;;) {
      const next = [...uploads.value].reverse().find((u) => u.state === "waiting");
      if (!next) break;
      const u = uploads.value.find((x) => x.key === next.key)!;
      u.state = "sending";
      try {
        u.savedAs = await props.api.upload(u.file, (sent) => (u.sent = sent), u.ctrl.signal);
        u.sent = u.file.size;
        u.state = "saved";
      } catch (e) {
        if (u.ctrl.signal.aborted) continue;
        const code = e instanceof TugboatError ? e.code : "error";
        u.state = "failed";
        u.error = messageFor(code);
        u.retry = canRetryUpload(code);
        if (["closed", "unauthorized", "in-use"].includes(code)) fatal.value = code;
      }
    }
  } finally {
    pumping = false;
  }
}

/** Send a failed file again: back in the queue, from the start (the PC resumes what it has). */
function retry(u: Upload) {
  Object.assign(u, { state: "waiting", sent: 0, error: undefined, retry: false, ctrl: new AbortController() });
  void pump();
}

function cancel(u: Upload) {
  u.ctrl.abort();
  if (u.state === "sending") void props.api?.cancel(u.file);
  uploads.value = uploads.value.filter((x) => x.key !== u.key);
}

const sending = computed(() => uploads.value.some((u) => u.state === "sending" || u.state === "waiting"));
const percent = (u: Upload) => (u.file.size ? Math.round((u.sent / u.file.size) * 100) : 100);

// --- Text ---
const draft = ref("");
const textState = ref<"idle" | "sending" | "sent" | "failed">("idle");
async function sendText() {
  const text = draft.value;
  if (!text.trim() || !props.api) return;
  textState.value = "sending";
  try {
    await props.api.sendText(text);
    draft.value = "";
    textState.value = "sent";
  } catch (e) {
    const code = e instanceof TugboatError ? e.code : "error";
    textState.value = "failed";
    if (["closed", "unauthorized", "in-use"].includes(code)) fatal.value = code;
  }
}

const copied = ref(false);
const copyFailed = ref(false);
const pcTextEl = ref<HTMLElement | null>(null);
async function copyPcText() {
  if (!pcText.value) return;
  const ok = (await copyWith(pcText.value.text, copyEnv())) === "copied";
  copied.value = ok;
  copyFailed.value = !ok;
  if (!ok) selectText(pcTextEl.value);
}

// --- The PC's clipboard ("Send clipboard to iPhone" on the PC) ---
// The PC holds it for up to 2 minutes and only says it was sent once this page tells it so: right
// away for text, once fetched for a picture. The page keeps showing it after that.
const clip = ref<ShownClip | null>(null);
/** The last clipboard put away with ✕, so the PC repeating it doesn't bring it back. */
let dismissedClip: number | null = null;
const clipCopy = ref<"idle" | "copied" | "select">("idle");
const clipTextEl = ref<HTMLElement | null>(null);
let acking = false;

function receiveClip(incoming: PageClip) {
  if (clip.value?.url) URL.revokeObjectURL(clip.value.url);
  clip.value = showClip(incoming);
  clipCopy.value = "idle";
  if (clip.value.phase === "ready") void ackClip();
  else if (clip.value.phase === "loading") void fetchClipImage();
}

async function fetchClipImage() {
  const c = clip.value;
  if (!c?.image || !props.api) return;
  const id = c.id;
  Object.assign(c, { phase: "loading", got: 0, error: undefined });
  try {
    const blob = await props.api.download(c.image, (got) => {
      if (clip.value?.id === id) clip.value.got = got;
    });
    if (clip.value?.id !== id) return;
    clip.value.url = URL.createObjectURL(blob);
    clip.value.phase = "ready";
    void ackClip();
  } catch (e) {
    if (clip.value?.id !== id) return;
    const code = e instanceof TugboatError ? e.code : "error";
    clip.value.phase = "failed";
    clip.value.error = code === "not-found" ? "Your PC isn't sending this picture any more." : messageFor(code);
  }
}

/** Tell the PC this page has its clipboard. Retried at each check-in until the PC has heard. */
async function ackClip() {
  const c = clip.value;
  if (!props.api || !c || !needsAck(c) || acking) return;
  acking = true;
  try {
    await props.api.clipAck(c.id);
    if (clip.value?.id === c.id) clip.value.acked = true;
  } catch (e) {
    const code = e instanceof TugboatError ? e.code : "network";
    const outcome = ackOutcome(code);
    if (outcome === "fatal") fatal.value = code;
    else if (outcome === "done" && clip.value?.id === c.id) clip.value.acked = true;
  } finally {
    acking = false;
  }
}

/** The big Copy button. On this plain-HTTP page the browser may refuse: then the text is selected
 * for a press-and-hold Copy, and the page says so. */
async function copyClip() {
  const text = clip.value?.text;
  if (text === null || text === undefined) return;
  clipCopy.value = await copyWith(text, copyEnv());
  if (clipCopy.value === "select") selectText(clipTextEl.value);
}

function dismissClip() {
  if (!clip.value) return;
  dismissedClip = clip.value.id;
  if (clip.value.url) URL.revokeObjectURL(clip.value.url);
  clip.value = null;
}

// --- Files from the PC ---
interface Fetching {
  state: "loading" | "ready" | "failed";
  got: number;
  url?: string;
  error?: string;
}
const downloads = reactive<Record<string, Fetching>>({});

/** Let go of a fetched file (its memory) and its Save button. */
function release(id: string) {
  const url = downloads[id]?.url;
  if (url) URL.revokeObjectURL(url);
  delete downloads[id];
}

/** The PC stopped offering these: free any copy the page still holds. */
function forgetWithdrawn(offered: PageOffer[]) {
  const still = new Set(offered.map((o) => o.id));
  for (const id of Object.keys(downloads)) if (!still.has(id)) release(id);
}

async function fetchOffer(o: PageOffer) {
  if (!props.api) return;
  // Hold at most one assembled file (they can be up to 1 GB each): fetching another lets go of
  // any earlier one that's ready, which goes back to "Get". No timer: a save sheet can stay open.
  for (const [id, d] of Object.entries(downloads)) if (id !== o.id && d.state === "ready") release(id);
  release(o.id);
  const d: Fetching = { state: "loading", got: 0 };
  downloads[o.id] = d;
  try {
    const blob = await props.api.download(o, (got) => (downloads[o.id].got = got));
    downloads[o.id] = { state: "ready", got: o.size, url: URL.createObjectURL(blob) };
  } catch (e) {
    const code = e instanceof TugboatError ? e.code : "error";
    downloads[o.id] = { state: "failed", got: 0, error: messageFor(code) };
  }
}

const fatalMessage = computed(() =>
  fatal.value === "no-secret" ? "Open this page by scanning the Tugboat code in tug on your PC." : messageFor(fatal.value ?? ""),
);
</script>

<template>
  <Controller v-if="controller && api" :api="api" @leave="leftController" />
  <div v-else class="mx-auto flex min-h-dvh max-w-[560px] flex-col px-4 pt-[max(1.25rem,env(safe-area-inset-top))] pb-[max(1.5rem,env(safe-area-inset-bottom))]">
    <header class="flex items-center gap-2.5">
      <TugMark :size="30" class="text-ink" />
      <h1 class="headline text-[30px] leading-none">Tugboat</h1>
      <span
        v-if="!fatal"
        class="ml-auto inline-flex items-center gap-1.5 rounded-full bg-card px-3 py-1 text-[13px] font-medium text-body"
        role="status"
      >
        <span
          :class="[
            'size-2 rounded-full',
            connection === 'ok' ? 'bg-accent-teal' : connection === 'network' ? 'bg-error' : 'animate-pulse bg-muted',
          ]"
        />
        {{ connection === "ok" ? "Connected" : connection === "network" ? "Can't reach PC" : "Connecting…" }}
      </span>
    </header>
    <p class="mt-2 text-[14px] text-muted">Encrypted between your phone and your PC.</p>

    <!-- Ended: Tugboat closed, link expired, or in use elsewhere. -->
    <div v-if="fatal" class="card mt-8 flex flex-col items-center gap-3 px-6 py-10 text-center">
      <WifiOff :size="28" class="text-muted" />
      <p class="text-[17px] text-ink">{{ fatalMessage }}</p>
    </div>

    <template v-else>
      <p v-if="connection === 'network'" class="mt-4 rounded-xl bg-error/10 px-4 py-3 text-[15px] text-ink" role="alert">
        {{ messageFor("network") }}
      </p>

      <!-- tug's game is asking, but the controller was put away. -->
      <div v-if="game && padDismissed" class="card mt-5 flex items-center gap-3">
        <Gamepad2 :size="22" class="shrink-0 text-primary" />
        <p class="flex-1 text-[15px] text-ink">Tugboat Run is open on your PC.</p>
        <button class="btn-primary btn-sm shrink-0" @click="padDismissed = false">Use as controller</button>
      </div>

      <!-- A race's second phone: nothing but the race (never files, text or the clipboard). -->
      <div v-if="guest && !game" class="card mt-5 flex items-center gap-3" role="status">
        <Gamepad2 :size="22" class="shrink-0 text-accent-teal" />
        <p class="flex-1 text-[15px] text-ink">You're the second controller for Tugboat Run. Keep this page open: you'll join the next two-player race on the PC.</p>
      </div>

      <template v-if="!guest">
        <!-- The PC's clipboard, sent with "Send clipboard to iPhone": the reason the page was opened. -->
        <section v-if="clip" class="mt-6" aria-labelledby="clip-title">
          <div class="flex items-center gap-2">
            <h2 id="clip-title" class="headline flex-1 text-[22px]">Your PC's clipboard</h2>
            <button
              v-if="clip.acked || clip.phase === 'failed'"
              class="-mr-1 shrink-0 rounded-lg p-2 text-muted active:bg-card-strong"
              aria-label="Put away"
              @click="dismissClip"
            >
              <X :size="18" />
            </button>
          </div>
          <div class="card mt-3">
            <template v-if="clip.text !== null">
              <p ref="clipTextEl" class="max-h-64 overflow-y-auto break-words whitespace-pre-wrap text-[17px] text-ink select-text">{{ clip.text }}</p>
              <button class="btn-primary mt-4 w-full min-h-14 text-[18px]" @click="copyClip">
                <Check v-if="clipCopy === 'copied'" :size="20" />
                <Copy v-else :size="20" />
                {{ clipCopy === "copied" ? "Copied" : "Copy" }}
              </button>
              <p v-if="clipCopy === 'select'" class="mt-3 text-[14px] text-body" role="status">
                Your browser didn't let tug copy it on this page. The text is selected: press and hold it, then tap Copy.
              </p>
            </template>
            <template v-else-if="clip.image">
              <template v-if="clip.phase === 'ready' && clip.url">
                <!-- Pictures can't be copied for you from a page like this: press and hold it instead. -->
                <img :src="clip.url" alt="The picture from your PC's clipboard" class="mx-auto block max-h-[60vh] w-auto max-w-full rounded-lg" />
                <p class="mt-3 text-[14px] text-body">Press and hold the picture to copy it or save it to Photos.</p>
                <a :href="clip.url" :download="clip.image.name" class="btn-secondary btn-sm mt-3">
                  <Download :size="16" />
                  Save
                </a>
              </template>
              <template v-else-if="clip.phase === 'failed'">
                <p class="text-[15px] text-error">{{ clip.error }}</p>
                <button class="btn-secondary btn-sm mt-3" @click="fetchClipImage">Try again</button>
              </template>
              <template v-else>
                <p class="flex items-center gap-2 text-[15px] text-muted" role="status">
                  <LoaderCircle :size="16" class="animate-spin" />
                  Getting the picture… {{ formatSize(clip.got) }} of {{ formatSize(clip.image.size) }}
                </p>
              </template>
            </template>
          </div>
        </section>

        <!-- Phone → PC -->
        <section class="mt-6" aria-labelledby="send-title">
          <h2 id="send-title" class="headline text-[22px]">Send to your PC</h2>
          <div class="mt-3 grid grid-cols-2 gap-3">
            <!-- The inputs cover their buttons: iOS ignores taps forwarded to hidden file inputs. -->
            <label class="btn-primary relative overflow-hidden px-3 whitespace-nowrap">
              <ImageIcon :size="20" />
              Photos &amp; videos
              <input type="file" multiple accept="image/*,video/*" class="absolute inset-0 cursor-pointer opacity-0" aria-label="Photos and videos" @change="pick" />
            </label>
            <label class="btn-secondary relative overflow-hidden px-3 whitespace-nowrap">
              <FileUp :size="20" />
              Files
              <input type="file" multiple class="absolute inset-0 cursor-pointer opacity-0" aria-label="Files" @change="pick" />
            </label>
          </div>
          <p class="mt-2.5 text-[14px] text-muted">Keep this page open while sending.</p>

          <ul v-if="uploads.length" class="mt-3 space-y-2">
            <li v-for="u in uploads" :key="u.key" class="card flex items-center gap-3 py-3">
              <div class="min-w-0 flex-1">
                <p class="truncate text-[15px] text-ink">{{ u.savedAs ?? u.file.name }}</p>
                <p class="mt-0.5 text-[13px]" :class="u.state === 'failed' ? 'text-error' : 'text-muted'">
                  <template v-if="u.state === 'saved'">Saved on your PC · {{ formatSize(u.file.size) }}</template>
                  <template v-else-if="u.state === 'failed'">{{ u.error }}</template>
                  <template v-else-if="u.state === 'waiting'">Waiting · {{ formatSize(u.file.size) }}</template>
                  <template v-else>{{ formatSize(u.sent) }} of {{ formatSize(u.file.size) }}</template>
                </p>
                <div v-if="u.state === 'sending'" class="mt-2 h-1.5 overflow-hidden rounded-full bg-track">
                  <div class="h-full rounded-full bg-primary transition-[width] duration-300" :style="{ width: `${percent(u)}%` }" />
                </div>
              </div>
              <Check v-if="u.state === 'saved'" :size="20" class="shrink-0 text-success" aria-label="Saved" />
              <button v-if="u.state === 'failed' && u.retry" class="btn-secondary btn-sm shrink-0" @click="retry(u)">Try again</button>
              <button
                v-if="u.state !== 'saved'"
                class="-mr-1 shrink-0 rounded-lg p-2 text-muted active:bg-card-strong"
                :aria-label="u.state === 'failed' ? 'Dismiss' : 'Stop sending'"
                @click="cancel(u)"
              >
                <X :size="18" />
              </button>
            </li>
          </ul>
          <p v-if="sending" class="sr-only" role="status">Sending</p>
        </section>

        <!-- Phone → PC: text onto the PC's clipboard -->
        <section class="mt-8" aria-labelledby="paste-title">
          <h2 id="paste-title" class="headline text-[22px]">Paste to PC</h2>
          <p class="mt-1 text-[14px] text-muted">Press and hold in the box, tap Paste, then Send. It goes on your PC's clipboard.</p>
          <textarea
            v-model="draft"
            rows="3"
            class="mt-3 block w-full resize-y rounded-xl border border-hairline bg-canvas px-3.5 py-3 text-[16px] text-ink outline-none placeholder:text-muted focus:border-primary"
            placeholder="Paste here"
            aria-label="Text to put on your PC's clipboard"
            @input="textState = 'idle'"
          />
          <div class="mt-2.5 flex items-center gap-3">
            <button class="btn-primary btn-sm" :disabled="!draft.trim() || textState === 'sending'" @click="sendText">
              <LoaderCircle v-if="textState === 'sending'" :size="16" class="animate-spin" />
              <Send v-else :size="16" />
              Send
            </button>
            <span v-if="textState === 'sent'" class="text-[14px] text-muted" role="status">Sent to your PC.</span>
            <span v-else-if="textState === 'failed'" class="text-[14px] text-error" role="alert">Couldn't send. Try again.</span>
          </div>
        </section>

        <!-- PC → phone -->
        <section class="mt-8" aria-labelledby="receive-title">
          <h2 id="receive-title" class="headline text-[22px]">From your PC</h2>
          <div v-if="pcText" class="card mt-3">
            <p class="text-[13px] font-medium text-muted">Text from your PC</p>
            <p ref="pcTextEl" class="mt-1 max-h-48 overflow-y-auto break-words whitespace-pre-wrap text-[16px] text-ink select-text">{{ pcText.text }}</p>
            <div class="mt-3 flex items-center gap-3">
              <button class="btn-secondary btn-sm shrink-0" @click="copyPcText">
                <Check v-if="copied" :size="16" class="text-success" />
                <Copy v-else :size="16" />
                {{ copied ? "Copied" : "Copy" }}
              </button>
              <span v-if="copyFailed" class="text-[13px] text-muted">It's selected: press and hold the text, then tap Copy.</span>
            </div>
          </div>
          <p v-if="!offers.length && !pcText" class="mt-2 text-[15px] text-muted">Drag files onto tug on your PC and they'll show up here.</p>
          <ul v-else class="mt-3 space-y-2">
            <li v-for="o in offers" :key="o.id" class="card flex items-center gap-3 py-3">
              <div class="min-w-0 flex-1">
                <p class="truncate text-[15px] text-ink">{{ o.name }}</p>
                <p class="mt-0.5 text-[13px]" :class="downloads[o.id]?.state === 'failed' ? 'text-error' : 'text-muted'">
                  <template v-if="o.size > MAX_DOWNLOAD">Over 1 GB, too big for a phone browser.</template>
                  <template v-else-if="downloads[o.id]?.state === 'failed'">{{ downloads[o.id].error }}</template>
                  <template v-else-if="downloads[o.id]?.state === 'loading'">
                    {{ formatSize(downloads[o.id].got) }} of {{ formatSize(o.size) }}
                  </template>
                  <template v-else>{{ formatSize(o.size) }}</template>
                </p>
                <div v-if="downloads[o.id]?.state === 'loading'" class="mt-2 h-1.5 overflow-hidden rounded-full bg-track">
                  <div
                    class="h-full rounded-full bg-primary transition-[width] duration-300"
                    :style="{ width: `${o.size ? Math.round((downloads[o.id].got / o.size) * 100) : 100}%` }"
                  />
                </div>
              </div>
              <template v-if="o.size <= MAX_DOWNLOAD">
                <!-- Saving needs its own tap once the file is ready (Safari wants a gesture). -->
                <a
                  v-if="downloads[o.id]?.state === 'ready'"
                  :href="downloads[o.id].url"
                  :download="o.name"
                  class="btn-primary btn-sm shrink-0"
                >
                  <Download :size="16" />
                  Save
                </a>
                <span v-else-if="downloads[o.id]?.state === 'loading'" class="shrink-0 p-2 text-muted">
                  <LoaderCircle :size="18" class="animate-spin" />
                </span>
                <button v-else class="btn-secondary btn-sm shrink-0" @click="fetchOffer(o)">
                  {{ downloads[o.id]?.state === "failed" ? "Try again" : "Get" }}
                </button>
              </template>
            </li>
          </ul>
        </section>
      </template>
    </template>

    <footer class="mt-auto pt-10 text-center text-[13px] text-muted">tug</footer>
  </div>
</template>
