<script setup lang="ts">
/**
 * The custom titlebar (`docs/12` §1, decision D8: no native menu bar).
 *
 * It carries the drag region, the project on screen, and the window's own controls — all icons,
 * which is what the reference does and what stops the top of a desktop window reading like a
 * form. Window controls are the OS's own: the decorations stay on, which is why nothing here
 * re-implements minimise and close.
 *
 * The directory field this used to hold is gone. "Open somewhere new" is a navigation act, and
 * it lives with the projects now (the sidebar's `+`, `docs/12` §2.1); a text box in the titlebar
 * was scaffolding from the first runnable shell, and the reference has nothing there at all.
 */
import { computed } from "vue";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  /** Where a new thread would land; with none, there is nowhere to start one. */
  workspace: string | null;
  busy: boolean;
  /** The project the active thread belongs to, for the label. */
  project: string | null;
  /** Active thread title if one is open */
  title?: string | null;
  /** Whether the left sidebar is currently collapsed */
  sidebarCollapsed?: boolean;
  /** Whether the right column is showing the thread panel (`docs/12` §1). */
  panel: boolean;
  /** Whether the terminal drawer is on screen (`docs/12` §11). */
  terminals: boolean;
  /** Whether the M0 readouts have taken the right column. */
  diagnostics: boolean;
}>();

const emit = defineEmits<{
  /** A new thread in the directory the window was last pointed at. */
  newThread: [];
  toggleSidebar: [];
  togglePanel: [];
  toggleTerminals: [];
  toggleDiagnostics: [];
}>();

/** Clean display name for the project (basename), while keeping the full path in the tooltip */
const projectName = computed(() => {
  if (!props.project) return "";
  const segments = props.project.split(/[/\\]/).filter(Boolean);
  return segments.pop() || props.project;
});

/** The shared button chrome: a quiet icon that fills in on hover, tinted while it is on. */
const BUTTON =
  "grid h-7 w-7 place-items-center rounded-[6px] text-dim transition-colors hover:bg-raised hover:text-fg disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-dim";
</script>

<template>
  <header
    class="flex h-10 shrink-0 items-center justify-between border-b border-line/40 px-3.5 select-none"
    data-tauri-drag-region
  >
    <div class="flex items-center gap-2 min-w-0">
      <button
        v-if="props.sidebarCollapsed"
        :class="BUTTON"
        title="show sidebar"
        aria-label="show sidebar"
        data-action="toggle-sidebar"
        type="button"
        @click="emit('toggleSidebar')"
      >
        <Icon name="panels" />
      </button>
      <span
        v-if="props.title"
        class="truncate text-[13px] font-medium text-fg"
        :title="props.title"
      >
        {{ props.title }}
      </span>
      <span
        v-if="props.project"
        class="truncate text-[12px]"
        :class="props.title ? 'text-faint' : 'text-[13px] font-medium text-fg/90'"
        :title="props.project"
        data-project
      >
        {{ projectName }}
      </span>
    </div>

    <div class="ml-auto flex items-center gap-0.5">
      <!-- When collapsed, show only the button to reopen it -->
      <button
        v-if="!props.panel && !props.diagnostics"
        :class="BUTTON"
        title="the thread panel: its plan and the files it changed"
        aria-label="thread panel"
        data-action="panel"
        type="button"
        @click="emit('togglePanel')"
      >
        <Icon name="panels" />
      </button>
    </div>
  </header>
</template>
