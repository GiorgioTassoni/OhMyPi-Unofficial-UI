<script setup lang="ts">
/**
 * Edge and corner hitzones for native window resizing on Linux/frameless windows.
 * When pointerdown is triggered on an edge, it invokes `startResizeDragging(direction)`.
 */
import { getCurrentWindow } from "@tauri-apps/api/window";

type ResizeDirection =
  | "East"
  | "North"
  | "NorthEast"
  | "NorthWest"
  | "South"
  | "SouthEast"
  | "SouthWest"
  | "West";

function onPointerDown(direction: ResizeDirection, e: PointerEvent) {
  // Only handle primary mouse click
  if (e.button !== 0) return;
  e.preventDefault();
  try {
    void getCurrentWindow().startResizeDragging(direction);
  } catch {
    // Fallback if not inside Tauri window
  }
}
</script>

<template>
  <div class="pointer-events-none fixed inset-0 z-50 overflow-hidden select-none">
    <!-- Edges -->
    <div
      class="pointer-events-auto absolute top-0 left-2 right-2 h-1 cursor-n-resize"
      @pointerdown="onPointerDown('North', $event)"
    />
    <div
      class="pointer-events-auto absolute bottom-0 left-2 right-2 h-1 cursor-s-resize"
      @pointerdown="onPointerDown('South', $event)"
    />
    <div
      class="pointer-events-auto absolute top-2 bottom-2 left-0 w-1 cursor-w-resize"
      @pointerdown="onPointerDown('West', $event)"
    />
    <div
      class="pointer-events-auto absolute top-2 bottom-2 right-0 w-1 cursor-e-resize"
      @pointerdown="onPointerDown('East', $event)"
    />

    <!-- Corners -->
    <div
      class="pointer-events-auto absolute top-0 left-0 h-2.5 w-2.5 cursor-nw-resize"
      @pointerdown="onPointerDown('NorthWest', $event)"
    />
    <div
      class="pointer-events-auto absolute top-0 right-0 h-2.5 w-2.5 cursor-ne-resize"
      @pointerdown="onPointerDown('NorthEast', $event)"
    />
    <div
      class="pointer-events-auto absolute bottom-0 left-0 h-2.5 w-2.5 cursor-sw-resize"
      @pointerdown="onPointerDown('SouthWest', $event)"
    />
    <div
      class="pointer-events-auto absolute bottom-0 right-0 h-2.5 w-2.5 cursor-se-resize"
      @pointerdown="onPointerDown('SouthEast', $event)"
    />
  </div>
</template>
