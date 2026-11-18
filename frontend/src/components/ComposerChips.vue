<script setup lang="ts">
/**
 * The composer's chip row (`docs/12` §5.1): folder, mode, model, effort, context.
 *
 * Each chip opens one popover and owns the command that popover sends, so the
 * composer above stays about the message and this row stays about the session's
 * settings. Exactly one popover is open at a time because one `openChip` says which —
 * two booleans would let two panels be on screen at once.
 *
 * The `+` attach control belongs to `8d` and slots in at the row's left.
 */
import { computed, ref } from "vue";
import { setModel } from "../bridge";
import { modelKey, providerBadge } from "../lib/models";
import {
  modelChipLabel,
  modeLabel,
  percentLabel,
  ringDash,
  effortLabel,
  type ChipRow,
} from "../lib/chips";
import Popover from "./Popover.vue";
import Icon from "./ui/Icon.vue";
import ModelPicker from "./ModelPicker.vue";
import ModePopover from "./ModePopover.vue";
import EffortPopover from "./EffortPopover.vue";
import ContextPopover from "./ContextPopover.vue";

const props = defineProps<ChipRow & { thread: string; statusLine?: string | null | undefined }>();

const emit = defineEmits<{
  (event: "failed", message: string): void;
  /** The session answered differently than it did before, so the screen re-reads it. */
  (event: "changed"): void;
  /** Open another directory: a new thread there, since a session's cwd is fixed. */
  (event: "workspace", path: string): void;
  /** Open the app's own terminal in the session's directory (`docs/12` §11). */
  (event: "terminal", path: string): void;
  /** The favourites order changed and should be stored. */
  (event: "favourites", keys: string[]): void;
}>();

/** Which chip is open. One at a time, by construction. */
const openChip = ref<null | "mode" | "model" | "effort" | "context">(null);
const selecting = ref(false);

function toggle(chip: typeof openChip.value): void {
  openChip.value = openChip.value === chip ? null : chip;
}

const currentModel = computed(
  () =>
    (props.models ?? []).find(
      (option) =>
        option.provider === props.model?.provider && option.id === props.model?.id,
    ) ?? null,
);

const modelPopoverLabel = computed(() => {
  const label = modelChipLabel(props.model, props.models ?? []);
  return label !== "no model" ? `model · ${label}` : "model";
});

const contextPercent = computed(() => props.context?.percent ?? null);
const ring = computed(() => ringDash(contextPercent.value, 7));

async function chooseModel(choice: { provider: string; modelId: string }): Promise<void> {
  if (selecting.value) {
    return;
  }
  selecting.value = true;
  try {
    await setModel(props.thread, choice.provider, choice.modelId);
    openChip.value = null;
    emit("changed");
  } catch (cause) {
    // Shown where it happened rather than silently reverting: the chip keeps naming
    // the model the session is actually running.
    emit("failed", describe(cause));
  } finally {
    selecting.value = false;
  }
}

/**
 * The chip chrome: 11.5px, quiet, and only the surface's own hover.
 *
 * The reference has no borders on this row at all — the chips are labels with glyphs, and the
 * one that reads as a *place* (the folder) is the one with a pill behind it. That is the shape
 * borrowed here: rules everywhere would make the composer look like a toolbar.
 */
const CHIP =
  "flex items-center gap-1.5 rounded-[7px] px-2 py-1 text-[11.5px] text-dim transition-colors hover:bg-selected hover:text-fg active:scale-[0.98]";

function describe(cause: unknown): string {
  return typeof cause === "string"
    ? cause
    : cause instanceof Error
      ? cause.message
      : String(cause);
}
</script>

