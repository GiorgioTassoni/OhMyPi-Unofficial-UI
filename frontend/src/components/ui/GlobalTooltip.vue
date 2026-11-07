<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { attachGlobalTooltipHandler, type TooltipState } from "../../lib/tooltip";

const state = ref<TooltipState>({
  visible: false,
  text: "",
  top: 0,
  left: 0,
  placement: "bottom",
});

let cleanup: (() => void) | null = null;

onMounted(() => {
  cleanup = attachGlobalTooltipHandler((next) => {
    state.value = next;
  });
});

onUnmounted(() => {
  if (cleanup) {
    cleanup();
    cleanup = null;
  }
});
</script>

<template>
  <Teleport to="body">
    <Transition
      enter-active-class="transition duration-100 ease-out"
      enter-from-class="opacity-0 scale-95"
      enter-to-class="opacity-100 scale-100"
      leave-active-class="transition duration-75 ease-in"
      leave-from-class="opacity-100 scale-100"
      leave-to-class="opacity-0 scale-95"
    >
      <div
        v-if="state.visible && state.text"
        class="fixed z-[9999] pointer-events-none select-none max-w-xs break-words rounded-[6px] border border-line-strong/60 bg-raised/95 px-2.5 py-1 text-[11.5px] font-medium leading-snug text-fg shadow-lg shadow-black/60 backdrop-blur-md"
        :style="{
          top: `${state.top}px`,
          left: `${state.left}px`,
        }"
        role="tooltip"
        aria-hidden="true"
      >
        {{ state.text }}
      </div>
    </Transition>
  </Teleport>
</template>
