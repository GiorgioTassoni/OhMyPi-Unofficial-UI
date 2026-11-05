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
  <section class="mt-8">
    <div class="flex items-center gap-3 border-b border-line/50 pb-2">
      <h2 class="text-[12px] font-semibold uppercase tracking-wider text-fg/80">
        MCP Tools & Extensions
      </h2>
    </div>

    <div
      class="flex items-start gap-6 rounded-[8px] px-3 py-3 hover:bg-raised/40"
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
            Active with Desktop
          </span>
          <span
            v-else
            class="inline-flex items-center gap-1 rounded-[4px] bg-surface px-1.5 py-0.5 text-[10.5px] font-medium text-dim"
          >
            <span class="h-1.5 w-1.5 rounded-full bg-line-strong" />
            Disabled
          </span>
        </div>

        <p class="mt-1 text-[12px] leading-relaxed text-faint">
          Interactive Human-in-the-Loop HTML design variant selection and live iframe preview.
          Pauses the agent turn until a variant is chosen via browser UI or terminal.
        </p>

        <div class="mt-2.5 flex flex-wrap items-center gap-2 text-[11px] text-faint">
          <span class="rounded-[4px] border border-line/50 bg-surface/50 px-1.5 py-0.5 font-mono text-[10.5px] text-dim">
            tool: design_choice
          </span>
          <span class="rounded-[4px] border border-line/50 bg-surface/50 px-1.5 py-0.5 font-mono text-[10.5px] text-dim">
            tool: preview_html
          </span>
          <span class="text-dim">· Stdio transport + local ephemeral preview</span>
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
