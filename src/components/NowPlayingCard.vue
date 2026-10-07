<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { FastForward, Heart, ListMusic, Music2, Pause, Play, Repeat, Repeat1, Rewind, RotateCcw, Shuffle, SkipBack, SkipForward, ThumbsDown, ThumbsUp, Volume1, Volume2 } from "lucide-vue-next";
import { useTugStore } from "../stores/tug";
import { duration } from "../lib/format";
import { canRestart, createHoldRepeater, repeatLabel, SKIP_SECONDS, skipLabel, skipMode, skipTargetMs, supportsDislike, supportsLike } from "../lib/media";
import { seekFraction } from "../lib/spotify";

// `compact` (from the sidebar, on short windows) drops the card's second row of extra controls and
// tucks them into rows that are there anyway, so the card keeps its one-row height and still fits
// without scrolling (see showExtraRow).
const props = defineProps<{ compact?: boolean }>();

const tug = useTugStore();
const np = computed(() => tug.nowPlaying);
const available = computed(() => tug.status.services.media && np.value.title != null);
const playing = computed(() => np.value.state === "playing");
// The song as it should read: on Spotify Connect the phone sends "Listening on <device>" as the
// artist, which is never shown as one (see lib/playback). The device goes in the header instead,
// under the same name the "Play on" picker uses.
const track = computed(() => tug.trackView);
const artistLine = computed(() => {
  const line = [track.value.artist, track.value.album].filter(Boolean).join(" — ");
  return line || (track.value.hint ? null : "Unknown artist");
});
const heading = computed(() =>
  available.value ? [np.value.player ?? "Now playing", tug.playingOn].filter(Boolean).join(" ") : "Now playing",
);

// Spotify augmentation: only when connected and Spotify is the AMS player (see the store).
const sp = computed(() => (tug.spotifyActive ? tug.spotifyPlayer : null));
// Art and Like belong to one song: only show them once Spotify reports the song the phone is
// playing (right after a skip it can still describe the previous one).
const sameSong = computed(() => !!sp.value && tug.spotifyTrackVerified);
const art = computed(() => (sameSong.value ? sp.value?.albumArt ?? null : null));

// Click/drag-to-seek: only when Spotify is the player and we know the song length. Otherwise the
// bar is a plain progress indicator, exactly as before.
const seekable = computed(() => !!sp.value && tug.connected && (np.value.duration ?? 0) > 0);
const bar = ref<HTMLElement | null>(null);
/** The fraction a drag is currently at, so the fill follows the pointer before it commits. */
const dragFraction = ref<number | null>(null);
function fractionAt(e: PointerEvent): number {
  const rect = bar.value?.getBoundingClientRect();
  return rect ? seekFraction(e.clientX, rect) : 0;
}
function seekDown(e: PointerEvent) {
  if (!seekable.value) return;
  (e.target as HTMLElement).setPointerCapture?.(e.pointerId);
  dragFraction.value = fractionAt(e);
}
function seekMove(e: PointerEvent) {
  if (dragFraction.value == null) return;
  dragFraction.value = fractionAt(e);
}
function seekUp() {
  if (dragFraction.value == null) return;
  const f = dragFraction.value;
  dragFraction.value = null;
  if (np.value.duration != null) void tug.spotifySeek(f * np.value.duration * 1000);
}

// AMS only reports elapsed time on state changes; advance it locally while playing.
const now = ref(Date.now());
const timer = window.setInterval(() => (now.value = Date.now()), 1000);

// Press-and-hold volume: one AMS step on press, then a steady repeat. The timing lives in
// lib/media.ts (pure, unit-tested); this just wires DOM events to it. Volume is a 0–1 fraction,
// so stop at the end in each direction to avoid spamming the phone at the limit.
const timers = { set: (fn: () => void, ms: number) => window.setTimeout(fn, ms), clear: (h: number) => window.clearTimeout(h) };
const holdUp = createHoldRepeater(timers);
const holdDown = createHoldRepeater(timers);
const atMax = () => (np.value.volume ?? 0) >= 1;
const atMin = () => (np.value.volume ?? 1) <= 0;
const pressUp = () => holdUp.start(() => tug.media("volumeUp"), atMax);
const pressDown = () => holdDown.start(() => tug.media("volumeDown"), atMin);

