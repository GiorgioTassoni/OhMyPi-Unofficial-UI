<script setup lang="ts">
/**
 * An enum control: the engine's choices in a labelled menu (`docs/12` §12).
 *
 * The labels come from the host (`row.choices`) because the generator is the only thing that
 * saw the schema's own names — humanising `one-at-a-time` here would be a second, worse
 * naming of the same value. The menu marks the value in force with a check rather than by
 * position, and a value the catalog does not list is shown as itself: a stale catalog must
 * not make a row look unset when the engine has something recorded.
 */
import { computed, ref } from "vue";

import Icon from "../ui/Icon.vue";
import Popover from "../Popover.vue";

const props = defineProps<{
  /** The engine's options, with the host's labels. */
  choices: { value: string; label: string }[];
  /** The value in force. */
  value: unknown;
  disabled?: boolean;
  /** What the menu is for, which is the row's label. */
  label: string;
  width?: number;
}>();

const emit = defineEmits<{ (event: "write", value: unknown): void }>();

const open = ref(false);

/** The current value as a string, or `null` when the engine reports none. */
const current = computed(() => (typeof props.value === "string" ? props.value : null));

/** The trigger's text: the host's label for this value, else the value, else nothing set. */
const currentLabel = computed(() => {
  const value = current.value;
  if (value === null) return "not set";
  return props.choices.find((choice) => choice.value === value)?.label ?? value;
});

/** Whether the value in force is one the menu can offer — it is not, on a stale catalog. */
const known = computed(() => current.value !== null && props.choices.some((choice) => choice.value === current.value));

function choose(value: string): void {
  open.value = false;
  if (value === current.value) return;
  emit("write", value);
}
</script>

<template>
  <Popover
    :open="open"
    :label="props.label"
    :width="props.width ?? 240"
    @close="open = false"
  >
    <template #trigger>
      <button
        type="button"
        :disabled="props.disabled"
        class="flex w-[200px] items-center gap-2 rounded-[6px] bg-raised px-2.5 py-1.5 text-left text-[12.5px] hover:bg-raised/80 disabled:opacity-40"
        :title="currentLabel"
        data-control="select"
        @click="open = !open"
      >
        <span class="min-w-0 flex-1 truncate" :class="known ? 'text-fg' : 'text-dim'">
          {{ currentLabel }}
        </span>
        <Icon name="chevron-down" class="h-3.5 w-3.5 shrink-0 text-faint" />
      </button>
    </template>

    <button
      v-for="choice in props.choices"
      :key="choice.value"
      type="button"
      role="option"
      :aria-selected="choice.value === current"
      class="flex w-full items-center gap-2.5 rounded-[6px] px-2.5 py-2 text-left hover:bg-raised"
      :class="choice.value === current ? 'bg-selected' : ''"
      data-choice=""
      :data-value="choice.value"
      @click="choose(choice.value)"
    >
      <span class="text-[11px] text-faint">
        <Icon v-if="choice.value === current" name="check" class="h-3.5 w-3.5 text-accent" />
      </span>
      <span class="min-w-0 flex-1 truncate text-[13px] text-fg">{{ choice.label }}</span>
      <span class="shrink-0 font-mono text-[10.5px] text-faint">{{ choice.value }}</span>
    </button>
  </Popover>
</template>
