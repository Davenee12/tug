<script setup lang="ts">
// Settings › Developer tools: the master switch with its consent line, what AI tools may do,
// who has connected, Revoke access, copy-paste setup for each AI tool, and the `tug` command.
import { computed, onMounted, onUnmounted, ref } from "vue";
import { Check, CircleAlert, Copy, KeyRound, Terminal } from "lucide-vue-next";
import { useDevToolsStore } from "../stores/devtools";
import { useTugStore } from "../stores/tug";
import { copyText } from "../lib/clipboard";
import { lastUsedLabel, PERMISSION_HELP, setupSnippets, type SetupSnippet } from "../lib/devtools";
import SettingsRow from "./SettingsRow.vue";
import SettingsSwitch from "./SettingsSwitch.vue";

const devtools = useDevToolsStore();
const tug = useTugStore();

const now = ref(Date.now());
let clock: number | undefined;
onMounted(() => {
  void devtools.refresh();
  clock = window.setInterval(() => (now.value = Date.now()), 30_000);
});
onUnmounted(() => {
  window.clearInterval(clock);
  window.clearTimeout(revokeTimer);
});

const s = computed(() => devtools.status);
const enabled = computed({
  get: () => s.value?.enabled ?? false,
  set: (v) => void devtools.setEnabled(v),
});

const snippets = computed<SetupSnippet[]>(() => (s.value?.cliPath ? setupSnippets(s.value.cliPath) : []));
const tab = ref<SetupSnippet["id"]>("claude");
const shown = computed(() => snippets.value.find((x) => x.id === tab.value) ?? snippets.value[0]);

const copied = ref<string | null>(null);
async function copy(key: string, text: string) {
  if (await copyText(text)) {
    copied.value = key;
    window.setTimeout(() => (copied.value = copied.value === key ? null : copied.value), 2000);
  } else {
    tug.notify("error", "Couldn't copy. Select the text and copy it instead.");
  }
}

// Revoke access: tap twice, like Clear history.
const confirmRevoke = ref(false);
let revokeTimer: number | undefined;
async function revoke() {
  if (!confirmRevoke.value) {
    confirmRevoke.value = true;
    window.clearTimeout(revokeTimer);
    revokeTimer = window.setTimeout(() => (confirmRevoke.value = false), 4000);
    return;
  }
  confirmRevoke.value = false;
  if (await devtools.revoke()) tug.notify("info", "Access revoked. Restart your AI tools to connect them again.");
}

async function togglePath() {
  const adding = !s.value?.onPath;
  if (await devtools.setOnPath(adding)) {
    tug.notify("info", adding ? "Added. Open a new terminal and type tug." : "Removed tug from your PATH.");
  }
}

const COMMANDS: Array<[string, string]> = [
  ["tug code", "Print the newest verification code (add --copy to put it on the clipboard)"],
  ["tug boat photo.png", "Send files to your phone with Tugboat"],
  ['tug text Sam "On my way"', "Text someone (tug asks you first)"],
  ["tug status", "Is tug running, and how's the phone"],
];
</script>

