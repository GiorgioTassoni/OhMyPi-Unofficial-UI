import { ref, onMounted, onUnmounted, type Ref } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { UnlistenFn } from "@tauri-apps/api/event";

export interface DragDropOptions {
  active: Ref<boolean>;
  targetRef: Ref<HTMLElement | null>;
  onFiles: (files: File[]) => void | Promise<void>;
  onPaths: (paths: readonly string[]) => void | Promise<void>;
}

export function useViewDragDrop(options: DragDropOptions) {
  const isDraggingOver = ref(false);
  let dragCounter = 0;
  let stopDragDropListening: UnlistenFn | null = null;

  function onDragEnter(e: DragEvent): void {
    if (e.dataTransfer?.types?.includes("Files")) {
      dragCounter++;
      isDraggingOver.value = true;
    }
  }

  function onDragOver(e: DragEvent): void {
    if (e.dataTransfer?.types?.includes("Files")) {
      e.dataTransfer.dropEffect = "copy";
      isDraggingOver.value = true;
    }
  }

  function onDragLeave(): void {
    dragCounter = Math.max(0, dragCounter - 1);
    if (dragCounter === 0) {
      isDraggingOver.value = false;
    }
  }

  async function onHtmlDrop(e: DragEvent): Promise<void> {
    dragCounter = 0;
    isDraggingOver.value = false;
    const files = e.dataTransfer?.files;
    if (files && files.length > 0) {
      await options.onFiles(Array.from(files));
    }
  }

  onMounted(async () => {
    try {
      stopDragDropListening = await getCurrentWebview().onDragDropEvent((event) => {
        if (!options.active.value) return;
        const payload = event.payload;

        switch (payload.type) {
          case "enter":
          case "over": {
            const scale = window.devicePixelRatio || 1;
            const x = payload.position.x / scale;
            const y = payload.position.y / scale;
            const targetEl = options.targetRef.value;
            if (targetEl) {
              const rect = targetEl.getBoundingClientRect();
              isDraggingOver.value =
                x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
            } else {
              isDraggingOver.value = true;
            }
            break;
          }
          case "drop": {
            const wasOver = isDraggingOver.value;
            isDraggingOver.value = false;
            if (wasOver && payload.paths.length > 0) {
              void options.onPaths(payload.paths);
            }
            break;
          }
          case "leave":
            isDraggingOver.value = false;
            break;
        }
      });
    } catch {
      // Non-tauri or testing environment
    }
  });

  onUnmounted(() => {
    stopDragDropListening?.();
  });

  return {
    isDraggingOver,
    onDragEnter,
    onDragOver,
    onDragLeave,
    onHtmlDrop,
  };
}
