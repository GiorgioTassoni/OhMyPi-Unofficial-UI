<script setup lang="ts">
import Icon from "./ui/Icon.vue";

defineProps<{
  active: boolean;
}>();
</script>

<template>
  <Transition
    enter-active-class="transition duration-150 ease-out"
    enter-from-class="opacity-0 scale-[0.98]"
    enter-to-class="opacity-100 scale-100"
    leave-active-class="transition duration-100 ease-in"
    leave-from-class="opacity-100 scale-100"
    leave-to-class="opacity-0 scale-[0.98]"
  >
    <div
      v-if="active"
      class="pointer-events-none absolute inset-3 z-30 flex flex-col items-center justify-center rounded-2xl border-2 border-dashed border-accent/70 bg-canvas/85 backdrop-blur-sm shadow-2xl"
    >
      <div class="flex flex-col items-center gap-3 p-6 text-center select-none">
        <div class="grid h-12 w-12 place-items-center rounded-full bg-accent/15 text-accent shadow-sm">
          <Icon name="plus" class="h-6 w-6" />
        </div>
        <div>
          <p class="text-[14px] font-semibold text-fg">Drop files or images here</p>
          <p class="text-[12px] text-faint">Attach to the current conversation</p>
        </div>
      </div>
    </div>
  </Transition>
</template>