// Space/Enter held on a focused button: ignore the OS key-repeat and let our own timer set the
// pace; preventDefault stops the synthetic click (and Space scrolling the page).
function holdKey(e: KeyboardEvent, press: () => void) {
  if (e.key !== " " && e.key !== "Enter" || e.repeat) return;
  e.preventDefault();
  press();
}

function stopHolds() {
  holdUp.stop();
  holdDown.stop();
}
// Leaving the tab or window (alt-tab, minimise) should end any hold even without a pointer/key up.
function onVisibility() {
  if (document.visibilityState !== "visible") stopHolds();
}
onMounted(() => {
  window.addEventListener("blur", stopHolds);
  document.addEventListener("visibilitychange", onVisibility);
});
onUnmounted(() => {
  window.clearInterval(timer);
  window.removeEventListener("blur", stopHolds);
  document.removeEventListener("visibilitychange", onVisibility);
  stopHolds();
});

const elapsed = computed(() => {
  const base = np.value.elapsed;
  if (base == null) return null;
  // Advance from when the phone reported the position, not from the last update of
  // any kind (a volume change must not rewind the bar).
  const reportedAt = np.value.elapsedAt ?? now.value;
  const drift = playing.value ? (Math.max(0, now.value - reportedAt) / 1000) * (np.value.rate ?? 1) : 0;
  return np.value.duration != null ? Math.min(base + drift, np.value.duration) : base + drift;
});
const progress = computed(() =>
  dragFraction.value != null
    ? dragFraction.value * 100
    : elapsed.value != null && np.value.duration
      ? (elapsed.value / np.value.duration) * 100
      : 0,
);
const can = (c: string) => np.value.available.length === 0 || np.value.available.includes(c as never);

// Extra AMS controls, shown only when the current player lists them (see lib/media). Skip ±15 s is
// offered by Apple Music and Spotify; Like/Dislike by Apple Music. Spotify keeps its own Web API
// Like (the heart next to the title), so AMS Like is suppressed while Spotify is the active player.
const backMode = computed(() => skipMode(np.value, "back", seekable.value));
const forwardMode = computed(() => skipMode(np.value, "forward", seekable.value));
const skipBack = computed(() => backMode.value !== "none");
const backLabel = computed(() => skipLabel(backMode.value, "back"));
const forwardLabel = computed(() => skipLabel(forwardMode.value, "forward"));
const skipForward = computed(() => forwardMode.value !== "none");
// Spotify jumps to the exact time through the Spotify connection (its own skip command restarts
// the song); other players get the phone's skip command.
function skip(direction: "back" | "forward") {
  const mode = direction === "back" ? backMode.value : forwardMode.value;
  if (mode === "seek") {
    // No position from the phone yet: don't guess (Forward would land at 0:15).
    if (elapsed.value == null) return;
    const delta = direction === "back" ? -SKIP_SECONDS : SKIP_SECONDS;
    void tug.spotifySeek(skipTargetMs(elapsed.value, delta, np.value.duration ?? null));
  } else if (mode === "ams") {
    void tug.media(direction === "back" ? "skipBackward" : "skipForward");
  }
}
const amsLike = computed(() => !tug.spotifyActive && supportsLike(np.value));
const amsDislike = computed(() => !tug.spotifyActive && supportsDislike(np.value));
const hasExtraControls = computed(() => skipBack.value || skipForward.value || amsLike.value || amsDislike.value);
// A standalone second row only when there's room (not compact). When compact the card can't grow (at
// the 600 px minimum window height even the one-row card only just fits) and the transport row can't
// widen (for Spotify it already spans the card), so the extras go where they add neither height nor
// transport-row width: Like/Dislike beside the title (where Spotify's heart sits) and skip ±15 s
// between the elapsed/total times, with a negative margin so the time row keeps its height.
const showExtraRow = computed(() => hasExtraControls.value && !props.compact);
const compactRating = computed(() => !!props.compact && (amsLike.value || amsDislike.value));
const compactSkip = computed(() => !!props.compact && (skipBack.value || skipForward.value));

