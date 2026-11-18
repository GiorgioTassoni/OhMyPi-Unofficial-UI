import { onUnmounted, ref, type Ref } from "vue";

export interface PaneResizeOptions {
  /** Whether the pane is currently open (`true`) or collapsed (`false`). */
  open: Ref<boolean>;
  /** Current pane width in pixels. */
  width: Ref<number>;
  /** Default width restored on double-click. */
  defaultWidth: number;
  minWidth: number;
  maxWidth: number;
  /** Maximum fraction of `window.innerWidth` the pane may occupy. */
  maxViewportRatio: number;
  collapseThreshold: number;
  /** Which edge the resize handle drags from (`"left"` measures `e.clientX`; `"right"` measures `window.innerWidth - e.clientX`). */
  side: "left" | "right";
}

/**
 * Shared pointer-drag resize controller for collapsible side panes.
 */
export function usePaneResize(options: PaneResizeOptions) {
  const isDragging = ref(false);
  let cleanupListeners: (() => void) | null = null;

  function stop(): void {
    isDragging.value = false;
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
    cleanupListeners?.();
    cleanupListeners = null;
  }

  function start(event: PointerEvent): void {
    event.preventDefault();
    stop();
    isDragging.value = true;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";

    const onPointerMove = (moveEvent: PointerEvent) => {
      if (!isDragging.value) return;
      const rawWidth =
        options.side === "left"
          ? moveEvent.clientX
          : window.innerWidth - moveEvent.clientX;
      if (rawWidth < options.collapseThreshold) {
        options.open.value = false;
      } else {
        options.open.value = true;
        const maxAllowed = Math.min(
          options.maxWidth,
          Math.floor(window.innerWidth * options.maxViewportRatio),
        );
        options.width.value = Math.max(options.minWidth, Math.min(rawWidth, maxAllowed));
      }
    };

    const onPointerUp = () => stop();

    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
    window.addEventListener("pointercancel", onPointerUp);
    cleanupListeners = () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
      window.removeEventListener("pointercancel", onPointerUp);
    };
  }

  function reset(): void {
    options.width.value = options.defaultWidth;
    options.open.value = true;
  }

  onUnmounted(stop);

  return { isDragging, start, reset };
}
