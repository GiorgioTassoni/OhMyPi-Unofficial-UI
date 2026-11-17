import { ref, onMounted, onUnmounted, watch, type Ref } from "vue";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { UnlistenFn } from "@tauri-apps/api/event";

export interface DragDropOptions {
  active: Ref<boolean>;
  targetRef: Ref<HTMLElement | null>;
  onFiles: (files: File[]) => void | Promise<void>;
  onPaths: (paths: readonly string[]) => void | Promise<void>;
  onError?: (cause: unknown) => void;
}

export interface DropPoint {
  x: number;
  y: number;
}

export interface DropTargetRect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export type NativeDropEvent =
  | { type: "leave" }
  | { type: "enter" | "over"; position: DropPoint }
  | { type: "drop"; position: DropPoint; paths: readonly string[] };

export interface DropContext {
  active: boolean;
  target: DropTargetRect | null;
  pixelsPerCss: number;
}

const reportsDevicePixels = /Windows/i.test(navigator.userAgent);

export function nativeDropIntent(
  event: NativeDropEvent,
  context: DropContext,
): { hovering: boolean; paths: readonly string[] } {
  if (!context.active || event.type === "leave") return { hovering: false, paths: [] };
  const x = event.position.x / context.pixelsPerCss;
  const y = event.position.y / context.pixelsPerCss;
  const target = context.target;
  const inside =
    target === null ||
    (x >= target.left && x <= target.right && y >= target.top && y <= target.bottom);
  if (event.type !== "drop") return { hovering: inside, paths: [] };
  return { hovering: false, paths: inside ? event.paths : [] };
}

export function useViewDragDrop(options: DragDropOptions) {
  const isDraggingOver = ref(false);
  let dragCounter = 0;
  let stopDragDropListening: UnlistenFn | null = null;
  let disposed = false;

  function onDragEnter(e: DragEvent): void {
    if (isTauri()) return;
    if (e.dataTransfer?.types?.includes("Files")) {
      dragCounter++;
      isDraggingOver.value = true;
    }
  }

  function onDragOver(e: DragEvent): void {
    if (isTauri()) return;
    if (e.dataTransfer?.types?.includes("Files")) {
      e.dataTransfer.dropEffect = "copy";
      isDraggingOver.value = true;
    }
  }

  function onDragLeave(): void {
    if (isTauri()) return;
    dragCounter = Math.max(0, dragCounter - 1);
    if (dragCounter === 0) {
      isDraggingOver.value = false;
    }
  }

  async function onHtmlDrop(e: DragEvent): Promise<void> {
    if (isTauri()) return;
    dragCounter = 0;
    isDraggingOver.value = false;
    const files = e.dataTransfer?.files;
    if (files && files.length > 0) {
      await options.onFiles(Array.from(files));
    }
  }

  watch(options.active, (active) => {
    if (!active) isDraggingOver.value = false;
  });

  onMounted(async () => {
    try {
      const stop = await getCurrentWebview().onDragDropEvent(({ payload }) => {
        if (disposed || !options.active.value) {
          isDraggingOver.value = false;
          return;
        }
        if (payload.type === "leave") {
          isDraggingOver.value = false;
          return;
        }
        const intent = nativeDropIntent(payload, {
          active: true,
          target: options.targetRef.value?.getBoundingClientRect() ?? null,
          pixelsPerCss: reportsDevicePixels ? window.devicePixelRatio || 1 : 1,
        });
        isDraggingOver.value = intent.hovering;
        if (intent.paths.length > 0) void options.onPaths(intent.paths);
      });
      if (disposed) stop();
      else stopDragDropListening = stop;
    } catch (cause) {
      if (isTauri()) options.onError?.(cause);
    }
  });

  onUnmounted(() => {
    disposed = true;
    isDraggingOver.value = false;
    stopDragDropListening?.();
    stopDragDropListening = null;
  });

  return {
    isDraggingOver,
    onDragEnter,
    onDragOver,
    onDragLeave,
    onHtmlDrop,
  };
}
