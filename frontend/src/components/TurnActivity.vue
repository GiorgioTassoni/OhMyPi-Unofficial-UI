<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from "vue";
import { type ConversationTurn, workLabel } from "../lib/turnActivity";
import type { DisclosureMemory } from "../lib/disclosureMemory";
import ActivityToolCall from "./ActivityToolCall.vue";
import ConversationRow from "./ConversationRow.vue";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  turn: ConversationTurn;
  flashed: number | null;
  memory: DisclosureMemory;
  nextTurnIsSteer?: boolean;
}>();
const emit = defineEmits<{
  failed: [message: string];
  openFile: [file: { path: string; content?: string; artifactId?: string | null }];
}>();

const now = ref(Date.now());
let timer: ReturnType<typeof setInterval> | null = null;

function startTicker() {
  if (timer) return;
  now.value = Date.now();
  timer = setInterval(() => {
    now.value = Date.now();
  }, 1000);
}

function stopTicker() {
  if (timer) {
    clearInterval(timer);
    timer = null;
  }
}

watch(
  () => props.turn.working,
  (working, wasWorking) => {
    if (working) {
      startTicker();
    } else {
      stopTicker();
    }
    if (wasWorking && !working && !userInteracted.value) {
      expanded.value = false;
      props.memory.set("turn", props.turn.start, false);
    }
  },
  { immediate: true }
);

onUnmounted(() => {
  stopTicker();
});

const label = computed(() => workLabel(props.turn, now.value, props.nextTurnIsSteer));
const hasTools = computed(() => props.turn.items.some((item) => item.kind === "group" || item.kind === "agent-completion"));
const showActivity = computed(() => hasTools.value || props.turn.working);
const expanded = ref(props.memory.get("turn", props.turn.start, props.turn.working));
const userInteracted = ref(false);
const closedGroups = ref(new Set(props.turn.items
  .filter((item) => item.kind === "group" && item.group.rows.length > 1 && !props.memory.get("group", item.index, true))
  .map((item) => item.index)));
function groupExpanded(index: number): boolean { return !closedGroups.value.has(index); }
function turnToggled(event: Event): void {
  expanded.value = (event.target as HTMLDetailsElement).open;
  props.memory.set("turn", props.turn.start, expanded.value);
}
function groupToggled(index: number, event: Event): void {
  const open = (event.target as HTMLDetailsElement).open;
  const next = new Set(closedGroups.value);
  if (open) next.delete(index);
  else next.add(index);
  closedGroups.value = next;
  props.memory.set("group", index, open);
}
</script>

