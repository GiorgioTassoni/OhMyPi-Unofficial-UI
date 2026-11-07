<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { RowSnapshot, TodoPhaseSnapshot } from "../bridge";
import type { ChatSubagent } from "../lib/agents";
import type { TurnReviewRequest } from "../lib/panel";
import FileViewerPanel, { type FileViewRequest } from "./FileViewerPanel.vue";
import FilesPanel from "./FilesPanel.vue";
import ThreadSubagentsPanel from "./ThreadSubagentsPanel.vue";
import TodosPanel from "./TodosPanel.vue";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  thread: string | null;
  label: string | null;
  phases: TodoPhaseSnapshot[];
  rows: RowSnapshot[];
  cwd: string | null;
  live: boolean;
  review: TurnReviewRequest | null;
  workspace?: string | null;
  terminals?: boolean;
  diagnostics?: boolean;
  subagents?: ChatSubagent[];
  activeAgentId?: string | null | undefined;
  fileRequest?: FileViewRequest | null | undefined;
}>();

const emit = defineEmits<{
  jump: [row: number];
  written: [];
  failed: [message: string];
  close: [];
  closeReview: [];
  toggleTerminals: [];
  toggleDiagnostics: [];
  openAgent: [thread: string, id: string];
  closeFileViewer: [];
  openFile: [file: { path: string; content?: string; artifactId?: string | null }];
}>();

const tab = ref<"todos" | "files" | "subagents" | "viewer">("todos");

const activeSubagentsCount = computed(() => (props.subagents ?? []).filter((s) => s.running).length);
const activeTodosCount = computed(() =>
  props.phases.reduce(
    (sum, p) => sum + p.tasks.filter((t) => t.status !== "completed" && t.status !== "abandoned").length,
    0
  )
);

watch(
  () => props.review?.id,
  (id) => {
    if (id !== undefined) tab.value = "files";
  },
  { immediate: true },
);

watch(
  () => props.fileRequest,
  (req) => {
    if (req) tab.value = "viewer";
  },
  { immediate: true },
);

watch(
  () => props.thread,
  () => {
    if (tab.value === "viewer" && !props.fileRequest) {
      tab.value = "todos";
    }
  },
);

const BUTTON =
  "grid h-7 w-7 place-items-center rounded-[6px] text-dim transition-colors hover:bg-raised hover:text-fg disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-dim";
</script>

