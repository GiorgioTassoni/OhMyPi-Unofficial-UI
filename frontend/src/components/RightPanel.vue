<script setup lang="ts">
/**
 * The right column (`docs/12` §8): the active thread's plan and its files.
 *
 * The panel belongs to the window but reads the *active* thread, because that is the one whose
 * conversation is on screen — and every row that names a transcript position jumps into the
 * column it belongs to, rather than opening a second copy of the same conversation.
 *
 * The reference sketch's `All` switch (plans from every open thread at once) is deliberately not
 * built in v1: `docs/12` §8.1 describes the per-thread plan, and a merged list needs its own
 * design for grouping and for jumping between columns.
 */
import { ref } from "vue";
import type { RowSnapshot, TodoPhaseSnapshot } from "../bridge";
import FilesPanel from "./FilesPanel.vue";
import TodosPanel from "./TodosPanel.vue";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  thread: string | null;
  /** The thread's name, so the panel says whose plan this is. */
  label: string | null;
  phases: TodoPhaseSnapshot[];
  rows: RowSnapshot[];
  cwd: string | null;
  live: boolean;
  busy?: boolean;
  workspace?: string | null;
  terminals?: boolean;
  diagnostics?: boolean;
}>();

const emit = defineEmits<{
  jump: [row: number];
  written: [];
  failed: [message: string];
  close: [];
  newThread: [];
  toggleTerminals: [];
  toggleDiagnostics: [];
}>();

/** `docs/12` §8 opens on the plan, which is the sketch's order. */
const tab = ref<"todos" | "files">("todos");

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
      <div class="flex items-center gap-1 min-w-0">
        <button
          v-for="name in (['todos', 'files'] as const)"
          :key="name"
          class="rounded-[6px] px-2 py-1 text-[12.5px] transition-colors"
          :class="tab === name ? 'bg-raised/90 text-fg font-medium shadow-sm' : 'text-dim hover:text-fg hover:bg-raised/40'"
          :aria-pressed="tab === name"
          @click="tab = name"
        >
          {{ name === "todos" ? "Todos" : "Files" }}
          <span v-if="name === 'todos' && props.phases.length" class="ml-1 rounded-full bg-surface px-1.5 py-0.5 font-mono text-[10px] text-accent">
            {{ props.phases.length }}
          </span>
        </button>
      </div>

      <div class="ml-auto flex items-center gap-0.5 shrink-0">
        <button
          :class="BUTTON"
          :disabled="props.busy || props.workspace === null"
          :title="
            props.workspace === null
              ? 'no project open — add one in the sidebar'
              : `new thread in ${props.workspace}`
          "
          aria-label="new thread"
          data-action="new-thread"
          type="button"
          @click="emit('newThread')"
        >
          <Icon name="plus" />
        </button>
        <button
          :class="[BUTTON, props.terminals ? 'bg-raised text-accent hover:text-accent' : '']"
          :disabled="props.workspace === null"
          title="the terminal panel: your own shell in this workspace, not the agent's"
          aria-label="terminal"
          data-action="terminal"
          type="button"
          @click="emit('toggleTerminals')"
        >
          <Icon name="terminal" />
        </button>
        <button
          :class="[BUTTON, 'bg-raised text-accent hover:text-accent']"
          title="the thread panel: its plan and the files it changed"
          aria-label="thread panel"
          data-action="panel"
          type="button"
          @click="emit('close')"
        >
          <Icon name="panels" />
        </button>
        <button
          :class="[BUTTON, props.diagnostics ? 'bg-raised text-accent hover:text-accent' : '']"
          title="the M0 readouts: handshake, process, stream counters, event tail"
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
        v-else
        :thread="props.thread"
        :rows="props.rows"
        :cwd="props.cwd"
        :live="props.live"
        @jump="emit('jump', $event)"
        @failed="emit('failed', $event)"
      />
    </template>
  </aside>
</template>
