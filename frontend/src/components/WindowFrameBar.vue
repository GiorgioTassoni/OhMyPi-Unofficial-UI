<script setup lang="ts">
/**
 * Custom integrated window frame bar for frameless mode (decorations: false).
 * Provides window title, workspace/project pill, window dragging (data-tauri-drag-region),
 * double-click to maximize, and custom minimize, maximize/restore, and close buttons.
 */
import { onMounted, onUnmounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import logo from "../assets/ohmypi-logo.svg";
import WindowResizeHandles from "./WindowResizeHandles.vue";

const props = defineProps<{
  project?: string | null;
  title?: string | null;
}>();

const isMaximized = ref(false);
let unlistenResized: (() => void) | null = null;

async function checkMaximized() {
  try {
    isMaximized.value = await getCurrentWindow().isMaximized();
  } catch {
    // Non-desktop web fallback
  }
}

async function handleMinimize() {
  try {
    await getCurrentWindow().minimize();
  } catch {
    // Non-desktop web fallback
  }
}

async function handleToggleMaximize() {
  try {
    await getCurrentWindow().toggleMaximize();
    await checkMaximized();
  } catch {
    // Non-desktop web fallback
  }
}

async function handleClose() {
  try {
    await getCurrentWindow().close();
  } catch {
    // Non-desktop web fallback
  }
}

onMounted(async () => {
  await checkMaximized();
  try {
    const win = getCurrentWindow();
    unlistenResized = await win.onResized(async () => {
      await checkMaximized();
    });
  } catch {
    // Non-desktop web fallback
  }
});

onUnmounted(() => {
  if (unlistenResized) {
    unlistenResized();
    unlistenResized = null;
  }
});
</script>

<template>
  <header
    class="flex h-8 w-full shrink-0 items-center justify-between border-b border-line/40 bg-surface/90 px-2.5 text-dim backdrop-blur-md select-none z-40"
    data-tauri-drag-region
    @dblclick="handleToggleMaximize"
  >
    <!-- Left: App brand -->
    <div class="flex items-center gap-2 min-w-0" data-tauri-drag-region>
      <img :src="logo" alt="Oh My Pi" class="h-3.5 w-3.5 shrink-0 pointer-events-none" />
      <span class="text-[11.5px] font-semibold tracking-tight text-fg/80 pointer-events-none">
        Oh My Pi
      </span>
    </div>

    <!-- Center: Context (thread title or project) -->
    <div
      class="flex flex-1 items-center justify-center px-4 overflow-hidden min-w-0"
      data-tauri-drag-region
    >
      <div
        v-if="props.project || props.title"
        class="inline-flex max-w-sm items-center gap-1.5 truncate rounded-full border border-line/30 bg-raised/50 px-2.5 py-0.5 text-[11px] text-faint"
        data-tauri-drag-region
      >
        <span v-if="props.title" class="truncate font-medium text-fg/75">{{ props.title }}</span>
        <span v-if="props.title && props.project" class="text-faint/60">·</span>
        <span v-if="props.project" class="truncate text-dim/80">{{ props.project }}</span>
      </div>
    </div>

    <!-- Right: Window Controls (minimize, maximize, close) -->
    <div class="flex items-center gap-0.5 shrink-0">
      <!-- Minimize -->
      <button
        type="button"
        class="flex h-6 w-8 items-center justify-center rounded-[4px] text-dim transition-colors hover:bg-raised hover:text-fg focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent"
        aria-label="Minimize"
        title="Minimize"
        @click.stop="handleMinimize"
      >
        <svg class="h-3 w-3 fill-current" viewBox="0 0 12 12">
          <rect y="5.5" width="12" height="1" rx="0.5" />
        </svg>
      </button>

      <!-- Maximize / Restore -->
      <button
        type="button"
        class="flex h-6 w-8 items-center justify-center rounded-[4px] text-dim transition-colors hover:bg-raised hover:text-fg focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent"
        :aria-label="isMaximized ? 'Restore' : 'Maximize'"
        :title="isMaximized ? 'Restore' : 'Maximize'"
        @click.stop="handleToggleMaximize"
      >
        <!-- Restore icon (two overlapping squares) -->
        <svg v-if="isMaximized" class="h-3 w-3 fill-none stroke-current stroke-[1.1]" viewBox="0 0 12 12">
          <rect x="1.5" y="3.5" width="7" height="7" rx="0.5" />
          <path d="M3.5 3.5V2a.5.5 0 0 1 .5-.5h6a.5.5 0 0 1 .5.5v6a.5.5 0 0 1-.5.5H8.5" />
        </svg>
        <!-- Maximize icon (single square) -->
        <svg v-else class="h-3 w-3 fill-none stroke-current stroke-[1.1]" viewBox="0 0 12 12">
          <rect x="2" y="2" width="8" height="8" rx="0.5" />
        </svg>
      </button>

      <!-- Close -->
      <button
        type="button"
        class="flex h-6 w-8 items-center justify-center rounded-[4px] text-dim transition-colors hover:bg-err/90 hover:text-canvas focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-err"
        aria-label="Close"
        title="Close"
        @click.stop="handleClose"
      >
        <svg class="h-3 w-3 fill-none stroke-current stroke-[1.2]" viewBox="0 0 12 12">
          <path d="M2.5 2.5l7 7M9.5 2.5l-7 7" stroke-linecap="round" />
        </svg>
      </button>
    </div>
  </header>

  <!-- Native edge and corner resize hitzones for frameless window -->
  <WindowResizeHandles v-if="!isMaximized" />
</template>
