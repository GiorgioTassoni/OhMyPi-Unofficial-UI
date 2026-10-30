<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import logo from "../assets/ohmypi-logo.svg";
import WindowResizeHandles from "./WindowResizeHandles.vue";

const props = defineProps<{
  project?: string | null;
  title?: string | null;
}>();

const STORAGE_KEY = "omp_window_maximized";
const isMaximized = ref(false);
let unlistenResized: (() => void) | null = null;
let lastToggleTime = 0;
let lastClickTime = 0;
let lastClickX = 0;
let lastClickY = 0;

function getStoredMaximized(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

function setStoredMaximized(val: boolean) {
  try {
    localStorage.setItem(STORAGE_KEY, val ? "true" : "false");
  } catch {}
}

async function checkMaximized() {
  try {
    const max = await getCurrentWindow().isMaximized();
    isMaximized.value = max;
    setStoredMaximized(max);
  } catch {}
}

async function handleMinimize() {
  try {
    await getCurrentWindow().minimize();
  } catch {}
}

async function handleToggleMaximize() {
  const now = Date.now();
  if (now - lastToggleTime < 300) return;
  lastToggleTime = now;

  try {
    const win = getCurrentWindow();
    await win.toggleMaximize();
    const max = await win.isMaximized();
    isMaximized.value = max;
    setStoredMaximized(max);
  } catch {}
}

function handleHeaderMouseDown(e: MouseEvent) {
  if (e.button !== 0) return;
  if ((e.target as HTMLElement)?.closest("button")) return;

  const now = Date.now();
  const dt = now - lastClickTime;
  const dx = Math.abs(e.clientX - lastClickX);
  const dy = Math.abs(e.clientY - lastClickY);

  if (dt < 350 && dx < 10 && dy < 10) {
    lastClickTime = 0;
    e.preventDefault();
    e.stopPropagation();
    void handleToggleMaximize();
  } else {
    lastClickTime = now;
    lastClickX = e.clientX;
    lastClickY = e.clientY;
  }
}

async function handleClose() {
  try {
    await getCurrentWindow().close();
  } catch {}
}

onMounted(async () => {
  try {
    const win = getCurrentWindow();
    if (getStoredMaximized()) {
      await win.maximize();
    }
  } catch {}

  await checkMaximized();

  try {
    const win = getCurrentWindow();
    unlistenResized = await win.onResized(async () => {
      try {
        const isMin = await win.isMinimized();
        if (isMin) return;
        const max = await win.isMaximized();
        isMaximized.value = max;
        setStoredMaximized(max);
      } catch {}
    });
  } catch {}
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
    @mousedown.capture="handleHeaderMouseDown"
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