<template>
  <!--
    The column is part of the window rather than a card inside it: one hairline against the
    transcript, and from there the tab strip and the list are the only chrome.
  -->
  <aside class="flex w-full h-full shrink-0 flex-col border-l border-line/40 bg-canvas" aria-label="thread panel">
    <!--
      The top bar matches TitleBar and Sidebar height (h-10 / 40px) and carries data-tauri-drag-region
      so the entire top window strip is continuous and draggable.
    -->
    <header
      class="group flex h-10 shrink-0 items-center justify-between border-b border-line/40 px-2.5 select-none"
      data-tauri-drag-region
    >
      <div class="flex items-center gap-0.5 min-w-0">
        <button
          v-for="name in (['todos', 'files', 'subagents', 'viewer'] as const)"
          :key="name"
          class="rounded-[6px] px-2 py-1 text-[12px] transition-colors"
          :class="tab === name ? 'bg-raised/90 text-fg font-medium shadow-sm' : 'text-dim hover:text-fg hover:bg-raised/40'"
          :aria-pressed="tab === name"
          @click="tab = name"
        >
          {{ name === "todos" ? "Todos" : name === "files" ? "Files" : name === "subagents" ? "Subagents" : "Viewer" }}
          <span v-if="name === 'todos' && activeTodosCount > 0" class="ml-1 rounded-full bg-surface px-1.5 py-0.5 font-mono text-[10px] text-accent">
            {{ activeTodosCount }}
          </span>
          <span
            v-else-if="name === 'subagents' && props.subagents && props.subagents.length > 0"
            class="ml-1 rounded-full bg-surface px-1.5 py-0.5 font-mono text-[10px]"
            :class="activeSubagentsCount > 0 ? 'text-accent font-semibold' : 'text-faint'"
          >
            {{ activeSubagentsCount > 0 ? activeSubagentsCount : props.subagents.length }}
          </span>
          <span
            v-else-if="name === 'viewer' && props.fileRequest"
            class="ml-1.5 inline-block h-1.5 w-1.5 rounded-full bg-accent"
            title="File open"
          />
        </button>
      </div>

      <div class="ml-auto flex items-center gap-0.5 shrink-0">
        <button
          :class="[BUTTON, props.terminals ? 'bg-raised text-accent hover:text-accent' : '']"
          :disabled="props.workspace === null"
          aria-label="terminal"
          data-action="terminal"
          type="button"
          @click="emit('toggleTerminals')"
        >
          <Icon name="terminal" />
        </button>
        <button
          :class="[BUTTON, 'bg-raised text-accent hover:text-accent']"
          aria-label="thread panel"
          data-action="panel"
          type="button"
          @click="emit('close')"
        >
          <Icon name="panels" />
        </button>
        <button
          :class="[BUTTON, props.diagnostics ? 'bg-raised text-accent hover:text-accent' : '']"
          aria-label="diagnostics"
          data-action="diagnostics"
          type="button"
          @click="emit('toggleDiagnostics')"
        >
          <Icon name="sliders" />
        </button>
      </div>
    </header>

    <div v-if="props.thread === null" class="flex min-h-0 flex-1 flex-col items-center justify-center p-6">
      <p class="max-w-sm text-center text-[12.5px] leading-relaxed text-faint">
        Open a thread to see its plan and the files it changed.
      </p>
    </div>

    <template v-else>
      <TodosPanel
        v-if="tab === 'todos'"
        :thread="props.thread"
        :phases="props.phases"
        :live="props.live"
        :rows="props.rows"
        @jump="emit('jump', $event)"
        @written="emit('written')"
        @failed="emit('failed', $event)"
      />
      <FilesPanel
        v-else-if="tab === 'files'"
        :key="props.review?.id ?? 'session-files'"
        :thread="props.thread"
        :rows="props.rows"
        :cwd="props.cwd"
        :live="props.live"
        :review="props.review"
        @jump="emit('jump', $event)"
        @failed="emit('failed', $event)"
        @close-review="emit('closeReview')"
        @open-file="emit('openFile', $event)"
      />
      <ThreadSubagentsPanel
        v-else-if="tab === 'subagents'"
        :thread="props.thread"
        :subagents="props.subagents ?? []"
        :active-agent-id="props.activeAgentId ?? null"
        :live="props.live"
        @open-agent="(thread, id) => emit('openAgent', thread, id)"
      />
      <div
        v-else-if="tab === 'viewer'"
        class="flex min-h-0 flex-1 flex-col overflow-hidden"
      >
        <FileViewerPanel
          v-if="props.fileRequest"
          :file="props.fileRequest"
          :thread="props.thread"
          :workspace="props.workspace"
          @close="emit('closeFileViewer')"
          @failed="emit('failed', $event)"
        />
        <div
          v-else
          class="flex min-h-0 flex-1 flex-col items-center justify-center p-6 text-center select-none"
        >
          <div class="flex h-12 w-12 items-center justify-center rounded-2xl border border-line/50 bg-surface/70 text-dim shadow-xs">
            <Icon name="file" class="h-5 w-5 text-accent/80" />
          </div>
          <h3 class="mt-3.5 text-[13.5px] font-semibold tracking-tight text-fg">
            Nothing to show here
          </h3>
          <p class="mt-1.5 max-w-[240px] text-[12px] leading-relaxed text-faint">
            No file or artifact is currently open for inspection. Open a file from the conversation or the Files tab to preview it here.
          </p>
          <button
            type="button"
            class="mt-4 inline-flex items-center gap-1.5 rounded-[6px] border border-line/60 bg-raised/40 px-3 py-1.5 text-[11.5px] font-medium text-dim transition-colors hover:border-line-strong hover:bg-raised hover:text-fg shadow-xs"
            @click="tab = 'files'"
          >
            <Icon name="folder" class="h-3.5 w-3.5" />
            <span>Browse session files</span>
          </button>
        </div>
      </div>
    </template>
  </aside>
</template>
