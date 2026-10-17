<script setup lang="ts">
// A photo (or other attachment) the iPhone won't send over Bluetooth, or a text whose words didn't
// come through: an icon, what it is, and where to see it. Inherits the text colour of what it sits
// in (a bubble, a Feed row), so it reads on any background.
import { computed, type Component } from "vue";
import { AudioLines, Camera, Contact, FileText, ImagePlay, MapPin, MessageSquareWarning, Paperclip, Sticker, Video } from "lucide-vue-next";
import { mediaLabel, OPEN_ON_IPHONE, UNREADABLE, type Media, type MediaKind } from "../lib/attachments";

const props = defineProps<{ media?: Media | null; unreadable?: boolean; inline?: boolean }>();

const ICONS: Record<MediaKind, Component> = {
  photo: Camera,
  video: Video,
  sticker: Sticker,
  audio: AudioLines,
  gif: ImagePlay,
  file: FileText,
  contact: Contact,
  location: MapPin,
  attachment: Paperclip,
};
const icon = computed(() => (props.unreadable || !props.media ? MessageSquareWarning : ICONS[props.media.kind]));
const label = computed(() => (props.unreadable || !props.media ? UNREADABLE : mediaLabel(props.media)));
</script>

<template>
  <!-- One line for a Feed row: icon, what it is · where to see it. -->
  <span v-if="inline" class="inline-flex min-w-0 items-center gap-1.5 align-bottom">
    <component :is="icon" :size="13" class="shrink-0" aria-hidden="true" />
    <span class="truncate">{{ label }}<span class="opacity-70"> · {{ OPEN_ON_IPHONE }}</span></span>
  </span>
  <span v-else class="flex items-center gap-2.5 py-0.5" role="note">
    <component :is="icon" :size="18" class="shrink-0 opacity-80" aria-hidden="true" />
    <span class="flex min-w-0 flex-col leading-tight">
      <span class="font-medium">{{ label }}</span>
      <span class="text-[12px] opacity-70">{{ OPEN_ON_IPHONE }}</span>
    </span>
  </span>
</template>
