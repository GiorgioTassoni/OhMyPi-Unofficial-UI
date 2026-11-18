<script setup lang="ts">
import Toggle from "../ui/Toggle.vue";

const props = defineProps<{
  enabled: boolean;
}>();

const emit = defineEmits<{
  change: [enabled: boolean];
}>();
</script>

<template>
  <section class="mt-7">
    <h2 class="px-1 pb-2 text-[12px] font-medium text-dim">
      MCP tools & extensions
    </h2>

    <div
      class="flex items-start gap-6 rounded-[10px] border border-line bg-raised/35 px-4 py-3.5"
      data-setting="mcp-design-choice"
    >
      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-2">
          <p class="text-[13.5px] font-medium text-fg">Visual Design Choice (MCP)</p>
          <span
            v-if="props.enabled"
            class="inline-flex items-center gap-1 rounded-[4px] bg-accent/10 px-1.5 py-0.5 text-[10.5px] font-medium text-accent"
          >
            <span class="h-1.5 w-1.5 rounded-full bg-accent" />
            Active
          </span>
          <span
            v-else
            class="inline-flex items-center gap-1 rounded-[4px] bg-surface px-1.5 py-0.5 text-[10.5px] font-medium text-dim"
          >
            <span class="h-1.5 w-1.5 rounded-full bg-line-strong" />
            Disabled
          </span>
        </div>

        <p class="mt-1 text-[12px] leading-relaxed text-dim">
          Interactive Human-in-the-Loop HTML design variant selection and live iframe preview.
          Pauses the agent turn until a variant is chosen via browser UI or terminal.
        </p>

        <div class="mt-2.5 flex flex-wrap items-center gap-1.5 text-[11px] text-faint">
          <span class="rounded-[4px] border border-line bg-surface/60 px-1.5 py-0.5 font-mono text-[10.5px] text-dim">
            design_choice
          </span>
          <span class="rounded-[4px] border border-line bg-surface/60 px-1.5 py-0.5 font-mono text-[10.5px] text-dim">
            preview_html
          </span>
        </div>
      </div>

      <Toggle
        :checked="props.enabled"
        label="Visual Design Choice (MCP)"
        @change="emit('change', $event)"
      />
    </div>
  </section>
</template>
