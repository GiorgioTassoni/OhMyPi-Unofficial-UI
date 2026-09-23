<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { type ConversationTurn, type GroupKind, workLabel } from "../lib/turnActivity";
import type { DisclosureMemory } from "../lib/disclosureMemory";
import ActivityToolCall from "./ActivityToolCall.vue";
import ConversationRow from "./ConversationRow.vue";
import Icon from "./ui/Icon.vue";

const props = defineProps<{ turn: ConversationTurn; flashed: number | null; memory: DisclosureMemory }>();
const emit = defineEmits<{ failed: [message: string] }>();
const label = computed(() => workLabel(props.turn));
const hasTools = computed(() => props.turn.items.some((item) => item.kind === "group"));
const showActivity = computed(() => hasTools.value || props.turn.working);
const expanded = ref(props.memory.get("turn", props.turn.start, props.turn.working));
const userInteracted = ref(false);
watch(() => props.turn.working, (working, wasWorking) => {
  if (wasWorking && !working && !userInteracted.value) {
    expanded.value = false;
    props.memory.set("turn", props.turn.start, false);
  }
});
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
function icon(kind: GroupKind): "search" | "terminal" | "pencil" | "wrench" {
  return kind === "read" ? "search" : kind === "command" ? "terminal" : kind === "edit" ? "pencil" : "wrench";
}
</script>

<template>
  <details v-if="showActivity" class="group/turn min-w-0" :open="expanded" data-turn-activity @toggle="turnToggled">
    <summary class="sticky top-0 z-10 flex cursor-pointer list-none items-center gap-2 rounded-[5px] bg-canvas py-1.5 text-[12px] text-faint select-none hover:text-fg [&::-webkit-details-marker]:hidden" @click="userInteracted = true">
      <span v-if="turn.working" class="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-warn" />
      <span class="shrink-0">{{ label }}</span>
      <Icon name="chevron-down" class="h-3 w-3 shrink-0 transition-transform group-open/turn:rotate-180" />
      <span class="ml-1 h-px min-w-0 flex-1 bg-line/70" />
    </summary>
    <div v-if="expanded" class="min-w-0 py-2">
      <template v-for="item in turn.items" :key="item.index">
        <details v-if="item.kind === 'group' && item.group.rows.length > 1" class="group/activity min-w-0" :open="groupExpanded(item.index)" :data-group-index="item.index" @toggle="groupToggled(item.index, $event)">
          <summary class="flex min-w-0 cursor-pointer list-none items-center gap-2 rounded-[6px] px-2 py-1.5 text-[12px] text-dim select-none hover:bg-raised/65 hover:text-fg [&::-webkit-details-marker]:hidden">
            <Icon :name="icon(item.group.kind)" class="h-3.5 w-3.5 shrink-0 text-faint" />
            <span class="min-w-0 flex-1 truncate font-medium text-fg">{{ item.group.label }}</span>
            <span v-if="item.group.added !== null" class="shrink-0 font-mono text-[11px]"><span class="text-ok">+{{ item.group.added }}</span> <span class="text-err">-{{ item.group.removed }}</span></span>
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
        />
        <div
          v-else
          :data-row-index="item.entry.index"
          :class="flashed === item.entry.index ? 'rounded bg-accent/10 ring-1 ring-accent' : ''"
        >
          <ConversationRow :row="item.entry.row" @failed="emit('failed', $event)" />
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
      <ConversationRow v-if="item.kind === 'row'" :row="item.entry.row" @failed="emit('failed', $event)" />
    </div>
  </div>
</template>
