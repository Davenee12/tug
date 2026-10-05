<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { avatarTone, initials } from "../lib/format";
import { useTugStore } from "../stores/tug";

// `person`: the avatar stands for someone (a conversation), not the app. They get their contact
// photo when the iPhone shared one, otherwise their initials — never the app's icon, so a friend
// never looks like a company. `photoKey` finds that photo (a number is more precise than a name);
// it defaults to the label, which the backend resolves as a name or number either way.
const props = defineProps<{ appId: string; label: string; size?: "sm" | "md"; person?: boolean; photoKey?: string }>();
const tug = useTugStore();
const tone = computed(() => avatarTone(props.appId));
const broken = ref(false);
watch([() => props.appId, () => props.photoKey, () => props.label], () => (broken.value = false));
// A person shows their photo (if any); an app shows its icon. Either falls back to the tile below.
const icon = computed(() => {
  if (broken.value) return null;
  return props.person ? tug.contactPhoto(props.photoKey ?? props.label) : tug.iconFor(props.appId);
});
</script>

<template>
  <img
    v-if="icon"
    :src="icon"
    :alt="label"
    :title="label"
    :class="[size === 'sm' ? 'size-8' : 'size-10', 'shrink-0 rounded-lg border border-hairline-soft object-cover']"
    draggable="false"
    @error="broken = true"
  />
  <div
    v-else
    :class="[
      tone,
      size === 'sm' ? 'size-8 text-[11px]' : 'size-10 text-[13px]',
      'flex shrink-0 items-center justify-center rounded-lg font-semibold tracking-wide',
    ]"
    :title="label"
  >
    {{ initials(label) }}
  </div>
</template>
