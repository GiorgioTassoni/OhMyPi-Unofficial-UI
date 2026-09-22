<script setup lang="ts">
/**
 * Where the settings come from, and what is wrong with any of it (`docs/12` §12).
 *
 * The body of the settings Info page: which files the app reads, which of them the user cannot
 * write, and whether the catalog and the engine agree about what exists. Four things are worth
 * interrupting for, and each is
 * named rather than counted: a file that exists and cannot be parsed, keys a file sets that the
 * app will not act on, a read-only overlay, and an engine with settings this build has never
 * heard of — the last one pointing at the raw config, which is the only place to reach them.
 */
import { computed } from "vue";

import type { SettingsDrift } from "../../bridge";
import type { SourceLine } from "../../lib/settings";
import Icon from "../ui/Icon.vue";

const props = defineProps<{
  lines: SourceLine[];
  drift: SettingsDrift;
  /** The engine release the shipped catalog was generated against. */
  catalogVersion: string;
  /** What the last screen-level action did, if anything — a restart, an action's notice. */
  note?: string | null;
}>();

const emit = defineEmits<{ (event: "raw"): void }>();

/** The lines with something to say beyond their path. */
const notes = computed(() => props.lines.filter((line) => line.note !== null));

/** The unknown keys, capped: a list of forty in a band is a wall, not a warning. */
const unknown = computed(() => {
  const keys = props.drift.unknownKeys;
  return {
    listed: keys.slice(0, 6).join(", "),
    rest: Math.max(0, keys.length - 6),
  };
});
</script>

<template>
  <div class="mb-6 flex flex-col gap-2 rounded-[10px] border border-line/60 bg-surface/60 p-3.5 text-[11px] shadow-sm">
    <div class="flex items-center justify-between border-b border-line/40 pb-2">
      <div class="flex items-center gap-2">
        <span class="text-[11px] font-semibold uppercase tracking-wider text-faint">Config Sources</span>
      </div>
      <span
        class="shrink-0 rounded border border-line/40 bg-raised/80 px-2 py-0.5 font-mono text-[10px] text-faint"
        title="the engine release this build's settings catalog was generated against"
      >
        catalog {{ props.catalogVersion }}
      </span>
    </div>

    <div class="flex flex-col gap-1.5 pt-1">
      <div
        v-for="line in props.lines"
        :key="`${line.role}:${line.path}`"
        class="flex items-center justify-between gap-3 rounded-[6px] bg-raised/40 px-2.5 py-1.5 font-mono text-[11px]"
        :class="line.bad ? 'text-warn border border-warn/30' : 'text-dim'"
        :data-source="line.role"
      >
        <span class="truncate break-all select-all text-fg/80" :title="line.path">{{ line.path }}</span>
        <span class="shrink-0 rounded border border-line/40 bg-raised px-1.5 py-0.5 text-[10px] uppercase font-medium text-faint">
          {{ line.role }}
        </span>
      </div>
    </div>

    <div v-if="notes.length > 0" class="flex flex-col gap-1 pt-1">
      <p
        v-for="line in notes"
        :key="`note:${line.path}`"
        class="flex items-start gap-1.5 text-[11px] leading-relaxed"
        :class="line.bad ? 'text-warn' : 'text-faint'"
      >
        <Icon v-if="line.bad" name="alert" class="mt-0.5 h-3 w-3 shrink-0" />
        <span class="break-all">{{ line.path }} — {{ line.note }}</span>
      </p>
    </div>

    <p v-if="props.drift.unknownKeys.length > 0" class="flex items-start gap-1.5 leading-relaxed text-warn pt-1" data-drift="unknown">
      <Icon name="alert" class="mt-0.5 h-3 w-3 shrink-0" />
      <span>
        The engine has {{ props.drift.unknownKeys.length }}
        {{ props.drift.unknownKeys.length === 1 ? "setting" : "settings" }} this build does not
        know:
        <span class="font-mono"
          >{{ unknown.listed }}{{ unknown.rest > 0 ? `, and ${unknown.rest} more` : "" }}</span
        >. They are reachable through the
        <button class="underline decoration-dotted hover:text-fg" data-action="drift-raw" @click="emit('raw')">
          raw config
        </button>.
      </span>
    </p>

    <p v-if="props.drift.missingKeys.length > 0" class="text-[11px] leading-relaxed text-faint pt-1" data-drift="missing">
      This build knows {{ props.drift.missingKeys.length }}
      {{ props.drift.missingKeys.length === 1 ? "setting" : "settings" }} the engine does not —
      the catalog and the engine are from different releases.
    </p>

    <p v-if="props.note" class="text-[11px] leading-relaxed text-dim pt-1" data-note="">{{ props.note }}</p>
  </div>
</template>
