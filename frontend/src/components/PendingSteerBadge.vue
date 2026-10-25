<script setup lang="ts">
import Icon from "./ui/Icon.vue";

defineProps<{
  text: string;
}>();
</script>

<template>
  <div class="flex flex-col items-end gap-1.5 my-3 animate-pulse">
    <div
      class="inline-flex items-center gap-1.5 rounded-full border border-amber-500/30 bg-amber-500/10 px-2.5 py-0.5 text-[11px] font-medium tracking-wider text-amber-500 uppercase select-none shadow-sm"
    >
      <Icon name="refresh" class="h-3 w-3 animate-spin text-amber-500" />
      <span>Injecting steer…</span>
    </div>
    <div class="group/message flex max-w-[80%] min-w-0 flex-col items-end gap-1.5">
      <p
        class="whitespace-pre-wrap break-words rounded-[16px] bg-raised/95 border border-amber-500/40 ring-1 ring-amber-500/20 px-4 py-2.5 text-[13px] leading-relaxed text-fg shadow-sm select-text opacity-90"
      >
        {{ text }}
      </p>
    </div>
  </div>
</template>
