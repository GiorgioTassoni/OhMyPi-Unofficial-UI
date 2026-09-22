<script setup lang="ts">
/**
 * Fork from here (`docs/12` §6.3): pick the user message the new session starts after.
 *
 * The rows come from the engine's `get_branch_messages`, which is the only place valid
 * entry ids exist — the transcript rows the app renders are its own model and carry no
 * entry ids at all. Sending anything else would make the engine throw "Invalid entry ID for
 * branching", and its outcome over RPC is not something a client can rely on.
 */
import { computed, ref } from "vue";
import type { BranchTarget } from "../bridge";
import Modal from "./Modal.vue";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  targets: BranchTarget[];
  busy: boolean;
}>();

const emit = defineEmits<{
  pick: [entryId: string];
  close: [];
}>();

const query = ref("");
const chosen = ref<string | null>(null);

/** Case-insensitive containment, which is what a user means when they type two words they
 * remember from the message. */
const matches = computed(() => {
  const needle = query.value.trim().toLowerCase();
  if (needle === "") return props.targets;
  return props.targets.filter((target) => target.text.toLowerCase().includes(needle));
});
</script>

<template>
  <Modal title="fork from a message" :busy="props.busy" @close="emit('close')">
    <div class="mb-2 flex items-center gap-2 rounded-[6px] bg-raised px-2.5 py-2">
      <Icon name="search" class="h-3.5 w-3.5 shrink-0 text-faint" />
      <input
        v-model="query"
        spellcheck="false"
        class="min-w-0 flex-1 bg-transparent text-[12.5px] text-fg outline-none placeholder:text-faint"
        placeholder="filter messages"
      />
    </div>

    <p v-if="props.targets.length === 0" class="px-1 py-2 text-[12.5px] text-faint">
      this session has no user messages to fork from yet
    </p>
    <p v-else-if="matches.length === 0" class="px-1 py-2 text-[12.5px] text-faint">
      nothing matches “{{ query }}”
    </p>

    <ul class="flex flex-col gap-px">
      <li v-for="target in matches" :key="target.entryId">
        <button
          class="w-full rounded-[6px] px-2.5 py-2 text-left text-[12.5px]"
          :class="
            chosen === target.entryId
              ? 'bg-selected text-fg'
              : 'text-dim hover:bg-raised hover:text-fg'
          "
          @click="chosen = target.entryId"
          @dblclick="emit('pick', target.entryId)"
        >
          {{ target.text.length > 160 ? `${target.text.slice(0, 160)}…` : target.text }}
        </button>
      </li>
    </ul>

    <div class="mt-3 flex items-center gap-2">
      <button
        :disabled="props.busy || chosen === null"
        class="rounded-[6px] bg-accent px-2.5 py-1 text-[12px] font-medium text-canvas disabled:opacity-40"
        @click="chosen && emit('pick', chosen)"
      >
        {{ props.busy ? "forking…" : "fork" }}
      </button>
      <span class="text-[11px] text-faint">
        the new session opens as a child of this one, in the same sidecar
      </span>
    </div>
  </Modal>
</template>
