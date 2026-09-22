<script setup lang="ts">
/**
 * One row of a settings section (`docs/12` §12).
 *
 * The reference's anatomy: what the thing is on the left — label, then its description — and
 * the control on the right. Three things this app adds to that shape, all of them because the
 * row is a *write* to a file another process reads:
 *
 * * **what the write costs** — a chip and one sentence from `restart`, and, where a restart is
 *   what would make the value live, the action that performs it;
 * * **where the value comes from** — a chip from `origin`, quiet enough to ignore and specific
 *   enough to explain a value that does not match what someone once wrote;
 * * **what happened** — the engine's own `message` after a write, verbatim, and its refusal
 *   when it said no. A short paraphrase would be a second, weaker claim.
 *
 * A row with an empty description shows its key instead, in mono. Forty-four curated keys carry
 * no `ui.description` — the schema declares none — and a key is what someone needs to find that
 * setting in the raw config file, which beats a blank line.
 *
 * A gated row keeps its place and loses its control: the reason stands where the control was,
 * with a way through to the key the reason names. That is `docs/13` §Q3's rule — a setting that
 * vanishes reads as one that never existed.
 */
import { computed } from "vue";

import type { SettingsRow } from "../../bridge";
import { originHint, restartHint } from "../../lib/settings";
import Icon from "../ui/Icon.vue";
import SettingsAction from "./SettingsAction.vue";
import SettingsControl from "./SettingsControl.vue";

const props = defineProps<{
  row: SettingsRow;
  /** A write is in flight for this row. */
  pending: boolean;
  /** The host's sentence after the last write to this row, if any. */
  message?: string | null;
  /** The host's refusal, in its own words. */
  error?: string | null;
  /** A search hit landed here. */
  highlighted?: boolean;
  /** Where this row's `needs` key lives, resolved by the screen that holds the catalog. */
  needs?: { sectionId: string; title: string } | null;
}>();

const emit = defineEmits<{
  (event: "write", value: unknown): void;
  (event: "reset"): void;
  (event: "restart"): void;
  (event: "navigate", sectionId: string, rowId: string): void;
  (event: "action", action: string): void;
  (event: "changed"): void;
}>();

/** The catalog's words for this row's provenance and cost; `null` on the other two roles. */
const from = computed(() => (props.row.role === "setting" ? originHint(props.row.origin) : null));
const cost = computed(() => (props.row.role === "setting" ? restartHint(props.row.restart) : null));
/** The fixed OMP role map is a full-width picker, not a generic label/control row. */
const modelRoles = computed(() => props.row.role === "setting" && props.row.key === "modelRoles");
/** Compound editors need the page width; a side-by-side label column only crowds their data. */
const compoundEditor = computed(
  () => props.row.role === "setting" && (props.row.control === "list" || props.row.control === "record"),
);
</script>

