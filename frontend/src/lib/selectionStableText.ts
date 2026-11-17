import { computed, onBeforeUnmount, onMounted, ref, watch, type ComputedRef } from "vue";
import { hasSelectionWithin } from "./conversationSelection";

export function useSelectionStableText(
  latest: () => string,
  container: () => HTMLElement | null,
): ComputedRef<string> {
  const frozen = ref<string | null>(null);

  function releaseIfDeselected(): void {
    if (frozen.value !== null && !hasSelectionWithin(container())) {
      frozen.value = null;
    }
  }

  watch(
    latest,
    (_next, previous) => {
      if (!hasSelectionWithin(container())) {
        frozen.value = null;
      } else if (frozen.value === null) {
        frozen.value = previous;
      }
    },
    { flush: "sync" },
  );

  function attach(): void {
    document.addEventListener("selectionchange", releaseIfDeselected);
  }

  function detach(): void {
    document.removeEventListener("selectionchange", releaseIfDeselected);
  }

  onMounted(attach);
  onBeforeUnmount(() => {
    detach();
    frozen.value = null;
  });

  return computed(() => frozen.value ?? latest());
}
