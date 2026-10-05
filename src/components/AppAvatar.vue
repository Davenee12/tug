<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { avatarTone, initials } from "../lib/format";
import { useTugStore } from "../stores/tug";

// `person`: the avatar stands for someone (a conversation), not the app: always their initials,
// so a friend never looks like a company.
const props = defineProps<{ appId: string; label: string; size?: "sm" | "md"; person?: boolean }>();
const tug = useTugStore();
const tone = computed(() => avatarTone(props.appId));
const broken = ref(false);
watch(() => props.appId, () => (broken.value = false));
const icon = computed(() => (props.person || broken.value ? null : tug.iconFor(props.appId)));
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
