<script setup lang="ts">
defineProps<{
  text: string;
}>();
</script>

<template>
  <div class="flex flex-col items-end gap-1 my-2">
    <div class="flex max-w-[80%] min-w-0 flex-col items-end">
      <p
        class="whitespace-pre-wrap break-words rounded-[16px] bg-raised/95 border border-line/60 border-t-line-strong/40 px-4 py-2.5 text-[13px] leading-relaxed text-fg shadow-sm select-text"
      >
        {{ text }}
      </p>
    </div>
  </div>
</template>
