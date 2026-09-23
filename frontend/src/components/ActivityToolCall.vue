<script setup lang="ts">
import { computed, ref } from "vue";
import { activityDiff, activityDiffCounts } from "../lib/activityDiff";
import { commandProgram } from "../lib/approval";
import { toolView, touchedFiles } from "../lib/toolView";
import type { DisclosureMemory } from "../lib/disclosureMemory";
import type { GroupKind, IndexedRow } from "../lib/turnActivity";
import DiffView from "./DiffView.vue";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  entry: IndexedRow;
  kind: GroupKind;
  standalone?: boolean;
  flashed: boolean;
  memory: DisclosureMemory;
}>();
const tool = computed(() => props.entry.row.tool!);
const view = computed(() => toolView(tool.value));
const commandPrefix = computed(() => commandProgram(view.value.summary ?? "") ?? tool.value.toolName);
const expanded = ref(props.memory.get("call", props.entry.index, false));
function toggled(event: Event): void {
  expanded.value = (event.target as HTMLDetailsElement).open;
  props.memory.set("call", props.entry.index, expanded.value);
}
const edits = computed(() => props.kind === "edit" && expanded.value ? activityDiff(tool.value) : null);
const counts = computed(() => props.kind === "edit" ? activityDiffCounts(tool.value) : null);
const files = computed(() => props.kind === "edit" ? touchedFiles(tool.value) : []);
const basename = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() || path;
const title = computed(() => {
  if (props.kind === "edit") {
    if (files.value.length === 1) return `Edited ${basename(files.value[0].path)}`;
    return `Edited ${files.value.length || 1} files`;
  }
  if (props.kind === "read") return `${props.standalone ? "Read " : ""}${basename(view.value.summary || "file")}`;
  if (props.kind === "command") return tool.value.finished ? "Ran command" : "Running";
  return tool.value.toolName;
});
const failed = computed(() => tool.value.isError || /^Tool call denied by user(?::|$)/i.test((tool.value.output ?? "").trim()));
const icon = computed(() => props.kind === "read" ? "file" : props.kind === "command" ? "terminal" : props.kind === "edit" ? "pencil" : "wrench");
</script>

<template>
  <div
    v-if="kind === 'read'"
    class="flex min-w-0 items-center gap-2 rounded-[6px] px-2 py-1.5 text-[12px] text-dim"
    :data-row-index="entry.index"
    :class="flashed ? 'bg-accent/10 ring-1 ring-accent' : ''"
    :title="view.summary ?? undefined"
  >
    <Icon name="file" class="h-3.5 w-3.5 shrink-0 text-faint" />
    <span class="min-w-0 truncate font-mono">{{ title }}</span>
    <Icon v-if="failed" name="close" class="ml-auto h-3.5 w-3.5 shrink-0 text-err" aria-label="Failed or denied" />
  </div>
  <details
    v-else
    class="group/call min-w-0"
    :open="expanded"
    :data-row-index="entry.index"
    :class="flashed ? 'rounded bg-accent/10 ring-1 ring-accent' : ''"
    @toggle="toggled"
  >
    <summary class="flex min-w-0 cursor-pointer list-none items-center gap-2 rounded-[6px] px-2 py-1.5 text-[12px] text-dim select-none hover:bg-raised/65 hover:text-fg [&::-webkit-details-marker]:hidden">
      <Icon :name="icon" class="h-3.5 w-3.5 shrink-0 text-faint" />
      <span :class="standalone ? 'font-medium text-fg' : 'font-mono'" class="min-w-0 truncate">{{ title }}</span>
      <code v-if="kind === 'command'" class="shrink-0 rounded-[5px] bg-raised px-1.5 py-0.5 font-mono text-[11px] text-fg">{{ commandPrefix }}</code>
      <span v-if="counts" class="ml-auto shrink-0 font-mono text-[11px]"><span class="text-ok">+{{ counts.added }}</span> <span class="text-err">-{{ counts.removed }}</span></span>
      <span v-else class="ml-auto" />
      <Icon v-if="failed" name="close" class="h-3.5 w-3.5 shrink-0 text-err" aria-label="Failed or denied" />
      <Icon name="chevron-down" class="h-3 w-3 shrink-0 text-faint transition-transform group-open/call:rotate-180" />
    </summary>

    <div v-if="expanded" class="ml-6 mb-1 min-w-0 overflow-hidden rounded-[6px] bg-raised/45 text-[11.5px] text-dim">
      <template v-if="kind === 'edit'">
        <div v-if="edits?.files.length === 0" class="px-3 py-2 text-faint">No file diff was reported for this call.</div>
        <div v-for="file in edits?.files" :key="file.path" class="min-w-0 border-t border-line/40 first:border-0">
          <div class="flex min-w-0 items-center gap-2 px-3 py-1.5 font-mono text-[11px]">
            <span class="min-w-0 flex-1 truncate" :title="file.path">{{ file.path }}</span>
            <span v-if="file.added !== null" class="shrink-0"><span class="text-ok">+{{ file.added }}</span> <span class="text-err">-{{ file.removed }}</span></span>
          </div>
          <DiffView v-if="file.pieces.length" :pieces="file.pieces" compact />
          <p v-else class="px-3 pb-2 text-[11px] text-faint">{{ tool.toolName === 'write' ? 'Whole-file write; no diff was supplied.' : 'No per-file diff was supplied.' }}</p>
        </div>
        <div v-if="edits?.batch" class="border-t border-line/40">
          <p class="px-3 py-1.5 text-[11px] text-faint">Combined batch diff · individual file attribution unavailable</p>
          <DiffView :pieces="[edits.batch]" compact />
        </div>
      </template>
      <template v-else>
        <div v-if="kind === 'command' && view.summary" class="border-b border-line/40 px-3 py-2">
          <p class="mb-1 text-[10px] uppercase tracking-wide text-faint">Command</p>
          <pre class="max-h-36 overflow-auto whitespace-pre-wrap break-words font-mono text-[11.5px] text-fg">{{ view.summary }}</pre>
        </div>
        <pre v-if="view.body.text" class="max-h-72 overflow-auto whitespace-pre-wrap break-words px-3 py-2 font-mono text-[11.5px] leading-relaxed">{{ view.body.text }}</pre>
        <p v-else-if="!tool.finished" class="px-3 py-2 text-faint">Running…</p>
        <p v-if="view.artifact" class="px-3 pb-2 text-faint">Output truncated · artifact://{{ view.artifact }}</p>
      </template>
    </div>
  </details>
</template>
