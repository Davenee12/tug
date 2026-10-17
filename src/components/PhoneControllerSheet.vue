<script setup lang="ts">
// "Use your phone as a controller", beside a game (Tugboat Run, Harbor Stack): Tugboat's code to
// scan, where things stand, and in a two-player game the Player 2 code for a friend's phone (which
// can only play, never reach photos, files or the clipboard). The game owns the channel
// (`useGameController`); this only shows it.
import { computed, useId } from "vue";
import { ArrowLeftRight, LoaderCircle, Play, Smartphone, X } from "lucide-vue-next";
import { useTugboatStore } from "../stores/tugboat";
import { capitalised } from "../lib/tugboat";
import { seatFor, type Player } from "../lib/tugboatRace";

const props = defineProps<{
  duo: boolean;
  controllerOn: boolean;
  controllerError: string | null;
  padConnected: readonly boolean[];
  /** The first phone plays Player 2 (so the keyboard can be Player 1). */
  swapped: boolean;
  /** What a connected phone does in a solo game: "Steer with the pad, Boost to hop." */
  soloHint: string;
  /** Show Swap phones (between games) and Resume (paused, nobody dropped). */
  canSwap: boolean;
  canResume: boolean;
  resumeDisabled: boolean;
}>();
const emit = defineEmits<{ close: []; swap: []; resume: []; off: [] }>();

const tugboat = useTugboatStore();
const ts = computed(() => tugboat.status);
const phoneName = computed(() => ts.value.phone ?? "phone");
const titleId = useId();
const p1Id = useId();
const p2Id = useId();
const PLAYER_NAME: Record<Player, string> = { 1: "Player 1", 2: "Player 2" };
const phoneFor = (p: Player) => props.padConnected[seatFor(p, props.swapped) - 1];
const anyPhone = computed(() => props.padConnected[0] || props.padConnected[1]);

const line = computed(() => {
  if (props.controllerError) return props.controllerError;
  if (props.duo) {
    const n = (props.padConnected[0] ? 1 : 0) + (props.padConnected[1] ? 1 : 0);
    if (n === 2) return "Both phones are connected. Each one shows its player's colour.";
    if (props.padConnected[0]) return "One phone connected. Scan the Player 2 code with the second phone.";
    if (props.padConnected[1]) return "One phone connected. Scan Tugboat's code with the other phone.";
  } else if (props.padConnected[0]) return `Your ${phoneName.value} is the controller. ${props.soloHint}`;
  const p = ts.value.phase;
  if (p === "off") return "Starting Tugboat…";
  if (p === "noNetwork") return "Connect this PC to your home or office Wi-Fi to use your phone.";
  if (props.duo) return "Player 1 scans Tugboat's code and Player 2 the Player 2 code, each with their phone's camera.";
  if (p === "connected" && ts.value.phoneActive) return `Your ${phoneName.value} is on Tugboat. The controller opens on it in a moment.`;
  if (p === "connected") return `Open Tugboat on your ${phoneName.value} again: scan the code if the page is closed.`;
  return "Scan with your phone's camera. Tugboat opens on it as a controller.";
});
const showCode = computed(() => !props.duo && ts.value.qr !== null && !props.padConnected[0]);
/** A two-player game's codes: Tugboat's own for the first phone, the Player 2 code for the second.
 * Each until its phone is in. */
const showRaceCode = computed(() => props.duo && ts.value.qr !== null && !props.padConnected[0]);
const showGuestCode = computed(() => props.duo && ts.value.guestQr !== null && !props.padConnected[1]);
</script>

