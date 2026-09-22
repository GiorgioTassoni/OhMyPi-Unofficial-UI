<script setup lang="ts">
/**
 * The context popover (`docs/12` §7.5).
 *
 * Usage, the auto-compaction switch, and `Compact now` with optional instructions.
 * The bar and the toggle both read the session's own state, so the panel never
 * contradicts the ring that opened it; `isCompacting` drives the busy state, because a
 * compaction is a real pause and a UI that offered the button again would queue a
 * second one behind the first.
 */
import { computed, ref } from "vue";
import {
  compact as compactSession,
  setAutoCompaction,
  type ContextSnapshot,
} from "../bridge";
import { percentLabel } from "../lib/chips";
import Toggle from "./ui/Toggle.vue";

const props = defineProps<{
  thread: string;
  open: boolean;
  /** `get_state.contextUsage`, or null before the session has one. */
  context: ContextSnapshot | null;
  /** `get_state.autoCompactionEnabled`. */
  autoCompaction: boolean | null;
  /** `get_state.isCompacting`. */
  compacting: boolean;
}>();

const emit = defineEmits<{
  (event: "close"): void;
  (event: "failed", message: string): void;
}>();

const instructions = ref("");
const busy = ref<null | "toggle" | "compact">(null);

/** `No usage yet` when the window or the tokens are unknown, never a fake 0%. */
const summary = computed(() => {
  const tokens = props.context?.tokens ?? 0;
  const window = props.context?.contextWindow ?? 0;
  if (tokens <= 0 || window <= 0) {
    return { text: "No usage yet", percent: null as number | null };
  }
  return {
    text: `${tokens.toLocaleString()} / ${window.toLocaleString()} tokens`,
    percent: Math.min(100, (tokens / window) * 100),
  };
});

const unavailable = computed(() => props.autoCompaction === null);

async function toggle(): Promise<void> {
  if (busy.value !== null || props.autoCompaction === null) {
    return;
  }
  busy.value = "toggle";
  try {
    await setAutoCompaction(props.thread, !props.autoCompaction);
  } catch (cause) {
    emit("failed", describe(cause));
  } finally {
    busy.value = null;
  }
}

async function compact(): Promise<void> {
  if (busy.value !== null) {
    return;
  }
  busy.value = "compact";
  try {
    // Trimmed: an empty box means "compact now", not "compact with empty instructions".
    const text = instructions.value.trim();
    await compactSession(props.thread, text === "" ? null : text);
    instructions.value = "";
  } catch (cause) {
    emit("failed", describe(cause));
  } finally {
    busy.value = null;
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
  <div class="flex flex-col gap-2">
    <!-- The readout, as the reference lays it out: the label left, its value right, and the
         bar under both with no box around any of it. -->
    <div class="flex flex-col gap-1.5 px-1 pt-1">
      <div class="flex items-baseline justify-between gap-4 px-1.5">
        <span class="text-[12.5px] text-fg">Usage</span>
        <span class="text-[12.5px] text-dim">{{ summary.text }}</span>
      </div>
      <div class="h-1.5 overflow-hidden rounded-full bg-raised">
        <div
          class="h-full rounded-full bg-accent"
          :style="{ width: `${summary.percent ?? 0}%` }"
        />
      </div>
      <p class="px-1.5 text-[11px] text-faint">
        {{ percentLabel(summary.percent) }} of the window
      </p>
    </div>

    <div class="flex items-center gap-3 px-1.5 py-1">
      <span class="flex min-w-0 flex-col">
        <span class="text-[12.5px] text-fg">auto-compact</span>
        <span class="text-[11.5px] text-dim">when the window fills</span>
      </span>
      <!--
        A switch, not an "on"/"off" pill: the reference draws one, and the state of a mode is
        something a control should show rather than spell out. `—` is kept for the case where the
        engine has not answered yet, because a switch that claims "off" before it knows is a lie.
      -->
      <span v-if="unavailable" class="ml-auto text-[11.5px] text-faint">—</span>
      <Toggle
        v-else
        class="ml-auto"
        :checked="autoCompaction === true"
        :disabled="busy !== null"
        label="auto-compact"
        @change="toggle"
      />
    </div>

    <div class="flex flex-col gap-2 px-1.5 pb-1">
      <input
        v-model="instructions"
        spellcheck="false"
        placeholder="instructions for the summary (optional)"
        class="w-full rounded-[6px] bg-raised px-2.5 py-2 text-[12.5px] text-fg outline-none placeholder:text-faint"
        @keydown.enter="compact"
      />
      <button
        type="button"
        :disabled="busy !== null || props.compacting"
        class="w-full rounded-[6px] bg-accent px-2.5 py-1 text-[12px] font-medium text-canvas disabled:opacity-40"
        @click="compact"
      >
        {{ props.compacting ? "compacting…" : busy === "compact" ? "asking…" : "compact now" }}
      </button>
    </div>
  </div>
</template>
