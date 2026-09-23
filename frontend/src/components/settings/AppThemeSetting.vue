<script setup lang="ts">
import { APP_THEMES, type AppTheme } from "../../lib/appTheme";

const props = defineProps<{ theme: AppTheme }>();
const emit = defineEmits<{ change: [theme: AppTheme] }>();
</script>

<template>
  <section class="mt-8" data-setting="app-theme">
    <div class="flex items-center gap-3 border-b border-line/50 pb-2">
      <h2 class="text-[12px] font-semibold uppercase tracking-wider text-fg/80">App theme</h2>
    </div>
    <p class="px-3 pt-3 text-[12px] leading-relaxed text-faint">Choose a palette for this window. Changes apply immediately.</p>
    <div class="grid grid-cols-2 gap-2 px-3 pt-3 sm:grid-cols-3">
      <button
        v-for="option in APP_THEMES"
        :key="option.id"
        type="button"
        :data-omp-theme="option.id"
        :aria-pressed="props.theme === option.id"
        class="min-w-0 rounded-[8px] border bg-canvas p-2.5 text-left text-fg transition-colors"
        :class="props.theme === option.id ? 'border-accent ring-1 ring-accent' : 'border-line hover:border-accent'"
        @click="emit('change', option.id)"
      >
        <span class="flex h-11 overflow-hidden rounded-[5px] border border-line">
          <span class="w-5 shrink-0 bg-rail" />
          <span class="flex flex-1 flex-col gap-1 bg-canvas p-2">
            <span class="h-2 w-4/5 rounded-sm bg-raised" />
            <span class="h-1.5 w-2/5 rounded-sm bg-accent" />
          </span>
        </span>
        <span class="mt-2 block truncate text-[12px] font-medium">{{ option.name }}</span>
        <span class="mt-0.5 block truncate text-[10.5px] text-dim">{{ option.description }}</span>
      </button>
    </div>
  </section>
</template>
