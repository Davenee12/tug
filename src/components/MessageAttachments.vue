<script setup lang="ts">
// A text's photos and videos from an iPhone backup: photos as real pictures (a JPEG preview made
// on this PC; click opens the photo in Photos), videos and HEIC photos Windows can't show yet as a
// chip that opens them, and anything not copied as the usual chip.
import { computed, onMounted, ref } from "vue";
import { Film, Image } from "lucide-vue-next";
import { useBackupStore } from "../stores/backup";
import MediaChip from "./MediaChip.vue";
import type { MediaKind } from "../lib/attachments";
import type { MessageAttachment } from "../types/protocol";

const props = defineProps<{ attachments: MessageAttachment[] }>();
const backup = useBackupStore();

const PICTURES = new Set(["photo", "gif", "sticker"]);
const pictures = computed(() => props.attachments.filter((a) => a.stored && PICTURES.has(a.kind)));
const videos = computed(() => props.attachments.filter((a) => a.stored && a.kind === "video"));
// Not on this PC (skipped, or a kind tug doesn't copy): one chip per kind, like Bluetooth's.
const chips = computed(() => {
  const counts = new Map<MediaKind, number>();
  for (const a of props.attachments) {
    if (a.stored) continue;
    const kind: MediaKind = a.kind === "file" ? "file" : (a.kind as MediaKind);
    counts.set(kind, (counts.get(kind) ?? 0) + 1);
  }
  return [...counts].map(([kind, count]) => ({ kind, count }));
});

const previews = ref<Record<number, string | null | undefined>>({});
onMounted(() => {
  for (const a of pictures.value) {
    void backup.preview(a.id).then((p) => (previews.value = { ...previews.value, [a.id]: p.dataUri }));
  }
});
</script>

<template>
  <div class="flex flex-col gap-2">
    <div v-if="pictures.length" class="flex flex-wrap gap-1.5">
      <template v-for="a in pictures" :key="a.id">
        <button
          v-if="previews[a.id]"
          class="overflow-hidden rounded-lg"
          title="Open in Photos"
          @click="backup.openAttachment(a.id)"
        >
          <img :src="previews[a.id] ?? undefined" alt="Photo" class="block max-h-64 max-w-[280px] object-cover" />
        </button>
        <button
          v-else-if="previews[a.id] === null"
          class="flex items-center gap-2 rounded-lg border border-current/20 px-3 py-2 text-[13px]"
          @click="backup.openAttachment(a.id)"
        >
          <Image :size="16" class="shrink-0 opacity-80" /> Open photo
        </button>
        <span v-else class="block h-32 w-40 rounded-lg bg-current/10" aria-label="Loading photo" />
      </template>
    </div>
    <button
      v-for="a in videos"
      :key="a.id"
      class="flex items-center gap-2 self-start rounded-lg border border-current/20 px-3 py-2 text-[13px]"
      title="Open in your video app"
      @click="backup.openAttachment(a.id)"
    >
      <Film :size="16" class="shrink-0 opacity-80" /> Open video
    </button>
    <MediaChip v-for="c in chips" :key="c.kind" :media="c" />
  </div>
</template>
