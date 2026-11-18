<script setup lang="ts">
import { APP_THEMES, type AppTheme } from "../../lib/appTheme";

const props = defineProps<{ theme: AppTheme }>();
const emit = defineEmits<{ change: [theme: AppTheme] }>();
</script>

<template>
  <section class="mt-7" data-setting="app-theme">
    <h2 class="px-1 pb-2 text-[12px] font-medium text-dim">App theme</h2>
    <div class="rounded-[10px] border border-line bg-raised/35 p-4">
      <p class="text-[12px] leading-relaxed text-dim">Choose a palette for this window. Changes apply immediately.</p>
      <div class="mt-3 grid grid-cols-2 gap-2.5 sm:grid-cols-3">
        <button
          v-for="option in APP_THEMES"
          :key="option.id"
          type="button"
          :data-omp-theme="option.id"
          :aria-pressed="props.theme === option.id"
          class="min-w-0 rounded-[8px] border bg-canvas p-2.5 text-left text-fg transition-colors"
          :class="props.theme === option.id ? 'border-accent ring-1 ring-accent' : 'border-line hover:border-line-strong'"
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
    </div>
  </section>
</template>