<template>
  <div class="flex flex-col">
    <!-- Optional extension status line (only shown when an extension pushes one) -->
    <div
      v-if="props.statusLine !== null && props.statusLine !== undefined"
      class="flex items-center px-3 pt-2 text-[11.5px] select-none"
    >
      <span
        data-status
        class="inline-flex items-center gap-1 rounded-[5px] border border-line/60 bg-surface/70 px-1.5 py-0.5 text-[11px] text-dim truncate max-w-[16rem]"
      >
        {{ props.statusLine }}
      </span>
    </div>

    <!-- Middle prompt area -->
    <slot />

    <!-- Bottom action toolbar inside the unified composer card -->
    <div class="flex items-center justify-between gap-2 px-2.5 pb-2 pt-1 text-[12px] select-none">
      <div class="flex items-center gap-1 min-w-0">
        <slot name="attach" />

        <Popover :open="openChip === 'mode'" label="approval mode" :width="360" @close="openChip = null">
          <template #trigger>
            <button
              type="button"
              :class="[
                CHIP,
                props.mode === 'yolo' ? 'text-warn hover:text-warn' : '',
              ]"
              @click="toggle('mode')"
            >
              {{ modeLabel(props.mode) }}
              <Icon name="chevron-down" class="h-3 w-3 text-faint" />
            </button>
          </template>
          <ModePopover
            :thread="props.thread"
            :open="openChip === 'mode'"
            :mode="props.mode"
            @close="openChip = null"
            @failed="emit('failed', $event)"
            @changed="emit('changed')"
          />
        </Popover>
      </div>

      <div class="flex items-center gap-1.5 ml-auto shrink-0">
        <Popover
          :open="openChip === 'model'"
          :label="modelPopoverLabel"
          :width="520"
          :focus-on-open="false"
          @close="openChip = null"
        >
          <template #trigger>
            <button
              type="button"
              :class="[CHIP, 'max-w-[17rem]']"
              @click="toggle('model')"
            >
              <span
                v-if="props.model?.provider"
                class="shrink-0 rounded-[4px] border border-line/70 bg-surface/80 px-1.5 py-0.2 text-[10.5px] font-medium text-dim"
              >
                {{ providerBadge(props.model.provider) }}
              </span>
              <span class="truncate font-medium text-fg/90">{{ modelChipLabel(props.model, props.models) }}</span>
              <Icon name="chevron-down" class="h-3 w-3 shrink-0 text-faint" />
            </button>
          </template>
          <ModelPicker
            :models="props.models"
            :favourites="props.favourites"
            :current="props.model === null ? null : modelKey(props.model)"
            :refreshing="props.refreshing"
            @select="chooseModel"
            @favourites="emit('favourites', $event)"
            @close="openChip = null"
          />
        </Popover>

        <Popover
          :open="openChip === 'effort'"
          label="effort"
          :width="320"
          :hide-header="true"
          @close="openChip = null"
        >
          <template #trigger>
            <button type="button" :class="CHIP" @click="toggle('effort')">
              {{ effortLabel(props.thinkingLevel) }}
              <Icon name="chevron-down" class="h-3 w-3 text-faint" />
            </button>
          </template>
          <EffortPopover
            :thread="props.thread"
            :open="openChip === 'effort'"
            :level="props.thinkingLevel"
            :model="currentModel"
            @close="openChip = null"
            @changed="emit('changed')"
            @failed="emit('failed', $event)"
          />
        </Popover>

        <Popover
          :open="openChip === 'context'"
          label="context"
          :width="360"
          @close="openChip = null"
        >
          <template #trigger>
            <button
              type="button"
              :class="CHIP"
              aria-label="Context usage"
              @click="toggle('context')"
            >
              <svg viewBox="0 0 20 20" class="h-3.5 w-3.5 shrink-0" aria-hidden="true">
                <circle cx="10" cy="10" r="7" fill="none" stroke="var(--color-line-strong)" stroke-width="2" />
                <circle
                  v-if="!ring.empty"
                  cx="10"
                  cy="10"
                  r="7"
                  fill="none"
                  :stroke-dasharray="ring.dash"
                  :stroke="(contextPercent ?? 0) >= 90 ? 'var(--color-err)' : (contextPercent ?? 0) >= 70 ? 'var(--color-warn)' : 'var(--color-accent)'"
                  stroke-width="2.5"
                  stroke-linecap="round"
                  transform="rotate(-90 10 10)"
                />
              </svg>
              <span class="tabular-nums">{{ percentLabel(contextPercent) }}</span>
            </button>
          </template>
          <ContextPopover
            :thread="props.thread"
            :open="openChip === 'context'"
            :context="props.context"
            :auto-compaction="props.autoCompaction"
            :compacting="props.compacting"
            @close="openChip = null"
            @changed="emit('changed')"
            @failed="emit('failed', $event)"
          />
        </Popover>

        <slot name="send-actions" />
      </div>
    </div>
  </div>
</template>
