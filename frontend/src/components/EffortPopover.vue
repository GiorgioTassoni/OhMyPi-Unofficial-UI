<script setup lang="ts">
/**
 * The effort ladder (`docs/12` §7.6).
 *
 * `off` through `max`, plus `auto`. Each row says whether the *current model* accepts
 * that level, from the catalogue's `thinking.efforts` — the engine's own metadata, so
 * the answer is measured rather than guessed, and a level the model cannot take is
 * marked instead of being sent and failing quietly.
 */
import { ref } from "vue";
import { setThinkingLevel } from "../bridge";
import { EFFORT_LEVELS, effortLabel, effortSupported, type ModelOption } from "../lib/chips";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  thread: string;
  open: boolean;
  /** The session's level, as `get_state` reports it. */
  level: string | null;
  /** The session's model, when the catalogue knows it — the support check. */
  model: ModelOption | null;
}>();

const emit = defineEmits<{
  (event: "close"): void;
  (event: "failed", message: string): void;
}>();

const switching = ref<string | null>(null);

async function choose(level: string): Promise<void> {
  if (switching.value !== null) {
    return;
  }
  if (level === props.level) {
    emit("close");
    return;
  }
  switching.value = level;
  try {
    await setThinkingLevel(props.thread, level);
    emit("close");
  } catch (cause) {
    emit("failed", describe(cause));
  } finally {
    switching.value = null;
  }
}

function describe(cause: unknown): string {
  return typeof cause === "string"
    ? cause
    : cause instanceof Error
      ? cause.message
      : String(cause);
}
</script>

<template>
  <div class="flex flex-col gap-0.5">
    <button
      v-for="level in EFFORT_LEVELS"
      :key="level"
      type="button"
      :disabled="switching !== null"
      class="flex w-full items-center gap-3 rounded-[6px] px-2.5 py-2 text-left hover:bg-raised disabled:opacity-40"
      @click="choose(level)"
    >
      <span class="text-[13px] text-fg">{{ effortLabel(level).replace("Effort ", "") }}</span>
      <span v-if="level === props.level" class="text-[11px] text-accent">current</span>
      <span v-if="switching === level" class="text-[11px] text-faint">setting…</span>

      <!-- A level this model cannot take is named rather than hidden, and the check takes
           the place of the shortcut the mode popover puts here. -->
      <span
        v-if="!effortSupported(level, props.model)"
        class="ml-auto shrink-0 text-[11px] text-faint"
      >
        model does not support
      </span>
      <Icon
        v-if="level === props.level"
        name="check"
        class="ml-auto h-4 w-4 shrink-0 text-accent"
      />
    </button>

    <p class="px-2.5 pb-0.5 pt-1.5 text-[11px] text-faint">
      auto picks a level per turn — a difficulty classifier, run on-device when a local
      model is configured
    </p>
  </div>
</template>
