<script setup lang="ts">
/**
 * One field, committed when the user is done with it (`docs/12` §12).
 *
 * Values commit on blur or `Enter` and **not** on every keystroke: a write here is a config
 * write, and a `set` per character would put a hundred lines in the engine's log and a
 * hundred chances to write something half-typed. `Escape` puts the engine's own value back.
 *
 * It is a field rather than an input with a model because the value arrives from the host and
 * comes back from it: the draft is seeded from `value` and re-seeded whenever the engine
 * reports something different, which is what makes a refused write visibly *not* applied
 * instead of leaving the field claiming a value the engine never took.
 */
import { ref, watch } from "vue";

import { textOf } from "../../lib/settings";

const props = defineProps<{
  /** The value as the host reports it. */
  value: unknown;
  /** Why the last commit was refused: the field marks itself, the row prints the reason. */
  error?: string | null;
  disabled?: boolean;
  /** Code, paths, counts: the value is mono. A sentence is not. */
  mono?: boolean;
  placeholder?: string;
  /** Tailwind width classes: a count does not need a path's room. */
  width?: string;
  /** Empty the field after a commit — for an "add" box, whose own value never changes. */
  clearAfterCommit?: boolean;
}>();

const emit = defineEmits<{ (event: "commit", text: string): void }>();

const draft = ref(textOf(props.value));
/**
 * The text this field last handed over.
 *
 * The guard is against *this*, not against the host's value: `Enter` commits and then the
 * field keeps its focus, so the blur that follows — a click elsewhere, a tab — would otherwise
 * write the same text a second time whenever the engine's answer is not character-for-character
 * what was typed (a `45` normalised out of `45 `, a number echoed back with its own formatting).
 */
const committed = ref(textOf(props.value));

watch(
  () => props.value,
  (next) => {
    draft.value = textOf(next);
    committed.value = textOf(next);
  },
);

/** Blur and `Enter` both land here; unchanged text is not a write. */
function commit(): void {
  if (draft.value === committed.value) return;
  committed.value = draft.value;
  emit("commit", draft.value);
  if (props.clearAfterCommit === true) draft.value = "";
}

function revert(): void {
  draft.value = textOf(props.value);
}
</script>

<template>
  <input
    v-model="draft"
    type="text"
    spellcheck="false"
    :disabled="props.disabled"
    :placeholder="props.placeholder ?? ''"
    :class="[
      props.width ?? 'w-[200px]',
      props.mono === true ? 'font-mono' : '',
      'rounded-[6px] border border-line/60 bg-raised px-2.5 py-1.5 text-[12.5px] text-fg outline-none transition-colors',
      'placeholder:text-faint focus:border-accent/80 focus:ring-1 focus:ring-accent/40 disabled:opacity-40',
      props.error != null ? 'border-err ring-1 ring-err' : '',
    ]"
    @blur="commit"
    @keydown.enter.prevent="commit"
    @keydown.esc="revert"
  />
</template>