// Start the song over with Back, only where Back restarts rather than skips (see canRestart).
function restart() {
  if (canRestart(elapsed.value)) void tug.media("previousTrack");
  else tug.notify("info", "Already at the start of the song");
}

// No loop button: on the test iPhone Spotify offers no remote commands for repeat and Apple Music
// accepts AdvanceRepeatMode but never changes mode (checked in the log, 2026-10-05). tug still
// reads and logs the repeat mode (see lib/media.ts), so a button can return for a player that
// honours it.
</script>

<template>
  <section class="rounded-xl bg-surface-dark-elevated p-4">
    <div class="caption-upper mb-2.5 flex items-center gap-2 text-on-dark-soft">
      <Music2 :size="13" />
      <span class="min-w-0 flex-1 truncate">{{ heading }}</span>
      <template v-if="available">
        <button
          class="rounded-full p-1.5 normal-case text-on-dark-soft active:text-on-dark"
          :disabled="!can('previousTrack')"
          aria-label="Restart song"
          title="Restart song"
          @click="restart"
        >
          <RotateCcw :size="15" />
        </button>
      </template>
      <button
        v-if="tug.spotify.connected"
        class="rounded-full p-1.5 normal-case text-on-dark-soft active:text-on-dark"
        aria-label="Open Spotify"
        title="Search and play on Spotify"
        @click="tug.openSpotifyPanel('search')"
      >
        <ListMusic :size="15" />
      </button>
    </div>

    <template v-if="available">
      <div class="flex items-center gap-3">
        <img v-if="art" :src="art" alt="" class="size-11 shrink-0 rounded-md object-cover" />
        <div class="min-w-0 flex-1">
          <p class="truncate font-display text-[22px] leading-tight text-on-dark" style="letter-spacing: -0.01em">
            {{ track.title }}
          </p>
          <p v-if="artistLine" class="mt-0.5 truncate text-[13px] text-on-dark-soft">
            {{ artistLine }}
          </p>
        </div>
        <!-- Like the current song: Spotify only, sat next to the title. -->
        <button
          v-if="sp && sameSong"
          class="shrink-0 rounded-full p-1.5 text-on-dark active:bg-surface-dark-soft"
          :aria-pressed="sp.saved === true"
          aria-label="Like song"
          title="Save to your Liked Songs"
          @click="tug.toggleSpotifyLike()"
        >
          <Heart :size="17" :fill="sp.saved ? 'currentColor' : 'none'" />
        </button>
        <!-- Compact only: Apple Music Like/Dislike sit here instead of on a second row. -->
        <div v-if="compactRating" class="flex shrink-0 items-center gap-0.5">
          <button
            v-if="amsDislike"
            class="rounded-full p-1.5 text-on-dark-soft active:bg-surface-dark-soft active:text-on-dark"
            aria-label="Dislike song"
            title="Dislike this song"
            @click="tug.media('dislikeTrack')"
          >
            <ThumbsDown :size="16" />
          </button>
          <button
            v-if="amsLike"
            class="rounded-full p-1.5 text-on-dark-soft active:bg-surface-dark-soft active:text-on-dark"
            aria-label="Like song"
            title="Like this song"
            @click="tug.media('likeTrack')"
          >
            <ThumbsUp :size="16" />
          </button>
        </div>
      </div>

      <!-- When Spotify is the player, click or drag the bar to seek; otherwise it just shows
           progress. Its box is unchanged either way so the compact card keeps its height. -->
      <div
        ref="bar"
        :class="['mt-3 h-1 overflow-hidden rounded-full bg-surface-dark-soft', seekable ? 'cursor-pointer' : '']"
        :role="seekable ? 'slider' : undefined"
        :aria-label="seekable ? 'Seek' : undefined"
        :aria-valuenow="seekable ? Math.round(progress) : undefined"
        aria-valuemin="0"
        aria-valuemax="100"
        @pointerdown="seekDown"
        @pointermove="seekMove"
        @pointerup="seekUp"
        @pointercancel="seekUp"
      >
        <!-- Keyed by track so a new song starts at its position instead of sliding back. -->
        <div
          :key="np.title ?? ''"
          :class="['h-full rounded-full bg-on-dark ease-linear', dragFraction == null ? 'transition-[width] duration-1000' : '']"
          :style="{ width: `${progress}%` }"
        />
      </div>
      <div class="mt-1.5 flex items-center justify-between font-mono text-[11px] text-on-dark-soft">
        <span>{{ duration(elapsed) }}</span>
        <!-- Compact only: skip ±15 s between the times. The negative margin keeps the full-size
             hit area without making this row any taller than the times alone. -->
        <div v-if="compactSkip" class="-my-1.5 flex items-center gap-1">
          <button
            v-if="skipBack"
            class="flex items-center gap-0.5 rounded-full px-1.5 py-1.5 text-on-dark-soft active:bg-surface-dark-soft active:text-on-dark"
            :aria-label="backLabel.text"
            :title="backLabel.text"
            @click="skip('back')"
          >
            <Rewind :size="15" />
            <span v-if="backLabel.seconds" class="text-[10px]">{{ backLabel.seconds }}</span>
          </button>
          <button
            v-if="skipForward"
            class="flex items-center gap-0.5 rounded-full px-1.5 py-1.5 text-on-dark-soft active:bg-surface-dark-soft active:text-on-dark"
            :aria-label="forwardLabel.text"
            :title="forwardLabel.text"
            @click="skip('forward')"
          >
            <span v-if="forwardLabel.seconds" class="text-[10px]">{{ forwardLabel.seconds }}</span>
            <FastForward :size="15" />
          </button>
        </div>
        <span>{{ duration(np.duration) }}</span>
      </div>

      <!-- One row: playback modes bookend it (Spotify only), volume just inside, transport centred.
           Nothing else goes here: for Spotify it already spans the card's full width. -->
      <div class="mt-3 flex items-center justify-between">
        <div class="flex items-center gap-0.5">
          <button
            v-if="sp"
            class="rounded-full p-1.5 active:bg-surface-dark-soft"
            :class="sp.shuffle ? 'text-on-dark' : 'text-on-dark-soft/50'"
            :aria-pressed="sp.shuffle"
            aria-label="Shuffle"
            :title="sp.shuffle ? 'Shuffle on' : 'Shuffle off'"
            @click="tug.toggleSpotifyShuffle()"
          >
            <Shuffle :size="16" />
          </button>
          <button
            class="rounded-full p-1.5 text-on-dark-soft active:text-on-dark"
            :disabled="!can('volumeDown')"
            aria-label="Volume down"
            title="Hold to keep changing"
            @pointerdown="pressDown"
            @pointerup="holdDown.stop"
            @pointercancel="holdDown.stop"
            @pointerleave="holdDown.stop"
            @blur="holdDown.stop"
            @keydown="holdKey($event, pressDown)"
            @keyup="holdDown.stop"
          >
            <Volume1 :size="17" />
          </button>
        </div>
        <div class="flex items-center gap-0.5">
          <button class="rounded-full p-1.5 text-on-dark active:bg-surface-dark-soft" :disabled="!can('previousTrack')" aria-label="Previous" @click="tug.media('previousTrack')">
            <SkipBack :size="17" />
          </button>
          <button
            class="flex size-10 items-center justify-center rounded-full bg-on-dark text-surface-dark active:bg-on-dark-soft"
            :aria-label="playing ? 'Pause' : 'Play'"
            @click="tug.media('togglePlayPause')"
          >
            <Pause v-if="playing" :size="17" fill="currentColor" />
            <Play v-else :size="17" fill="currentColor" class="translate-x-px" />
          </button>
          <button class="rounded-full p-1.5 text-on-dark active:bg-surface-dark-soft" :disabled="!can('nextTrack')" aria-label="Next" @click="tug.media('nextTrack')">
            <SkipForward :size="17" />
          </button>
        </div>
        <div class="flex items-center gap-0.5">
          <button
            class="rounded-full p-1.5 text-on-dark-soft active:text-on-dark"
            :disabled="!can('volumeUp')"
            aria-label="Volume up"
            title="Hold to keep changing"
            @pointerdown="pressUp"
            @pointerup="holdUp.stop"
            @pointercancel="holdUp.stop"
            @pointerleave="holdUp.stop"
            @blur="holdUp.stop"
            @keydown="holdKey($event, pressUp)"
            @keyup="holdUp.stop"
          >
            <Volume2 :size="17" />
          </button>
          <button
            v-if="sp"
            class="rounded-full p-1.5 active:bg-surface-dark-soft"
            :class="sp.repeat && sp.repeat !== 'off' ? 'text-on-dark' : 'text-on-dark-soft/50'"
            aria-label="Repeat"
            :title="repeatLabel(sp.repeat)"
            @click="tug.cycleSpotifyRepeat()"
          >
            <Repeat1 v-if="sp.repeat === 'one'" :size="16" />
            <Repeat v-else :size="16" />
          </button>
        </div>
      </div>

      <!-- Extra controls the player supports over AMS: skip ±15 s and (Apple Music) Like/Dislike.
           A second row shown only when there's room (see showExtraRow); on a short window they move
           beside the title and between the times instead, so the sidebar never has to clip or scroll. -->
      <div v-if="showExtraRow" class="mt-2 flex items-center justify-center gap-1">
        <button
          v-if="amsDislike"
          class="rounded-full p-1.5 text-on-dark-soft active:bg-surface-dark-soft active:text-on-dark"
          aria-label="Dislike song"
          title="Dislike this song"
          @click="tug.media('dislikeTrack')"
        >
          <ThumbsDown :size="16" />
        </button>
        <button
          v-if="skipBack"
          class="flex items-center gap-0.5 rounded-full px-2 py-1.5 text-on-dark-soft active:bg-surface-dark-soft active:text-on-dark"
          :aria-label="backLabel.text"
          :title="backLabel.text"
          @click="skip('back')"
        >
          <Rewind :size="15" />
          <span v-if="backLabel.seconds" class="font-mono text-[10px]">{{ backLabel.seconds }}</span>
        </button>
        <button
          v-if="skipForward"
          class="flex items-center gap-0.5 rounded-full px-2 py-1.5 text-on-dark-soft active:bg-surface-dark-soft active:text-on-dark"
          :aria-label="forwardLabel.text"
          :title="forwardLabel.text"
          @click="skip('forward')"
        >
          <span v-if="forwardLabel.seconds" class="font-mono text-[10px]">{{ forwardLabel.seconds }}</span>
          <FastForward :size="15" />
        </button>
        <button
          v-if="amsLike"
          class="rounded-full p-1.5 text-on-dark-soft active:bg-surface-dark-soft active:text-on-dark"
          aria-label="Like song"
          title="Like this song"
          @click="tug.media('likeTrack')"
        >
          <ThumbsUp :size="16" />
        </button>
      </div>
    </template>

    <p v-else class="text-[13px] text-on-dark-soft">
      {{ tug.connected ? "Nothing playing on your iPhone." : "Media controls appear once your iPhone is connected." }}
    </p>
  </section>
</template>
