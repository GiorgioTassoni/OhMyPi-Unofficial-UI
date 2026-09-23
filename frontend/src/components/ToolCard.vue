<script setup lang="ts">
/**
 * One tool call, as `docs/12` §3.2 asks: a header that says what the call is, and
 * a body that shows what it did.
 *
 * All the deciding happens in `lib/toolView.ts` — this arranges the outcome. A
 * tool the view does not know still renders: its arguments and its result are
 * what every tool has.
 */
import { computed, ref, watch } from "vue";
import type { ToolSnapshot } from "../bridge";
import { touchedFiles, toolView } from "../lib/toolView";
import DiffView from "./DiffView.vue";
import Icon from "./ui/Icon.vue";

const props = defineProps<{ tool: ToolSnapshot }>();
const view = computed(() => toolView(props.tool));
const touched = computed(() => touchedFiles(props.tool));

const isCommand = computed(
  () =>
    props.tool.toolName === "bash" ||
    props.tool.toolName === "bash_interactive" ||
    props.tool.toolName === "eval",
);

const isMutation = computed(
  () => touched.value.length > 0 || view.value.body.kind === "diff",
);

const denied = computed(
  () => props.tool.finished && /^Tool call denied by user(?::|$)/i.test(props.tool.output.trim()),
);
const failedOrDenied = computed(() => props.tool.finished && (props.tool.isError || denied.value));
const expanded = ref(!props.tool.finished || failedOrDenied.value);

// Live edits start open while the tool runs, then settle into the compact card.
watch(
  () => props.tool.finished,
  (finished) => {
    if (finished && !failedOrDenied.value && isMutation.value) expanded.value = false;
  },
);

const mutationTitle = computed(() => {
  const files = touched.value;
  if (files.length === 0) return "Edited files";
  const verb = files[0].kind === "written" ? "Wrote" : "Edited";
  if (files.length !== 1) return `${verb} ${files.length} files`;
  const filename = files[0].path.split(/[\\/]/).filter(Boolean).pop() || files[0].path;
  return `${verb} ${filename}`;
});

const diffStats = computed(() => {
  if (view.value.body.kind !== "diff") return null;
  const lines = view.value.body.text.split("\n");
  let added = 0;
  let removed = 0;
  for (const line of lines) {
    if (line.startsWith("+++ ") || line.startsWith("--- ")) continue;
    if (line.startsWith("+")) added += 1;
    else if (line.startsWith("-")) removed += 1;
  }
  return { added, removed };
});
</script>

