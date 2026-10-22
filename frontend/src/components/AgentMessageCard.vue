<script setup lang="ts">
import { ref } from "vue";
import type { AgentMessage } from "../lib/agentMessage";
import Icon from "./ui/Icon.vue";
import MarkdownBody from "./MarkdownBody.vue";

defineProps<{ message: AgentMessage }>();
const emit = defineEmits<{ (event: "failed", message: string): void }>();
const expanded = ref(false);
function onToggle(event: Event): void {
  expanded.value = (event.target as HTMLDetailsElement).open;
}
</script>

<template>
  <details class="group/agent my-1 min-w-0 overflow-hidden rounded-[9px] border border-line/60 bg-raised/25" @toggle="onToggle">
    <summary class="flex min-w-0 cursor-pointer list-none items-start gap-2.5 px-3 py-2.5 select-none hover:bg-raised/50 [&::-webkit-details-marker]:hidden">
      <Icon
        :name="message.status === 'failed' ? 'close' : message.status === 'cancelled' ? 'info' : 'bot'"
        class="mt-0.5 h-3.5 w-3.5 shrink-0"
        :class="message.status === 'failed' ? 'text-err' : message.status === 'cancelled' ? 'text-warn' : 'text-accent'"
      />
      <div class="min-w-0 flex-1">
        <div class="flex min-w-0 flex-wrap items-baseline gap-x-1.5 text-[12px]">
          <span class="break-words font-medium text-fg">{{ message.sender }}</span>
          <span v-if="message.recipient" class="break-words text-dim">→ {{ message.recipient }}</span>
          <span class="text-faint">· {{ message.label }}</span>
        </div>
        <p class="mt-0.5 truncate text-[11.5px] text-dim">{{ message.summary }}</p>
      </div>
      <Icon name="chevron-down" class="mt-0.5 h-3 w-3 shrink-0 text-faint transition-transform group-open/agent:rotate-180" />
    </summary>
    <!-- Render long reports only on expansion; old transcripts remain lightweight. -->
    <div v-if="expanded" class="max-h-96 overflow-auto border-t border-line/50 px-3 py-3 text-[12.5px] leading-relaxed text-fg">
      <MarkdownBody v-if="message.body" :text="message.body" @failed="emit('failed', $event)" />
      <section v-for="(section, index) in message.sections" :key="index" class="mb-3 last:mb-0">
        <h4 class="mb-1 text-[11px] font-medium text-dim">{{ section.title }}</h4>
        <MarkdownBody :text="section.text" @failed="emit('failed', $event)" />
      </section>
      <details class="group/raw mt-3 border-t border-line/50 pt-2">
        <summary class="flex cursor-pointer list-none items-center gap-1.5 text-[11px] text-faint hover:text-dim [&::-webkit-details-marker]:hidden">
          <Icon name="chevron-right" class="h-3 w-3 transition-transform group-open/raw:rotate-90" />
          Raw message
        </summary>
        <pre class="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-words font-mono text-[11px] text-dim select-text">{{ message.raw }}</pre>
      </details>
    </div>
  </details>
</template>
