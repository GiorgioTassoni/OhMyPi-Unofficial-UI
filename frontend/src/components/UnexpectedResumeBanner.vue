<script setup lang="ts">
/**
 * Informative banner shown when a resumed thread is idle at the conversation level,
 * but background subagents from a prior unexpected close/forced shutdown were revived
 * by the engine runtime.
 *
 * Provides a one-click dismiss ("Got it") or one-click termination ("Stop them").
 */
import { ref, watch } from "vue";
import { stopTurn } from "../bridge";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  thread: string | null;
  count: number;
}>();

const emit = defineEmits<{
  failed: [message: string];
}>();

const dismissed = ref(false);
const stopping = ref(false);

watch(
  () => props.thread,
  () => {
    dismissed.value = false;
    stopping.value = false;
  },
);

async function onStop(): Promise<void> {
  if (stopping.value || !props.thread) return;
  stopping.value = true;
  try {
    await stopTurn(props.thread);
    dismissed.value = true;
  } catch (error) {
    emit("failed", typeof error === "string" ? error : String(error));
  } finally {
    stopping.value = false;
  }
}
</script>

<template>
  <Transition
    enter-active-class="transition duration-150 ease-out"
    enter-from-class="opacity-0 -translate-y-1"
    enter-to-class="opacity-100 translate-y-0"
    leave-active-class="transition duration-100 ease-in"
    leave-from-class="opacity-100 translate-y-0"
    leave-to-class="opacity-0 -translate-y-1"
  >
    <div
      v-if="props.count > 0 && !dismissed"
      class="mb-2 flex items-center justify-between gap-3 rounded-lg border border-accent/30 bg-accent/10 px-3 py-2 text-[12px] text-fg shadow-xs"
      role="status"
      aria-live="polite"
    >
      <div class="flex items-center gap-2 min-w-0">
        <Icon name="bot" class="h-4 w-4 shrink-0 text-accent animate-pulse" />
        <span class="truncate">
          <strong class="font-medium">Unexpected close detected:</strong>
          this thread resumed {{ props.count }} background {{ props.count === 1 ? 'agent' : 'agents' }}.
        </span>
      </div>

      <div class="flex items-center gap-1.5 shrink-0">
        <button
          type="button"
          :disabled="stopping"
          class="rounded px-2.5 py-1 text-[11px] font-medium text-dim hover:text-fg hover:bg-raised/60 transition-colors disabled:opacity-50"
          @click="dismissed = true"
        >
          Got it
        </button>
        <button
          type="button"
          :disabled="stopping"
          class="rounded border border-err/40 bg-err/15 px-2.5 py-1 text-[11px] font-medium text-err hover:bg-err/25 transition-colors disabled:opacity-50"
          @click="onStop"
        >
          {{ stopping ? "stopping…" : "Stop them" }}
        </button>
      </div>
    </div>
  </Transition>
</template>
