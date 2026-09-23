<script setup lang="ts">
/** One completed turn's file changes, derived from its successful tool calls. */
import type { TurnFileSummary } from "../lib/panel";
import { fileReview, relativeTo } from "../lib/panel";
import { computed, ref } from "vue";
import DiffView from "./DiffView.vue";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  summary: TurnFileSummary;
  workspace: string | null;
  sidebar?: boolean;
}>();

const emit = defineEmits<{ jump: [row: number]; review: [summary: TurnFileSummary] }>();
const openFiles = ref<string[]>(props.sidebar ? props.summary.files.map((file) => file.path) : []);
const reviews = computed(() => new Map(
  props.summary.files.map((file) => [file.path, fileReview(props.summary, file.path)]),
));

function toggleFile(path: string): void {
  openFiles.value = openFiles.value.includes(path)
    ? openFiles.value.filter((open) => open !== path)
    : [...openFiles.value, path];
}
</script>

<template>
  <article class="overflow-hidden rounded-[9px] border border-line/70 bg-surface/60" data-turn-files>
    <div class="flex items-center gap-2.5 border-b border-line/50 px-3 py-2.5">
      <span class="grid size-7 shrink-0 place-items-center rounded-[6px] bg-raised text-dim">
        <Icon name="file" class="h-3.5 w-3.5" />
      </span>
      <span class="min-w-0 flex-1 text-[12.5px] font-medium text-fg">
        Changed {{ props.summary.files.length }} file{{ props.summary.files.length === 1 ? "" : "s" }}
      </span>
      <span v-if="props.summary.lines" class="shrink-0 font-mono text-[11px]">
        <span class="text-ok">+{{ props.summary.lines.added }}</span>
        <span class="ml-1 text-err">-{{ props.summary.lines.removed }}</span>
      </span>
      <button
        v-if="!props.sidebar"
        type="button"
        class="shrink-0 rounded-[6px] px-2 py-1 text-[11.5px] text-dim transition-colors hover:bg-raised hover:text-fg focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent"
        title="Review this turn's changes in the Files panel"
        @click="emit('review', props.summary)"
      >
        Review
      </button>
    </div>

    <div class="py-1">
      <div v-for="file in props.summary.files" :key="file.path">
        <button
          type="button"
          class="flex w-full items-center gap-3 px-3 py-1.5 text-left transition-colors hover:bg-raised/70 focus-visible:bg-raised/70 focus-visible:outline-none"
          :title="`Show every reported change to ${file.path}`"
          :aria-expanded="openFiles.includes(file.path)"
          @click="toggleFile(file.path)"
        >
          <span class="min-w-0 flex-1 truncate font-mono text-[11.5px] text-dim">
            {{ relativeTo(file.path, props.workspace) }}
          </span>
          <span v-if="file.lines" class="shrink-0 font-mono text-[10.5px]">
            <span class="text-ok">+{{ file.lines.added }}</span>
            <span class="ml-1 text-err">-{{ file.lines.removed }}</span>
          </span>
          <Icon
            name="chevron-right"
            class="h-3 w-3 shrink-0 text-faint transition-transform"
            :class="openFiles.includes(file.path) ? 'rotate-90' : ''"
          />
        </button>

        <div v-if="openFiles.includes(file.path)" class="border-y border-line/40 bg-canvas/20">
          <DiffView v-if="reviews.get(file.path)?.pieces.length" :pieces="reviews.get(file.path)!.pieces" />
          <p v-if="reviews.get(file.path)?.includesBatch" class="px-3 py-1.5 text-[11px] text-faint">
            This includes a batch diff that may also show other files.
          </p>
          <div v-if="reviews.get(file.path)?.withoutDiff.length" class="flex flex-wrap items-center gap-1.5 px-3 py-2 text-[11px] text-faint">
            <span>Some changes have no displayable diff:</span>
            <button
              v-for="row in reviews.get(file.path)?.withoutDiff"
              :key="row"
              type="button"
              class="rounded-[5px] px-1.5 py-0.5 text-dim hover:bg-raised hover:text-fg"
              @click="emit('jump', row)"
            >
              view tool call
            </button>
          </div>
        </div>
      </div>
    </div>
  </article>
</template>