<template>
  <div class="my-1 overflow-hidden rounded-[10px] border border-line/60 bg-surface shadow-sm">
    <!-- Case 1: File Mutations -->
    <div v-if="isMutation">
      <button
        type="button"
        class="flex w-full cursor-pointer select-none items-center justify-between gap-2 px-3 py-2 text-left transition-colors hover:bg-raised/40"
        :class="expanded ? 'border-b border-line/40' : ''"
        :aria-expanded="expanded"
        @click="expanded = !expanded"
      >
        <div class="flex min-w-0 items-center gap-2">
          <Icon name="pencil" class="h-3.5 w-3.5 shrink-0 text-accent" />
          <span class="flex min-w-0 flex-col gap-0.5">
            <span class="truncate text-[12.5px] font-medium text-fg" :title="mutationTitle">{{ mutationTitle }}</span>
            <span v-if="diffStats" class="flex items-center gap-1 font-mono text-[11px] leading-none">
              <span v-if="diffStats.added > 0" class="font-semibold text-ok">+{{ diffStats.added }}</span>
              <span v-if="diffStats.removed > 0" class="font-semibold text-err">-{{ diffStats.removed }}</span>
            </span>
          </span>
        </div>
        <div class="flex shrink-0 items-center gap-2">
          <span v-if="failedOrDenied" class="text-err" role="img" :aria-label="denied ? 'Tool call denied' : 'Tool call failed'" :title="denied ? 'Tool call denied' : 'Tool call failed'">
            <Icon name="close" class="h-3.5 w-3.5" />
          </span>
          <Icon
            name="chevron-right"
            class="h-3.5 w-3.5 text-faint transition-transform"
            :class="expanded ? 'rotate-90' : ''"
          />
        </div>
      </button>

      <!-- Full paths belong to the expanded view, never the compact header. -->
      <div v-if="expanded && touched.length > 0" class="flex flex-col divide-y divide-line/30 px-3 py-1.5 text-[12px] select-text">
        <div
          v-for="file in touched"
          :key="file.path"
          class="flex items-start gap-2 py-1 font-mono text-dim"
        >
          <Icon name="file" class="mt-0.5 h-3.5 w-3.5 shrink-0 text-faint select-none" />
          <span class="min-w-0 break-all">{{ file.path }}</span>
        </div>
      </div>

      <!-- Expandable Diff -->
      <div v-if="expanded && view.body.kind === 'diff'" class="border-t border-line/40">
        <DiffView :diff="view.body.text" />
      </div>
    </div>

    <!-- Case 2: Command Execution (Run command card matching reference 07-chats.png) -->
    <div v-else-if="isCommand">
      <header
        class="flex cursor-pointer select-none items-center justify-between gap-2 px-3 py-2 transition-colors hover:bg-raised/40"
        @click="expanded = !expanded"
      >
        <div class="flex min-w-0 flex-1 items-center gap-2">
          <Icon name="terminal" class="h-3.5 w-3.5 shrink-0 text-accent" />
          <span class="shrink-0 text-[12px] font-medium text-fg">Run command</span>
          <code
            v-if="view.summary"
            class="truncate rounded bg-raised/80 px-2 py-0.5 font-mono text-[11.5px] text-dim select-text cursor-text"
            @click.stop
          >
            {{ view.summary }}
          </code>
        </div>
        <div class="flex shrink-0 items-center gap-2">
          <span v-if="failedOrDenied" class="text-err" role="img" :aria-label="denied ? 'Tool call denied' : 'Tool call failed'" :title="denied ? 'Tool call denied' : 'Tool call failed'">
            <Icon name="close" class="h-3.5 w-3.5" />
          </span>
          <Icon
            name="chevron-right"
            class="h-3.5 w-3.5 text-faint transition-transform"
            :class="expanded ? 'rotate-90' : ''"
          />
        </div>
      </header>

      <pre
        v-if="expanded && view.body.text"
        class="max-h-80 overflow-auto whitespace-pre-wrap break-words border-t border-line/40 bg-canvas/40 px-3 py-2 font-mono text-[11.5px] text-dim select-text"
      >{{ view.body.text }}</pre>
      <p
        v-else-if="expanded && !view.body.text && !props.tool.finished"
        class="border-t border-line/40 px-3 py-2 font-mono text-[11px] italic text-faint"
      >
        executing…
      </p>
    </div>

    <!-- Case 3: Standard Tool Call -->
    <div v-else>
      <header
        class="flex cursor-pointer select-none items-center justify-between gap-2 px-3 py-2 transition-colors hover:bg-raised/40"
        @click="expanded = !expanded"
      >
        <div class="flex min-w-0 flex-1 items-center gap-2">
          <Icon name="wrench" class="h-3.5 w-3.5 shrink-0 text-faint" />
          <span class="shrink-0 text-[12px] font-medium text-fg">{{ tool.toolName }}</span>
          <span
            v-if="tool.intent || view.summary"
            class="truncate font-mono text-[11.5px] text-dim select-text cursor-text"
            @click.stop
          >
            {{ view.summary || tool.intent }}
          </span>
        </div>
        <div class="flex shrink-0 items-center gap-2">
          <span v-if="failedOrDenied" class="text-err" role="img" :aria-label="denied ? 'Tool call denied' : 'Tool call failed'" :title="denied ? 'Tool call denied' : 'Tool call failed'">
            <Icon name="close" class="h-3.5 w-3.5" />
          </span>
          <Icon
            name="chevron-right"
            class="h-3.5 w-3.5 text-faint transition-transform"
            :class="expanded ? 'rotate-90' : ''"
          />
        </div>
      </header>

      <pre
        v-if="expanded && view.body.text"
        class="max-h-80 overflow-auto whitespace-pre-wrap break-words border-t border-line/40 bg-canvas/40 px-3 py-2 font-mono text-[11.5px] text-dim select-text"
        :class="view.body.kind === 'args' ? 'text-faint' : ''"
      >{{ view.body.text }}</pre>
    </div>

    <!-- Truncated output note if present -->
    <p
      v-if="view.artifact"
      class="border-t border-line/40 px-3 py-1.5 font-mono text-[10.5px] text-warn select-text"
    >
      truncated · full output in artifact://{{ view.artifact }}
    </p>
  </div>
</template>