<template>
  <div>
    <div class="mb-6 rounded-xl bg-surface-card">
      <SettingsRow
        label="Let AI tools use tug"
        description="AI tools you connect, like Claude Code or Cursor, will be able to read your texts and notifications. They can only send a text after you click Send in tug. Off until you turn it on, and only tools on this PC can connect."
      >
        <SettingsSwitch v-model="enabled" label="Let AI tools use tug" :disabled="devtools.working || !s" />
      </SettingsRow>
      <div
        v-if="s && !s.bridgeRunning"
        class="flex items-start gap-2.5 border-t border-hairline-soft px-5 py-3 text-[13px] text-body"
      >
        <CircleAlert :size="16" class="mt-0.5 shrink-0 text-accent-amber" />
        AI tools can't reach tug right now. Quit tug from the tray and open it again.
      </div>
      <p v-if="devtools.error" class="border-t border-hairline-soft px-5 py-3 text-[13px] text-error">{{ devtools.error }}</p>
    </div>

    <template v-if="s?.enabled">
      <p class="caption-upper mb-2 px-1 text-muted">What AI tools can do</p>
      <div class="mb-6 divide-y divide-hairline-soft rounded-xl bg-surface-card">
        <SettingsRow v-for="p in s.permissions" :key="p.key" :label="p.label" :description="PERMISSION_HELP[p.key]">
          <SettingsSwitch
            :model-value="p.on"
            :label="p.label"
            :disabled="devtools.working"
            @update:model-value="(v: boolean) => devtools.setPermission(p.key, v)"
          />
        </SettingsRow>
      </div>

      <p class="caption-upper mb-2 px-1 text-muted">Connected tools</p>
      <div class="mb-6 rounded-xl bg-surface-card">
        <ul v-if="s.clients.length" class="divide-y divide-hairline-soft">
          <li v-for="c in s.clients" :key="`${c.kind}:${c.name}`" class="flex items-center gap-3 px-5 py-2.5 text-[14px]">
            <Terminal v-if="c.kind === 'cli'" :size="15" class="shrink-0 text-muted" />
            <KeyRound v-else :size="15" class="shrink-0 text-muted" />
            <span class="min-w-0 flex-1 truncate text-ink">{{ c.name }}</span>
            <span class="shrink-0 text-[12px] text-muted-soft">{{ c.kind === "cli" ? "tug command" : "AI tool" }} · {{ lastUsedLabel(c.lastUsed, now) }}</span>
          </li>
        </ul>
        <p v-else class="px-5 py-3 text-[13px] text-muted">No tools have connected yet.</p>
        <SettingsRow
          class="border-t border-hairline-soft"
          label="Revoke access"
          description="Disconnects every AI tool now. Each one has to be restarted to connect again."
        >
          <button :class="['btn-secondary btn-sm', confirmRevoke ? 'text-error' : '']" :disabled="devtools.working" @click="revoke">
            {{ confirmRevoke ? "Tap again to revoke" : "Revoke access" }}
          </button>
        </SettingsRow>
      </div>
    </template>

    <p class="caption-upper mb-2 px-1 text-muted">Set up an AI tool</p>
    <div class="mb-6 rounded-xl bg-surface-card px-5 py-4">
      <template v-if="shown">
        <div class="flex flex-wrap gap-1.5" role="tablist" aria-label="AI tool">
          <button
            v-for="x in snippets"
            :key="x.id"
            role="tab"
            :aria-selected="x.id === shown.id"
            :class="['rounded-md px-3 py-1 text-[13px]', x.id === shown.id ? 'bg-surface-cream-strong font-medium text-ink' : 'text-muted active:bg-surface-cream-strong']"
            @click="tab = x.id"
          >
            {{ x.name }}
          </button>
        </div>
        <p class="mt-3 text-[13px] text-muted">{{ shown.where }}:</p>
        <div class="relative mt-1.5">
          <pre class="selectable overflow-x-auto rounded-lg bg-canvas px-3 py-2.5 pr-20 font-mono text-[12px] leading-relaxed break-all whitespace-pre-wrap text-ink">{{ shown.code }}</pre>
          <button class="btn-secondary btn-sm absolute top-1.5 right-1.5" @click="copy(shown.id, shown.code)">
            <Check v-if="copied === shown.id" :size="13" /><Copy v-else :size="13" />
            {{ copied === shown.id ? "Copied" : "Copy" }}
          </button>
        </div>
        <p class="mt-3 text-[12px] text-muted-soft">
          ChatGPT's desktop app can only connect to tools on the internet, not ones on this PC, so it can't use tug yet.
          {{ s?.enabled ? "" : "Turn on Let AI tools use tug first." }}
        </p>
      </template>
      <p v-else class="text-[13px] text-muted">The tug command isn't included in this copy of tug, so AI tools can't be set up here.</p>
    </div>

    <template v-if="s?.cliPath">
      <p class="caption-upper mb-2 px-1 text-muted">The tug command</p>
      <div class="rounded-xl bg-surface-card">
        <ul class="divide-y divide-hairline-soft">
          <li v-for="[cmd, what] in COMMANDS" :key="cmd" class="flex items-center gap-4 px-5 py-2.5">
            <code class="shrink-0 font-mono text-[12px] text-ink">{{ cmd }}</code>
            <span class="min-w-0 flex-1 text-right text-[13px] text-muted">{{ what }}</span>
          </li>
        </ul>
        <SettingsRow
          class="border-t border-hairline-soft"
          label="Use tug in any terminal"
          :description="
            s.onPath
              ? 'tug is on your PATH. Terminals opened from now on know the tug command.'
              : 'Adds tug\'s command folder to your own PATH (nothing else changes). Or run it by its full path.'
          "
        >
          <button class="btn-secondary btn-sm" :disabled="devtools.working" @click="togglePath">
            {{ s.onPath ? "Remove from PATH" : "Add tug to PATH" }}
          </button>
        </SettingsRow>
        <div class="flex items-center gap-2 border-t border-hairline-soft px-5 py-2.5">
          <code class="selectable min-w-0 flex-1 truncate font-mono text-[12px] text-muted">{{ s.cliPath }}</code>
          <button class="btn-secondary btn-sm shrink-0" @click="copy('path', s.cliPath)">
            <Check v-if="copied === 'path'" :size="13" /><Copy v-else :size="13" />
            {{ copied === "path" ? "Copied" : "Copy path" }}
          </button>
        </div>
      </div>
    </template>
  </div>
</template>
