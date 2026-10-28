<script setup lang="ts">
/**
 * Autocomplete dropdown menu for `@` file mentions in the Composer.
 */
import type { WorkspaceFileMatch } from "../bridge";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  files: WorkspaceFileMatch[];
  selected: number;
  query: string;
}>();

const emit = defineEmits<{
  (event: "pick", index: number): void;
  (event: "hover", index: number): void;
}>();
</script>

<template>
  <div
    class="mb-2 max-h-72 overflow-auto rounded-[8px] border border-line/50 bg-canvas/90 backdrop-blur-md p-1.5 shadow-lg shadow-black/30"
    role="dialog"
    aria-label="file mentions"
  >
    <p class="flex items-center justify-between px-2 pb-1 pt-1.5 text-[10.5px] font-medium uppercase tracking-[0.09em] text-faint">
      <span class="flex items-center gap-1.5">
        <Icon name="file" class="h-3 w-3 text-accent" />
        <span>Files</span>
      </span>
      <span v-if="props.query" class="font-mono text-[10px] normal-case text-dim">
        @{{ props.query }}
      </span>
    </p>

    <div v-if="props.files.length === 0" class="px-3 py-4 text-center text-[12px] text-faint">
      No files matching “{{ props.query }}”
    </div>

    <div v-else class="flex flex-col gap-0.5">
      <button
        v-for="(file, index) in props.files"
        :key="file.path"
        type="button"
        class="flex w-full items-center gap-2.5 rounded-[6px] px-2.5 py-1.5 text-left transition-colors"
        :class="index === props.selected ? 'bg-selected text-fg font-medium' : 'text-dim hover:bg-raised/70 hover:text-fg'"
        @click="emit('pick', index)"
        @mousemove="emit('hover', index)"
      >
        <Icon name="file" class="h-3.5 w-3.5 shrink-0 text-faint" />
        <span class="shrink-0 text-[12.5px] font-medium text-fg">{{ file.name }}</span>
        <span class="min-w-0 flex-1 truncate font-mono text-[11px] text-faint">
          {{ file.path }}
        </span>
      </button>
    </div>

    <p class="px-2 pb-0.5 pt-1.5 text-[10.5px] text-faint border-t border-line/20 mt-1">
      ↑↓ choose · enter / tab inserts · esc closes
    </p>
  </div>
</template>