<template>
  <aside class="flex w-[320px] shrink-0 flex-col overflow-y-auto border-l border-surface-dark-elevated bg-surface-dark-soft px-6 py-5" :aria-labelledby="titleId">
    <div class="flex items-center gap-2">
      <h3 :id="titleId" class="font-display text-[22px] leading-tight">{{ duo ? "Phone controllers" : "Phone controller" }}</h3>
      <button class="ml-auto rounded-md p-1.5 text-on-dark-soft active:bg-surface-dark-elevated" aria-label="Hide phone controller" @click="emit('close')">
        <X :size="16" />
      </button>
    </div>
    <p class="mt-3 flex items-start gap-2 text-[14px] text-on-dark" role="status">
      <LoaderCircle v-if="controllerOn && ts.phase === 'off' && !controllerError" :size="16" class="mt-0.5 shrink-0 animate-spin text-on-dark-soft" />
      <span v-else :class="['mt-[7px] size-2 shrink-0 rounded-full', anyPhone ? 'bg-accent-teal' : 'bg-on-dark-soft/40']" />
      {{ line }}
    </p>

    <div v-if="showCode && ts.qr" class="mx-auto mt-5 rounded-xl bg-canvas p-3">
      <svg :viewBox="`-2 -2 ${ts.qr.size + 4} ${ts.qr.size + 4}`" class="block size-[200px]" shape-rendering="crispEdges" role="img" aria-label="QR code to use your phone as a controller">
        <path :d="ts.qr.path" class="fill-ink" />
      </svg>
    </div>
    <p v-if="showCode && ts.qr" class="mt-3 text-center text-[12px] text-on-dark-soft">This is Tugboat's code: the same one for photos and files.</p>
    <div v-else-if="!duo && padConnected[0]" class="mt-5 flex flex-col items-center gap-2 py-4 text-center">
      <span class="flex size-14 items-center justify-center rounded-2xl bg-surface-dark-elevated">
        <Smartphone :size="26" class="text-accent-teal" />
      </span>
      <p class="font-display text-[20px]">{{ capitalised(phoneName) }} connected</p>
    </div>

    <template v-if="duo">
      <section v-if="showRaceCode && ts.qr" class="mt-5 text-center" :aria-labelledby="p1Id">
        <h4 :id="p1Id" class="text-[13px] text-on-dark">Player 1: Tugboat's code</h4>
        <div class="mx-auto mt-2 w-fit rounded-xl bg-canvas p-3">
          <svg :viewBox="`-2 -2 ${ts.qr.size + 4} ${ts.qr.size + 4}`" class="block size-[160px]" shape-rendering="crispEdges" role="img" aria-label="Tugboat's code, for Player 1's phone">
            <path :d="ts.qr.path" class="fill-ink" />
          </svg>
        </div>
        <p class="mt-2 text-[12px] text-on-dark-soft">The same code as for photos and files.</p>
      </section>
      <section v-if="showGuestCode && ts.guestQr" class="mt-5 text-center" :aria-labelledby="p2Id">
        <h4 :id="p2Id" class="text-[13px] text-on-dark">Player 2 code</h4>
        <div class="mx-auto mt-2 w-fit rounded-xl bg-canvas p-3">
          <svg :viewBox="`-2 -2 ${ts.guestQr.size + 4} ${ts.guestQr.size + 4}`" class="block size-[160px]" shape-rendering="crispEdges" role="img" aria-label="Player 2 code, for a second phone">
            <path :d="ts.guestQr.path" class="fill-ink" />
          </svg>
        </div>
        <p class="mt-2 text-[12px] text-on-dark-soft">For a friend's phone: it can only play, never see your photos, files or clipboard.</p>
        <p class="mt-1 font-mono text-[11px] break-all text-on-dark-soft/80 select-all">{{ ts.guestUrl }}</p>
      </section>
    </template>

    <ul v-if="duo" class="mt-5 space-y-2">
      <li v-for="p in [1, 2] as Player[]" :key="p" class="flex items-center gap-3 rounded-xl bg-surface-dark-elevated px-3 py-2">
        <span :class="['size-3 shrink-0 rounded-full', p === 1 ? 'bg-primary' : 'bg-accent-teal']" />
        <span class="flex-1 text-[14px]">{{ PLAYER_NAME[p] }}</span>
        <span class="text-[12px]" :class="phoneFor(p) ? 'text-accent-teal' : 'text-on-dark-soft'">{{ phoneFor(p) ? "Phone connected" : "Keyboard" }}</span>
      </li>
    </ul>
    <button v-if="canSwap" class="btn-on-dark btn-sm mt-2 self-center" @click="emit('swap')">
      <ArrowLeftRight :size="14" />
      Swap phones
    </button>

    <div class="mt-auto space-y-2 pt-5">
      <button v-if="canResume" class="btn-primary w-full" :disabled="resumeDisabled" @click="emit('resume')">
        <Play :size="16" />
        Resume
      </button>
      <button v-if="controllerOn" class="btn-on-dark btn-sm w-full" @click="emit('off')">{{ duo ? "Stop using phones" : "Stop using the phone" }}</button>
      <p class="text-center text-[12px] text-on-dark-soft/80">Encrypted, over your Wi-Fi. The keyboard always works too.</p>
    </div>
  </aside>
</template>
