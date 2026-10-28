<script setup lang="ts">
/**
 * The Subagents tab in the thread panel (`RightPanel.vue`).
 *
 * Shows all subagents called in the current conversation:
 * - Active subagents and the latest 5 finished subagents have full details and transcript access.
 * - Older subagents (beyond the top 5) show only their name and finished status without diffs or chronology.
 */
import { computed, ref } from "vue";
import { type ChatSubagent, age } from "../lib/agents";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  thread: string | null;
  subagents: ChatSubagent[];
  activeAgentId?: string | null | undefined;
  live: boolean;
}>();

const emit = defineEmits<{
  openAgent: [thread: string, id: string];
}>();

function formatAgentAge(ms: number | null): string {
  if (!ms) return "finished";
  return age(ms, Date.now());
}

const activeCount = computed(() => props.subagents.filter((s) => s.running).length);

/** Maximum number of finished subagents rendered with full detail cards and transcript access. */
const MAX_DETAILED_FINISHED_AGENTS = 5;

const activeSubagents = computed(() => props.subagents.filter((s) => s.running));
const finishedSubagents = computed(() => props.subagents.filter((s) => !s.running));
const detailedFinishedSubagents = computed(() =>
  finishedSubagents.value.slice(0, MAX_DETAILED_FINISHED_AGENTS),
);
const olderFinishedSubagents = computed(() =>
  finishedSubagents.value.slice(MAX_DETAILED_FINISHED_AGENTS),
);

const primarySubagents = computed(() => [
  ...activeSubagents.value,
  ...detailedFinishedSubagents.value,
]);

const showOlder = ref(false);
</script>

<template>
  <div class="flex h-full w-full min-h-0 flex-1 flex-col overflow-y-auto">
    <!-- Empty state -->
    <div
      v-if="props.subagents.length === 0"
      class="flex flex-1 flex-col items-center justify-center px-6 py-12 text-center"
    >
      <div class="mb-3 flex h-10 w-10 items-center justify-center rounded-xl border border-line/40 bg-raised/40 text-faint shadow-xs">
        <Icon name="sparkle" class="h-5 w-5" />
      </div>
      <p class="text-[13px] font-medium text-main">No subagents yet</p>
      <p class="mt-1 max-w-[220px] text-[12px] leading-relaxed text-faint">
        Subagents spawned in this conversation will appear here.
      </p>
    </div>

    <!-- Subagents list -->
    <div v-else class="flex flex-col p-2.5">
      <div class="mb-2 flex items-center justify-between px-1 text-[11px] text-faint">
        <span>{{ props.subagents.length }} {{ props.subagents.length === 1 ? 'subagent' : 'subagents' }}</span>
        <span v-if="activeCount > 0" class="flex items-center gap-1 font-mono text-accent">
          <span class="inline-block size-1.5 rounded-full bg-accent animate-pulse" />
          {{ activeCount }} running
        </span>
      </div>

      <ul class="flex flex-col gap-1.5">
        <li v-for="agent in primarySubagents" :key="agent.id">
          <button
            type="button"
            class="group flex w-full flex-col gap-1 rounded-lg border p-2.5 text-left transition-all"
            :class="props.activeAgentId === agent.id
              ? 'border-accent/40 bg-raised/90 text-fg shadow-xs ring-1 ring-accent/30'
              : 'border-line/30 bg-surface/30 hover:border-line/60 hover:bg-raised/60 text-dim hover:text-fg'"
            @click="emit('openAgent', agent.thread, agent.id)"
          >
            <div class="flex items-center gap-2">
              <!-- Status dot -->
              <span
                class="size-2 shrink-0 rounded-full transition-all"
                :class="{
                  'animate-pulse bg-accent': agent.running,
                  'bg-ok': agent.status === 'completed',
                  'bg-err': agent.status === 'failed' || agent.status === 'aborted',
                  'bg-faint/80': !agent.running && agent.status !== 'completed' && agent.status !== 'failed' && agent.status !== 'aborted',
                }"
              />

              <span class="min-w-0 flex-1 truncate text-[12.5px] font-medium text-fg">
                {{ agent.name }}
              </span>

              <span
                class="shrink-0 font-mono text-[10.5px]"
                :class="agent.running ? 'text-accent font-medium' : 'text-faint'"
              >
                {{ agent.running ? 'working' : (agent.finishedAt ? formatAgentAge(agent.finishedAt) : 'finished') }}
              </span>
            </div>

            <!-- Description / task if available -->
            <p
              v-if="agent.description || agent.task"
              class="line-clamp-2 text-[11.5px] leading-relaxed text-faint group-hover:text-dim"
            >
              {{ agent.description || agent.task }}
            </p>
          </button>
        </li>
      </ul>

      <!-- Older subagents (beyond the latest 5) - showing only name and finished without diff/chronology -->
      <div v-if="olderFinishedSubagents.length > 0" class="mt-3 pt-2.5 border-t border-line/20">
        <button
          type="button"
          class="flex w-full items-center justify-between rounded px-1.5 py-1 text-[11px] text-faint hover:text-dim hover:bg-raised/30 transition-colors"
          :aria-expanded="showOlder"
          aria-controls="earlier-subagents-list"
          @click="showOlder = !showOlder"
        >
          <span class="font-medium">Earlier completed ({{ olderFinishedSubagents.length }})</span>
          <div class="flex items-center gap-1">
            <span class="text-[10px]">{{ showOlder ? 'hide' : 'show' }}</span>
            <Icon :name="showOlder ? 'chevron-down' : 'chevron-right'" class="h-3 w-3" />
          </div>
        </button>

        <ul v-if="showOlder" id="earlier-subagents-list" class="mt-1.5 flex flex-col gap-1">
          <li
            v-for="agent in olderFinishedSubagents"
            :key="agent.id"
            class="flex items-center justify-between rounded border border-line/20 bg-surface/20 px-2.5 py-1 text-[11.5px]"
          >
            <span class="truncate font-medium text-dim">{{ agent.name }}</span>
            <span class="shrink-0 font-mono text-[10px] text-faint">finished</span>
          </li>
        </ul>
      </div>
    </div>
  </div>
</template>
