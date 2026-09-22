<script setup lang="ts">
/**
 * One tool call, as `docs/12` §3.2 asks: a header that says what the call is, and
 * a body that shows what it did.
 *
 * All the deciding happens in `lib/toolView.ts` — this arranges the outcome. A
 * tool the view does not know still renders: its arguments and its result are
 * what every tool has.
 */
import { computed, ref } from "vue";
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

const expanded = ref(!props.tool.finished || props.tool.isError || isMutation.value);

const diffStats = computed(() => {
  if (view.value.body.kind !== "diff") return null;
  const lines = view.value.body.text.split("\n");
  let added = 0;
  let removed = 0;
  for (const line of lines) {
    if (line.startsWith("+++") || line.startsWith("---")) continue;
    if (line.startsWith("+")) added += 1;
    else if (line.startsWith("-")) removed += 1;
  }
  return { added, removed };
});

const status = computed(() => {
  if (!props.tool.finished) {
    return { label: "running", tint: "bg-ok/15 text-ok border border-ok/30" };
  }
  if (props.tool.isError) {
    return { label: "failed", tint: "bg-err/15 text-err border border-err/30" };
  }

  return { label: "done", tint: "bg-raised text-faint" };
});
</script>

<template>
  <div class="my-1 overflow-hidden rounded-[10px] border border-line/60 bg-surface shadow-sm">
    <!-- Case 1: File Mutations (Changes card matching reference 07-chats.png) -->
    <div v-if="isMutation">
      <header
        class="flex cursor-pointer select-none items-center justify-between border-b border-line/40 px-3 py-2 transition-colors hover:bg-raised/40"
        @click="expanded = !expanded"
      >
        <div class="flex min-w-0 items-center gap-2">
          <Icon name="pencil" class="h-3.5 w-3.5 text-accent" />
          <span class="text-[12.5px] font-medium text-fg">Changes</span>
          <span v-if="diffStats" class="flex items-center gap-1 font-mono text-[11px]">
            <span v-if="diffStats.added > 0" class="font-semibold text-ok">+{{ diffStats.added }}</span>
            <span v-if="diffStats.removed > 0" class="font-semibold text-err">-{{ diffStats.removed }}</span>
          </span>
        </div>
        <div class="flex items-center gap-2">
          <span class="rounded-full px-2 py-0.5 text-[10.5px]" :class="status.tint">
            {{ status.label }}
          </span>
          <Icon
            name="chevron-right"
            class="h-3.5 w-3.5 text-faint transition-transform"
            :class="expanded ? 'rotate-90' : ''"
          />
        </div>
      </header>

      <!-- Files list -->
      <div v-if="touched.length > 0" class="flex flex-col divide-y divide-line/30 px-3 py-1.5 text-[12px] select-text">
        <div
          v-for="file in touched"
          :key="file.path"
          class="flex items-center justify-between py-1 font-mono"
        >
          <div class="flex min-w-0 items-center gap-2 text-fg/90">
            <Icon name="file" class="h-3.5 w-3.5 shrink-0 text-faint select-none" />
            <span class="truncate">{{ file.path }}</span>
          </div>
          <span v-if="diffStats" class="ml-2 flex shrink-0 items-center gap-1 text-[11px] select-none">
            <span v-if="diffStats.added > 0" class="text-ok">+{{ diffStats.added }}</span>
            <span v-if="diffStats.removed > 0" class="text-err">-{{ diffStats.removed }}</span>
          </span>
        </div>
      </div>

      <!-- Expandable Diff -->
      <div v-if="expanded && view.body.kind === 'diff'" class="border-t border-line/40">
        <DiffView :diff="view.body.text" />
      </div>
      <pre
        v-else-if="expanded && view.body.text"
        class="max-h-80 overflow-auto whitespace-pre-wrap break-words border-t border-line/40 px-3 py-2 font-mono text-[11.5px] text-dim select-text"
      >{{ view.body.text }}</pre>
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
          <span class="rounded-full px-2 py-0.5 text-[10.5px]" :class="status.tint">
            {{ status.label }}
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
          <span class="rounded-full px-2 py-0.5 text-[10.5px]" :class="status.tint">
            {{ status.label }}
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
