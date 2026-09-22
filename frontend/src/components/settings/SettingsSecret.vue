<script setup lang="ts">
/**
 * A credential control, which is replace-only (`docs/13` §Secrets).
 *
 * There is no reveal, and there cannot be one: the engine never prints a stored credential
 * (`omp config list` reports `{redacted: true}`), so the row shows *presence* — a masked
 * placeholder and a word for what it means — and offers exactly two writes: replace with a
 * new value, or clear the key. That is all the app can honestly do with a secret, and it is
 * enough: changing a token does not require reading the old one.
 *
 * A newly typed value is never echoed back after the write either. The row says it was
 * recorded and returns to the masked state, because the value the screen would show next is
 * the one the engine holds — which it will not print.
 */
import { computed, ref } from "vue";

import Icon from "../ui/Icon.vue";

const props = defineProps<{
  /** Whether the engine holds a value for this key. */
  present: boolean;
  /** The engine holds one it will not print, which is the normal case for a credential. */
  redacted: boolean;
  keyName: string;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  (event: "write", value: unknown): void;
  (event: "reset"): void;
}>();

/** The replace box is open, holding a value that has not been written yet. */
const editing = ref(false);
const typed = ref("");

function start(): void {
  typed.value = "";
  editing.value = true;
}

function save(): void {
  const value = typed.value.trim();
  editing.value = false;
  typed.value = "";
  if (value !== "") emit("write", value);
}

function cancel(): void {
  editing.value = false;
  typed.value = "";
}

/** The presence state, in the two words the row can stand behind. */
const presence = computed(() => {
  if (!props.present) return "not set";
  return props.redacted ? "stored — the engine does not print it" : "set";
});
</script>

<template>
  <div class="flex flex-col items-end gap-1.5">
    <div v-if="!editing" class="flex items-center gap-2">
      <span class="font-mono text-[12.5px]" :class="props.present ? 'text-dim' : 'text-faint'">
        {{ props.present ? "••••••••" : "—" }}
      </span>
      <button
        type="button"
        :disabled="props.disabled"
        class="rounded-[6px] border border-line px-2 py-0.5 text-[11.5px] text-dim hover:border-line-strong hover:text-fg disabled:opacity-40"
        :title="`write a new value for ${props.keyName}`"
        data-action="secret-replace"
        @click="start"
      >
        Replace
      </button>
      <button
        v-if="props.present"
        type="button"
        :disabled="props.disabled"
        class="rounded-[6px] px-1.5 py-0.5 text-[11.5px] text-faint hover:bg-raised hover:text-err disabled:opacity-40"
        :title="`clear ${props.keyName}, back to the engine's default`"
        data-action="secret-clear"
        @click="emit('reset')"
      >
        <Icon name="trash" class="h-3.5 w-3.5" />
      </button>
    </div>

    <div v-else class="flex items-center gap-2">
      <input
        :ref="(input) => (input as HTMLInputElement | null)?.focus()"
        v-model="typed"
        type="password"
        spellcheck="false"
        autocomplete="off"
        placeholder="new value"
        aria-label="new value"
        class="w-[180px] rounded-[6px] bg-raised px-2.5 py-1.5 text-[12.5px] text-fg outline-none placeholder:text-faint focus:ring-1 focus:ring-accent"
        data-input="secret"
        @keydown.enter.prevent="save"
        @keydown.esc="cancel"
        @blur="save"
      />
      <button
        type="button"
        class="rounded-[6px] border border-line-strong px-2 py-0.5 text-[11.5px] text-fg hover:bg-raised"
        data-action="secret-save"
        @click="save"
      >
        Save
      </button>
    </div>

    <span class="text-[10.5px] text-faint">{{ presence }}</span>
  </div>
</template>
