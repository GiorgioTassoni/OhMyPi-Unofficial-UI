<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { agentMessages, type AgentSnapshot, type RowSnapshot } from "../bridge";
import ConversationRow from "./ConversationRow.vue";

const props = defineProps<{
  thread: string;
  agentId: string;
  agent: AgentSnapshot | null;
  agentName?: string;
  active: boolean;
}>();
const emit = defineEmits<{ failed: [message: string] }>();

const displayName = computed(() => props.agent?.agent || props.agentName || props.agentId);
const statusDisplay = computed(() => {
  if (props.agent?.status) return props.agent.status;
  if (rows.value.length > 0) return "completed";
  return loading.value ? "loading" : "settled";
});

const rows = ref<RowSnapshot[]>([]);
const scroll = ref<HTMLElement | null>(null);
const loading = ref(true);
const error = ref<string | null>(null);
let lastByte = -1;
let polling: ReturnType<typeof setInterval> | null = null;
let reading = false;

async function read(): Promise<void> {
  if (reading || !props.active) return;
  reading = true;
  const pane = scroll.value;
  const follow = !pane || pane.scrollHeight - pane.scrollTop - pane.clientHeight < 96;
  try {
    const page = await agentMessages(props.thread, props.agentId, 0);
    if (page.nextByte !== lastByte || page.reset || rows.value.length === 0) {
      rows.value = page.rows;
      lastByte = page.nextByte;
      if (follow && page.rows.length > 0) {
        await nextTick();
        scroll.value?.scrollTo({ top: scroll.value.scrollHeight });
      }
    }
    error.value = null;
  } catch (cause) {
    error.value = String(cause);
  } finally {
    loading.value = false;
    reading = false;
  }
}

watch(() => [props.active, props.agentId], ([active]) => {
  if (active) {
    lastByte = -1;
    void read();
  }
}, { immediate: true });

onMounted(() => { polling = setInterval(() => { void read(); }, 1000); });
onUnmounted(() => { if (polling !== null) clearInterval(polling); });
</script>

<template>
  <section class="flex min-h-0 min-w-0 flex-1 flex-col" aria-label="Read-only agent activity">
    <header class="shrink-0 border-b border-line/50 px-5 py-3">
      <div class="mx-auto flex max-w-[760px] flex-wrap items-center gap-x-3 gap-y-1">
        <span class="text-[13px] font-medium text-fg">{{ displayName }}</span>
        <span class="text-[11px] text-dim">{{ statusDisplay }}</span>
        <span v-if="agent?.progress?.resolvedModel" class="font-mono text-[11px] text-faint">{{ agent.progress.resolvedModel }}</span>
        <span class="ml-auto text-[11px] text-faint">Read-only</span>
      </div>
      <p v-if="agent?.progress?.lastIntent || agent?.description" class="mx-auto mt-1 max-w-[760px] truncate text-[11.5px] text-dim">
        {{ agent?.progress?.lastIntent || agent?.description }}
      </p>
    </header>
    <div ref="scroll" class="min-h-0 flex-1 overflow-y-auto px-5 py-5">
      <div class="mx-auto flex max-w-[760px] flex-col gap-3 pb-8">
        <p v-if="rows.length === 0 && loading" class="text-[12px] text-faint">Loading agent activity…</p>
        <p v-else-if="rows.length === 0 && !error" class="text-[12px] text-faint">No activity recorded for this agent.</p>
        <p v-if="error" class="text-[12px] text-err">Could not load agent activity: {{ error }}</p>
        <ConversationRow v-for="(row, index) in rows" :key="index" :row="row" @failed="emit('failed', $event)" />
      </div>
    </div>
  </section>
</template>
