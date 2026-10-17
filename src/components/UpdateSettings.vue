<script setup lang="ts">
// Settings › About › Updates: the version line's neighbour. Says where updates are (Checking…,
// You're up to date, Downloading…, Ready — Restart to update, Couldn't update — Try again), the
// "Download updates automatically" switch and a link to the release notes. Builds without an update
// key say so and offer nothing to press.
import { computed } from "vue";
import { ExternalLink, LoaderCircle, RefreshCw } from "lucide-vue-next";
import { useUpdateStore } from "../stores/update";
import { autoDescription } from "../lib/update";
import SettingsRow from "./SettingsRow.vue";
import SettingsSwitch from "./SettingsSwitch.vue";

const update = useUpdateStore();
const about = computed(() => update.about);
const auto = computed({
  get: () => update.status?.auto ?? true,
  set: (v: boolean) => void update.setAuto(v),
});

function press() {
  switch (about.value.action?.run) {
    case "check":
      return void update.check();
    case "download":
      return void update.download();
    case "restart":
      return update.pressPill();
    case "open":
      return void update.openReleasePage();
  }
}
</script>

<template>
  <SettingsRow label="Updates" :description="about.line">
    <button v-if="about.action" class="btn-secondary btn-sm" :disabled="about.busy" @click="press">
      <LoaderCircle v-if="about.busy" :size="14" class="animate-spin" />
      <RefreshCw v-else-if="about.action.run === 'check'" :size="14" />
      {{ about.action.label }}
    </button>
    <LoaderCircle v-else-if="about.busy" :size="16" class="animate-spin text-muted" aria-hidden="true" />
  </SettingsRow>
  <SettingsRow
    v-if="about.showAuto"
    :label="update.status?.portable ? 'Check for updates automatically' : 'Download updates automatically'"
    :description="autoDescription(update.status?.portable ?? false)"
  >
    <SettingsSwitch v-model="auto" :label="update.status?.portable ? 'Check for updates automatically' : 'Download updates automatically'" />
  </SettingsRow>
  <SettingsRow
    v-if="about.showAuto"
    label="Release notes"
    description="What's in each version, on tug's GitHub releases page. Update checks only ever contact GitHub."
  >
    <button class="btn-secondary btn-sm" @click="update.openReleasePage()">
      <ExternalLink :size="14" /> Release notes
    </button>
  </SettingsRow>
</template>
