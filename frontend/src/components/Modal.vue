<script setup lang="ts">
/**
 * A modal, teleported like the popovers are.
 *
 * Modal rather than inline because both of its uses decide something irreversible-ish: which
 * user message to fork from, and whether to delete a session's file. `Escape` and a press
 * outside cancel, which is the safe direction for both.
 */
import { onMounted, onUnmounted } from "vue";

import Icon from "./ui/Icon.vue";

const props = defineProps<{
  title: string;
  /** Nothing to cancel *to* while work is in flight. */
  busy?: boolean;
}>();

const emit = defineEmits<{ close: [] }>();

function onKeydown(event: KeyboardEvent): void {
  if (event.key !== "Escape" || props.busy) return;
  event.preventDefault();
  event.stopPropagation();
  emit("close");
}

onMounted(() => document.addEventListener("keydown", onKeydown, true));
onUnmounted(() => document.removeEventListener("keydown", onKeydown, true));
</script>

<template>
  <Teleport to="body">
    <div
      class="fixed inset-0 z-40 flex items-center justify-center bg-black/50"
      @pointerdown.self="!props.busy && emit('close')"
    >
      <div
        role="dialog"
        :aria-label="props.title"
        class="flex max-h-[70vh] w-[32rem] flex-col overflow-hidden rounded-[10px] border border-line bg-surface shadow-2xl shadow-black/60"
      >
        <header class="flex items-center gap-2 px-4 pb-2 pt-3.5">
          <span class="text-[13px] text-fg">{{ props.title }}</span>
          <button
            class="ml-auto rounded-[6px] p-1 text-faint hover:bg-raised hover:text-fg"
            type="button"
            title="close"
            @click="emit('close')"
          >
            <Icon name="close" class="h-3.5 w-3.5" />
          </button>
        </header>
        <div class="min-h-0 flex-1 overflow-auto px-4 pb-4">
          <slot />
        </div>
      </div>
    </div>
  </Teleport>
</template>
