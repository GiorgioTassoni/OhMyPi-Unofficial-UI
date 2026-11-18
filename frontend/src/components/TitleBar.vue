<script setup lang="ts">
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  workspace: string | null;
  busy: boolean;
  project?: string | null;
  title?: string | null;
  sidebarCollapsed?: boolean;
  panel: boolean;
  terminals: boolean;
  diagnostics: boolean;
}>();

const emit = defineEmits<{
  newThread: [];
  toggleSidebar: [];
  togglePanel: [];
  toggleTerminals: [];
  toggleDiagnostics: [];
}>();

const BUTTON =
  "grid h-7 w-7 place-items-center rounded-[6px] text-dim transition-colors hover:bg-raised hover:text-fg disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-dim";
</script>

<template>
  <header
    class="flex h-10 shrink-0 items-center justify-between border-b border-line/40 px-2.5 select-none gap-2 min-w-0"
  >
    <div v-if="props.sidebarCollapsed" class="flex items-center gap-2 shrink-0">
      <button
        :class="BUTTON"
        aria-label="show sidebar"
        data-action="toggle-sidebar"
        type="button"
        @click="emit('toggleSidebar')"
      >
        <Icon name="panels" />
      </button>
    </div>

    <div class="flex items-center min-w-0 max-w-full overflow-x-auto no-scrollbar gap-1 py-1">
      <slot />
    </div>

    <div class="h-full min-w-6 flex-1" data-tauri-drag-region />

    <div class="ml-auto flex items-center gap-0.5 shrink-0">
      <button
        v-if="!props.panel && !props.diagnostics"
        :class="BUTTON"
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
