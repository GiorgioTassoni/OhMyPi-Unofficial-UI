<script setup lang="ts">
/**
 * Closing overlay shown during application shutdown.
 * Provides instant visual feedback with a backdrop blur and spinner
 * while the backend terminates live sessions, closes terminals, and releases brokers.
 */
import logo from "../assets/ohmypi-logo.svg";

defineProps<{
  active: boolean;
}>();
</script>

<template>
  <Transition
    enter-active-class="transition-opacity duration-200 ease-out"
    enter-from-class="opacity-0"
    enter-to-class="opacity-100"
    leave-active-class="transition-opacity duration-150 ease-in"
    leave-from-class="opacity-100"
    leave-to-class="opacity-0"
  >
    <div
      v-if="active"
      class="fixed inset-0 z-[9999] flex flex-col items-center justify-center bg-canvas/75 backdrop-blur-md select-none"
      role="status"
      aria-live="polite"
      aria-label="Closing application"
    >
      <div
        class="flex flex-col items-center gap-4 rounded-2xl border border-line/50 bg-surface/90 px-8 py-6 shadow-2xl backdrop-blur-xl"
      >
        <!-- Spinner ring with pulsing logo -->
        <div class="relative flex items-center justify-center">
          <img :src="logo" alt="Oh My Pi" class="h-8 w-8 animate-pulse pointer-events-none" />
          <div
            class="absolute -inset-2.5 rounded-full border-2 border-accent/20 border-t-accent animate-spin"
            aria-hidden="true"
          />
        </div>

        <!-- Reassuring status feedback -->
        <div class="flex flex-col items-center gap-1 text-center">
          <h3 class="text-[13.5px] font-semibold text-fg tracking-tight">Closing Oh My Pi</h3>
          <p class="text-[11.5px] text-faint">Safely disconnecting sessions and background tasks...</p>
        </div>
      </div>
    </div>
  </Transition>
</template>