<template>
  <details v-if="showActivity" class="group/turn min-w-0" :open="expanded" data-turn-activity @toggle="turnToggled">
    <summary class="sticky top-0 z-10 flex cursor-pointer list-none items-center gap-2 rounded-[5px] bg-canvas py-1.5 text-[12px] text-faint select-none hover:text-fg [&::-webkit-details-marker]:hidden" @click="userInteracted = true">
      <span v-if="turn.working" class="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-warn" />
      <span class="shrink-0 tabular-nums">{{ label }}</span>
      <Icon name="chevron-down" class="h-3 w-3 shrink-0 transition-transform group-open/turn:rotate-180" />
      <span class="ml-1 h-px min-w-0 flex-1 bg-line/70" />
    </summary>
    <div v-if="expanded" class="min-w-0 py-2">
      <template v-for="(item, position) in turn.items" :key="`${item.index}-${position}`">
        <details v-if="item.kind === 'group' && item.group.rows.length > 1" class="group/activity min-w-0" :open="groupExpanded(item.index)" :data-group-index="item.index" @toggle="groupToggled(item.index, $event)">
          <summary class="flex min-w-0 cursor-pointer list-none items-center gap-2 rounded-[6px] px-2 py-1.5 text-[12px] text-dim select-none hover:bg-raised/65 hover:text-fg [&::-webkit-details-marker]:hidden">
            <Icon :name="item.group.icon" class="h-3.5 w-3.5 shrink-0 text-faint" />
            <span class="min-w-0 flex-1 truncate font-medium text-fg">{{ item.group.label }}</span>
            <span v-if="item.group.added !== null" class="shrink-0 font-mono text-[11px]">
              <span v-if="item.group.added > 0" class="text-ok">+{{ item.group.added }}</span>
              <span v-if="item.group.added > 0 && item.group.removed !== null && item.group.removed > 0"> </span>
              <span v-if="item.group.removed !== null && item.group.removed > 0" class="text-err">-{{ item.group.removed }}</span>
            </span>
            <Icon name="chevron-down" class="h-3 w-3 shrink-0 text-faint transition-transform group-open/activity:rotate-180" />
          </summary>
          <div v-if="groupExpanded(item.index)" class="mb-1 ml-4 border-l border-line/60 pl-2">
            <ActivityToolCall
              v-for="entry in item.group.rows"
              :key="entry.index"
              :entry="entry"
              :kind="item.group.kind"
              :flashed="flashed === entry.index"
              :memory="memory"
              @open-file="emit('openFile', $event)"
            />
          </div>
        </details>
        <ActivityToolCall
          v-else-if="item.kind === 'group'"
          :entry="item.group.rows[0]"
          :kind="item.group.kind"
          :flashed="flashed === item.index"
          :memory="memory"
          standalone
          @open-file="emit('openFile', $event)"
        />
        <div
          v-else-if="item.kind === 'agent-completion'"
          :data-row-index="item.entry.index"
          class="flex min-w-0 items-center gap-2 rounded-[6px] px-2 py-1 text-[12px] text-dim"
          :class="flashed === item.entry.index ? 'bg-accent/10 ring-1 ring-accent' : ''"
        >
          <Icon name="bot" class="h-3.5 w-3.5 shrink-0" :class="item.status === 'completed' ? 'text-ok' : 'text-err'" />
          <span class="truncate">{{ item.status === 'completed' ? 'Completed' : item.status === 'failed' ? 'Failed' : 'Stopped' }} job <span class="font-medium text-fg">{{ item.name }}</span></span>
        </div>
        <div
          v-else-if="item.kind === 'notice'"
          :data-row-index="item.entry.index"
          class="my-2 flex items-center gap-2.5 py-1 text-[11.5px] select-none"
          role="status"
        >
          <div class="h-px flex-1 bg-line/50" />
          <div
            class="flex items-center gap-1.5 font-medium"
            :class="item.entry.row.role === 'notice:error' ? 'text-err' : item.entry.row.role === 'notice:warning' ? 'text-warn' : 'text-dim'"
          >
            <Icon
              :name="item.entry.row.role === 'notice:error' ? 'close' : 'sparkle'"
              class="h-3.5 w-3.5 shrink-0"
              :class="item.entry.row.role === 'notice:error' ? 'text-err' : item.entry.row.role === 'notice:warning' ? 'text-warn' : 'text-accent'"
            />
            <span>{{ item.entry.row.text }}</span>
          </div>
          <div class="h-px flex-1 bg-line/50" />
        </div>
        <div
          v-else
          :data-row-index="item.entry.index"
          :class="flashed === item.entry.index ? 'rounded bg-accent/10 ring-1 ring-accent' : ''"
        >
          <ConversationRow :row="item.entry.row" :hide-copy="true" @failed="emit('failed', $event)" />
        </div>
      </template>
      <p v-if="turn.items.length === 0 && turn.working" class="px-2 py-1 text-[11.5px] text-faint">Waiting for activity…</p>
    </div>
  </details>
  <div v-else-if="turn.items.length" class="min-w-0 space-y-1">
    <div
      v-for="item in turn.items"
      :key="item.index"
      :data-row-index="item.index"
      :class="flashed === item.index ? 'rounded bg-accent/10 ring-1 ring-accent' : ''"
    >
      <div
        v-if="item.kind === 'notice'"
        class="my-2 flex items-center gap-2.5 py-1 text-[11.5px] select-none"
        role="status"
      >
        <div class="h-px flex-1 bg-line/50" />
        <div
          class="flex items-center gap-1.5 font-medium"
          :class="item.entry.row.role === 'notice:error' ? 'text-err' : item.entry.row.role === 'notice:warning' ? 'text-warn' : 'text-dim'"
        >
          <Icon
            :name="item.entry.row.role === 'notice:error' ? 'close' : 'sparkle'"
            class="h-3.5 w-3.5 shrink-0"
            :class="item.entry.row.role === 'notice:error' ? 'text-err' : item.entry.row.role === 'notice:warning' ? 'text-warn' : 'text-accent'"
          />
          <span>{{ item.entry.row.text }}</span>
        </div>
        <div class="h-px flex-1 bg-line/50" />
      </div>
      <ConversationRow v-else-if="item.kind === 'row'" :row="item.entry.row" :hide-copy="true" @failed="emit('failed', $event)" />
    </div>
  </div>
</template>
