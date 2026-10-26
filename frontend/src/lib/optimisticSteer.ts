import { ref, watch, type Ref } from "vue";
import type { RowSnapshot } from "../bridge";

export interface PendingSteerState {
  text: string;
}

export function useOptimisticSteer(
  rows: Ref<readonly RowSnapshot[]>,
  onNewSteer?: () => void,
) {
  const pendingSteer = ref<PendingSteerState | null>(null);

  function setSteering(payload: { text: string }): void {
    if (!payload.text) {
      pendingSteer.value = null;
    } else {
      pendingSteer.value = { text: payload.text };
      onNewSteer?.();
    }
  }

  watch(rows, (newRows) => {
    if (!pendingSteer.value) return;
    const steerText = pendingSteer.value.text;
    for (let i = newRows.length - 1; i >= 0; i--) {
      const r = newRows[i];
      if (r.role === "user") {
        if (r.steering || (steerText && r.text.includes(steerText))) {
          pendingSteer.value = null;
        }
        break;
      }
    }
  });

  return {
    pendingSteer,
    setSteering,
  };
}