<template>
  <!--
    A settings key. The control is disabled while a write is in flight — a second write to the
    same key would race the first — and the row says it is writing rather than looking frozen.
  -->
  <div
    v-if="props.row.role === 'setting'"
    class="flex rounded-[8px] px-3 py-3 transition-colors"
    :class="[
      compoundEditor ? 'flex-col gap-3' : 'items-start gap-6',
      props.highlighted === true ? 'bg-selected ring-1 ring-accent' : 'hover:bg-raised/40',
    ]"
    :data-row-key="props.row.key"
    data-role="setting"
    :data-control="props.row.control"
    :data-origin="props.row.origin"
    :data-restart="props.row.restart"
    :data-pending="props.pending ? 'true' : null"
    :data-layout="compoundEditor ? 'stacked' : 'inline'"
  >
    <div class="min-w-0" :class="compoundEditor ? 'w-full' : 'flex-1'">
      <p class="text-[13.5px] font-medium text-fg">{{ props.row.label }}</p>

      <p v-if="props.row.description !== ''" class="mt-0.5 text-[12px] leading-relaxed text-faint">
        {{ props.row.description }}
      </p>
      <!-- No description in the schema: the key is the honest thing to put here. -->
      <p v-else-if="!modelRoles" class="mt-0.5 font-mono text-[11.5px] leading-relaxed text-faint">
        {{ props.row.key }}
      </p>

      <p
        v-if="props.row.danger"
        class="mt-1.5 flex items-start gap-1.5 text-[11px] leading-relaxed"
        :class="props.row.danger.level === 'confirm' ? 'text-err' : 'text-warn'"
        :data-danger="props.row.danger.level"
      >
        <Icon name="alert" class="mt-0.5 h-3 w-3 shrink-0" />
        <span>{{ props.row.danger.why }}</span>
      </p>

      <!-- What the write costs, and where the value is written. -->
      <div class="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-[10.5px] text-faint">
        <span class="rounded-full border border-line/40 bg-raised/70 px-2 py-0.5" :title="from?.note">{{ from?.chip }}</span>
        <span class="rounded-full border border-line/40 bg-raised/70 px-2 py-0.5" :title="cost?.detail">{{ cost?.chip }}</span>
        <span>{{ cost?.note }}</span>
        <button
          v-if="cost?.action === 'restart' && !props.row.gated"
          type="button"
          class="rounded-[6px] border border-line bg-surface/50 px-2 py-0.5 text-dim transition-colors hover:border-line-strong hover:bg-raised hover:text-fg"
          title="restart every live session, so what is already written is in effect now"
          data-action="restart-sessions"
          @click="emit('restart')"
        >
          Restart sessions
        </button>
        <span
          v-else-if="cost?.action === 'relaunch' && !props.row.gated"
          class="rounded-full border border-warn/40 bg-warn/10 px-2 py-0.5 text-[10.5px] text-warn"
          title="this app has no relaunch command — quit and reopen it to apply this"
          data-action="relaunch"
        >
          relaunch needed
        </span>
        <span v-if="props.pending" class="flex items-center gap-1 text-accent" data-writing="">
          <span class="size-1.5 animate-pulse rounded-full bg-accent" />
          writing…
        </span>
      </div>

      <p v-if="props.message" class="mt-1 text-[11px] leading-relaxed text-ok" data-message="">
        {{ props.message }}
      </p>
      <p v-if="props.error" class="mt-1 text-[11px] leading-relaxed text-err" data-error="">
        {{ props.error }}
      </p>
    </div>

    <div
      class="flex shrink-0 pt-0.5"
      :class="compoundEditor ? 'w-full justify-stretch' : 'w-[340px] justify-end'"
    >
      <!-- A gated row: the reason where the control would be, and the way through to its key. -->
      <div v-if="props.row.gated" class="flex flex-col items-end gap-1" data-gated-control="">
        <p class="max-w-[300px] text-right text-[11.5px] leading-relaxed text-faint">
          {{ props.row.gated.reason }}
        </p>
        <button
          v-if="props.needs !== null && props.needs !== undefined && props.row.gated.needs !== null"
          type="button"
          class="text-[10.5px] text-dim underline decoration-dotted hover:text-fg"
          :title="`go to ${props.needs.title}`"
          data-action="gated-needs"
          @click="emit('navigate', props.needs.sectionId, props.row.gated.needs)"
        >
          open {{ props.row.gated.needs }}
        </button>
      </div>
      <SettingsControl
        v-else
        :row="props.row"
        :pending="props.pending"
        @write="emit('write', $event)"
        @reset="emit('reset')"
      />
    </div>
  </div>

  <!-- A fact the app reports: the value is the whole of it. -->
  <div
    v-else-if="props.row.role === 'static'"
    class="flex items-start gap-4 rounded-[6px] px-3 py-2.5 hover:bg-raised"
    :data-row-key="props.row.id"
    data-role="static"
  >
    <div class="min-w-0 flex-1">
      <p class="text-[13px] text-fg">{{ props.row.label }}</p>
      <p v-if="props.row.description !== ''" class="mt-0.5 text-[11.5px] leading-relaxed text-faint">
        {{ props.row.description }}
      </p>
      <p v-else class="mt-0.5 font-mono text-[11.5px] text-faint">{{ props.row.id }}</p>
    </div>
    <div class="flex w-[340px] shrink-0 justify-end pt-0.5">
      <span class="break-all text-right font-mono text-[12px] text-dim" data-value="">
        {{ props.row.value }}
      </span>
    </div>
  </div>

  <!-- A control that does something. -->
  <div
    v-else
    class="flex items-start gap-4 rounded-[6px] px-3 py-2.5 hover:bg-raised"
    :data-row-key="props.row.id"
    data-role="action"
    :data-action-row="props.row.action"
    :data-tone="props.row.tone"
  >
    <div class="min-w-0 flex-1">
      <p class="text-[13px] text-fg">{{ props.row.label }}</p>
      <p v-if="props.row.description !== ''" class="mt-0.5 text-[11.5px] leading-relaxed text-faint">
        {{ props.row.description }}
      </p>
      <p v-else class="mt-0.5 font-mono text-[11.5px] text-faint">{{ props.row.id }}</p>
    </div>
    <div class="flex w-[340px] shrink-0 justify-end pt-0.5">
      <SettingsAction :row="props.row" @dispatch="emit('action', $event)" @changed="emit('changed')" />
    </div>
  </div>
</template>
