<script setup lang="ts">
import Icon from "./ui/Icon.vue";

defineProps<{
  text: string;
}>();
</script>

<template>
  <div class="flex flex-col items-end gap-1 my-2">
    <div class="flex items-center gap-1.5 text-[11.5px] font-medium text-accent select-none">
      <Icon name="refresh" class="h-3 w-3 animate-spin text-accent" />
      <span>steering…</span>
    </div>
    <div class="flex max-w-[80%] min-w-0 flex-col items-end">
      <p
        class="whitespace-pre-wrap break-words rounded-[16px] bg-raised/95 border border-line/60 border-t-line-strong/40 px-4 py-2.5 text-[13px] leading-relaxed text-fg shadow-sm select-text"
      >
        {{ text }}
      </p>
    </div>
  </div>
</template>
