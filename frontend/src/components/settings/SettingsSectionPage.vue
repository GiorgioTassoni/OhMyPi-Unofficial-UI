<script setup lang="ts">
/**
 * One section of the settings screen: its title, its blurb, and its groups (`docs/12` §12).
 *
 * The group heading is the engine's own group name, kept uppercase over a hairline rule — the
 * reference's section header. Rows are drawn by `SettingsRow`, which owns the anatomy; this
 * file owns the vertical rhythm between them. Their already-prioritized order arrives from the
 * screen model, so this renderer contains no second ordering policy.
 */
import type { SettingsRow as SettingsRowModel, SettingsSection } from "../../bridge";
import { rowId } from "../../lib/settings";
import SettingsRow from "./SettingsRow.vue";

const props = defineProps<{
  section: SettingsSection;
  /** The keys with a write in flight, so the row can show it. */
  pending: Set<string>;
  /** The host's sentence per row, after a write. Absent for rows nobody has written. */
  messages: Record<string, string | undefined>;
  /** The host's refusal per row. */
  errors: Record<string, string | undefined>;
  /** The row a search hit landed on, highlighted until the next navigation. */
  highlight: string | null;
  /** Where a gated row's `needs` key lives, resolved against the whole catalog. */
  locate: (key: string) => { sectionId: string; title: string } | null;
}>();

const emit = defineEmits<{
  (event: "write", row: SettingsRowModel, value: unknown): void;
  (event: "reset", row: SettingsRowModel): void;
  (event: "restart"): void;
  (event: "navigate", sectionId: string, rowId: string): void;
  (event: "action", action: string): void;
  (event: "changed"): void;
}>();

/** A gated row's link target, or `null` when the host named no key to reach. */
function needsFor(row: SettingsRowModel): { sectionId: string; title: string } | null {
  if (row.role !== "setting" || row.gated?.needs == null) return null;
  return props.locate(row.gated.needs);
}
</script>

<template>
  <div>
    <h1 class="text-[24px] font-semibold tracking-tight text-fg">{{ props.section.title }}</h1>
    <p v-if="props.section.blurb !== ''" class="mt-1.5 text-[13px] leading-relaxed text-dim">
      {{ props.section.blurb }}
    </p>

    <section v-for="group in props.section.groups" :key="group.name" class="mt-8">
      <div class="flex items-center gap-3 border-b border-line/50 pb-2">
        <h2 class="text-[12px] font-semibold uppercase tracking-wider text-fg/80">
          {{ group.name }}
        </h2>
      </div>

      <div class="flex flex-col divide-y divide-line/30">
        <SettingsRow
          v-for="row in group.rows"
          :key="rowId(row)"
          :row="row"
          :pending="props.pending.has(rowId(row))"
          :message="props.messages[rowId(row)] ?? null"
          :error="props.errors[rowId(row)] ?? null"
          :highlighted="props.highlight === rowId(row)"
          :needs="needsFor(row)"
          @write="emit('write', row, $event)"
          @reset="emit('reset', row)"
          @restart="emit('restart')"
          @navigate="(sectionId, target) => emit('navigate', sectionId, target)"
          @action="emit('action', $event)"
          @changed="emit('changed')"
        />
      </div>
    </section>

    <p v-if="props.section.groups.length === 0" class="mt-8 text-[12.5px] text-faint">
      This section has no rows in this build.
    </p>
  </div>
</template>
