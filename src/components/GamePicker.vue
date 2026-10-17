<script setup lang="ts">
// The games picker: tug's little games side by side, from the sidebar's Play button, Ctrl+K "game"
// or the taskbar's Play a game. Each card says what the game is and where you stand today (your
// best, a daily streak). Click a card, press 1 or 2, or Tab to it and press Enter. Esc closes.
import { computed, onMounted, ref } from "vue";
import { Container, Flame, Gamepad2, Ship, X } from "lucide-vue-next";
import { useGameStore } from "../stores/game";
import { useFocusTrap } from "../lib/focusTrap";
import { currentStreak, localDate } from "../lib/tugboatDaily";
import type { GameId } from "../types/protocol";

const game = useGameStore();
const root = ref<HTMLElement | null>(null);
const cardButtons = ref<HTMLButtonElement[]>([]);
useFocusTrap(root, () => game.close());

const today = localDate(new Date());

const cards = computed(() => [
  {
    id: "tugboat-run" as GameId,
    key: "1",
    name: "Tugboat Run",
    icon: Ship,
    line: "Steer through the harbor, hop the buoys, collect coins.",
    best: game.scores["tugboat-run"].best,
    streak: currentStreak(game.scores["tugboat-run"].daily, today),
  },
  {
    id: "harbor-stack" as GameId,
    key: "2",
    name: "Harbor Stack",
    icon: Container,
    line: "Drop containers from a swinging crane. Line them up; the overhang falls in.",
    best: game.scores["harbor-stack"].best,
    streak: currentStreak(game.scores["harbor-stack"].daily, today),
  },
]);

function play(id: GameId) {
  void game.show(id);
}

function onKey(e: KeyboardEvent) {
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  const card = cards.value.find((c) => c.key === e.key);
  if (card) {
    e.preventDefault();
    play(card.id);
  }
}

onMounted(() => cardButtons.value[0]?.focus());
</script>

<template>
  <div
    ref="root"
    role="dialog"
    aria-modal="true"
    aria-labelledby="games-title"
    class="fixed inset-0 z-50 flex flex-col bg-surface-dark text-on-dark"
    @keydown="onKey"
  >
    <header class="flex items-center gap-3 border-b border-surface-dark-elevated px-5 py-3">
      <Gamepad2 :size="20" class="shrink-0 text-primary" />
      <h2 id="games-title" class="font-display text-[24px] leading-none" style="letter-spacing: -0.02em">Games</h2>
      <button class="btn-on-dark btn-sm ml-auto w-8 px-0" aria-label="Close games" @click="game.close()">
        <X :size="15" />
      </button>
    </header>
    <div class="flex min-h-0 flex-1 overflow-y-auto p-6">
      <div class="m-auto w-full max-w-[720px]">
        <p class="font-display text-[30px] leading-tight">What shall we play?</p>
        <p class="mt-1 text-[14px] text-on-dark-soft">Both play with the keyboard, the mouse, or your phone as a controller.</p>
        <ul class="mt-5 grid gap-3 sm:grid-cols-2">
          <li v-for="c in cards" :key="c.id">
            <button
              ref="cardButtons"
              class="group flex h-full w-full flex-col items-start gap-2 rounded-2xl bg-surface-dark-elevated p-5 text-left ring-1 ring-transparent transition-colors outline-none hover:ring-on-dark-soft/30 focus-visible:ring-2 focus-visible:ring-primary"
              @click="play(c.id)"
            >
              <span class="flex w-full items-center gap-2.5">
                <span class="flex size-10 items-center justify-center rounded-xl bg-surface-dark">
                  <component :is="c.icon" :size="22" class="text-primary" />
                </span>
                <span class="font-display text-[24px] leading-none">{{ c.name }}</span>
                <kbd class="ml-auto rounded border border-on-dark-soft/30 px-1.5 font-mono text-[11px] text-on-dark-soft">{{ c.key }}</kbd>
              </span>
              <span class="text-[14px] text-on-dark-soft">{{ c.line }}</span>
              <span class="mt-auto flex items-center gap-3 pt-1 text-[12px]">
                <span class="text-on-dark-soft">{{ c.best > 0 ? `Best ${c.best}` : "Not played yet" }}</span>
                <span v-if="c.streak > 0" class="flex items-center gap-1 text-accent-amber">
                  <Flame :size="13" />
                  {{ c.streak === 1 ? "1 day streak" : `${c.streak} day streak` }}
                </span>
              </span>
            </button>
          </li>
        </ul>
        <p class="mt-4 text-[12px] text-on-dark-soft/80">Scores stay on this PC.</p>
      </div>
    </div>
  </div>
</template>
