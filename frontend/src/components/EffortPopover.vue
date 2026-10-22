<script setup lang="ts">
/**
 * Stepped effort slider popover (`docs/12` §7.6).
 *
 * Replaces the static list picker with a modern stepped horizontal slider inspired by the
 * model effort controls in contemporary LLM interfaces, while honoring Oh My Pi's dark
 * surface tokens and typography.
 *
 * Supports:
 * - Dynamic model capability detection (filtering to supported discrete steps).
 * - Smooth pointer dragging with instant thumb feedback and debounced commit on release.
 * - Direct tap on ticks and tick labels.
 * - First-class Auto mode toggle with active indicator and per-turn explanation.
 * - Keyboard navigation (Arrow keys, Home, End, Enter).
 */
import { computed, onUnmounted, ref, watch } from "vue";
import { setThinkingLevel } from "../bridge";
import {
  effortStepShortLabel,
  effortStepTitle,
  supportedEffortSteps,
  type ModelOption,
} from "../lib/chips";
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
  (event: "changed"): void;
}>();

const switching = ref<string | null>(null);
const isDragging = ref(false);
const dragIndex = ref<number | null>(null);
const trackRef = ref<HTMLElement | null>(null);

/** Optimistic local level state so pointer releases don't snap back while RPC finishes. */
const selectedLevel = ref<string>(props.level ?? "off");

watch(
  () => props.level,
  (newLevel) => {
    if (!isDragging.value && switching.value === null) {
      selectedLevel.value = newLevel ?? "off";
    }
  },
  { immediate: true }
);

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      selectedLevel.value = props.level ?? "off";
      dragIndex.value = null;
    }
  }
);

const steps = computed(() => supportedEffortSteps(props.model));
const isReasoningSupported = computed(() => steps.value.length > 1);
const isAuto = computed(() => {
  if (dragIndex.value !== null) return false;
  return selectedLevel.value === "auto";
});

const modelDisplayName = computed(() => {
  if (!props.model) return "Reasoning model";
  if (props.model.name && props.model.name.trim() !== "") {
    return props.model.name;
  }
  const id = props.model.id || "";
  const slash = id.lastIndexOf("/");
  return slash >= 0 ? id.slice(slash + 1) : id || "Model";
});

const headerSubtitle = computed(() => {
  if (!isReasoningSupported.value) {
    return `${modelDisplayName.value} · no reasoning`;
  }
  if (isAuto.value && dragIndex.value === null) {
    return `adaptive per turn · ${modelDisplayName.value}`;
  }
  return modelDisplayName.value;
});

const displayedLevel = computed(() => {
  if (dragIndex.value !== null) {
    return steps.value[dragIndex.value] ?? "off";
  }
  return selectedLevel.value;
});

const activeIndex = computed(() => {
  if (dragIndex.value !== null) {
    return dragIndex.value;
  }
  if (isAuto.value) {
    const medIdx = steps.value.indexOf("medium");
    return medIdx >= 0 ? medIdx : Math.floor(steps.value.length / 2);
  }
  const idx = steps.value.indexOf(selectedLevel.value);
  return idx >= 0 ? idx : 0;
});

const activeStepRatio = computed(() => {
  const count = steps.value.length;
  if (count <= 1) return 0;
  return activeIndex.value / (count - 1);
});

function getStepFromPointer(event: PointerEvent): number {
  const track = trackRef.value;
  const count = steps.value.length;
  if (!track || count <= 1) {
    return 0;
  }
  const rect = track.getBoundingClientRect();
  const inset = 14;
  const usableWidth = Math.max(1, rect.width - inset * 2);
  const offsetX = event.clientX - rect.left - inset;
  const ratio = Math.max(0, Math.min(1, offsetX / usableWidth));
  const nearest = Math.round(ratio * (count - 1));
  return Math.max(0, Math.min(count - 1, nearest));
}

function onPointerDown(event: PointerEvent): void {
  if (switching.value !== null || !isReasoningSupported.value) {
    return;
  }
  if (event.button !== 0) {
    return;
  }
  event.preventDefault();
  isDragging.value = true;
  dragIndex.value = getStepFromPointer(event);

  window.addEventListener("pointermove", onPointerMove);
  window.addEventListener("pointerup", onPointerUp);
  window.addEventListener("pointercancel", onPointerUp);
}

function onPointerMove(event: PointerEvent): void {
  if (!isDragging.value) {
    return;
  }
  dragIndex.value = getStepFromPointer(event);
}

async function onPointerUp(event: PointerEvent): Promise<void> {
  if (!isDragging.value) {
    return;
  }
  isDragging.value = false;
  window.removeEventListener("pointermove", onPointerMove);
  window.removeEventListener("pointerup", onPointerUp);
  window.removeEventListener("pointercancel", onPointerUp);

  const idx = dragIndex.value ?? getStepFromPointer(event);
  dragIndex.value = null;
  const target = steps.value[idx];
  if (target !== undefined) {
    await choose(target);
  }
}

onUnmounted(() => {
  window.removeEventListener("pointermove", onPointerMove);
  window.removeEventListener("pointerup", onPointerUp);
  window.removeEventListener("pointercancel", onPointerUp);
});

function onKeydown(event: KeyboardEvent): void {
  if (!isReasoningSupported.value || switching.value !== null) {
    return;
  }
  let nextIdx = activeIndex.value;
  if (event.key === "ArrowLeft" || event.key === "ArrowDown") {
    event.preventDefault();
    nextIdx = Math.max(0, activeIndex.value - 1);
  } else if (event.key === "ArrowRight" || event.key === "ArrowUp") {
    event.preventDefault();
    nextIdx = Math.min(steps.value.length - 1, activeIndex.value + 1);
  } else if (event.key === "Home") {
    event.preventDefault();
    nextIdx = 0;
  } else if (event.key === "End") {
    event.preventDefault();
    nextIdx = steps.value.length - 1;
  } else if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    emit("close");
    return;
  } else {
    return;
  }

  const target = steps.value[nextIdx];
  if (target && target !== selectedLevel.value) {
    choose(target);
  }
}

async function choose(level: string): Promise<void> {
  if (switching.value !== null) {
    return;
  }
  if (level === selectedLevel.value && level === props.level) {
    return;
  }
  const previousLevel = selectedLevel.value;
  selectedLevel.value = level;
  switching.value = level;
  try {
    await setThinkingLevel(props.thread, level);
    emit("changed");
  } catch (cause) {
    selectedLevel.value = previousLevel;
    emit("failed", describe(cause));
  } finally {
    switching.value = null;
  }
}

async function toggleAuto(): Promise<void> {
  if (switching.value !== null || !isReasoningSupported.value) {
    return;
  }
  if (selectedLevel.value === "auto") {
    const target = steps.value.includes("medium")
      ? "medium"
      : steps.value[Math.min(2, steps.value.length - 1)];
    await choose(target);
  } else {
    await choose("auto");
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
  <div
    class="flex flex-col gap-2 p-3 select-none outline-none"
    role="slider"
    tabindex="0"
    :aria-label="`Thinking effort for ${modelDisplayName}`"
    :aria-valuemin="0"
    :aria-valuemax="steps.length - 1"
    :aria-valuenow="activeIndex"
    :aria-valuetext="displayedLevel"
    @keydown="onKeydown"
  >
    <!-- Header: icon, current level title, model name, auto button, esc -->
    <div class="flex items-center justify-between gap-2">
      <div class="flex items-center gap-2 min-w-0">
        <Icon name="zap" class="h-4 w-4 text-accent shrink-0" />
        <div class="flex flex-col min-w-0">
          <div class="flex items-center gap-1.5 leading-tight">
            <span class="text-[13px] font-semibold text-accent truncate">
              {{ effortStepTitle(displayedLevel) }}
            </span>
            <span v-if="switching !== null" class="text-[10px] text-faint animate-pulse">
              saving…
            </span>
          </div>
          <span class="text-[10.5px] text-faint truncate leading-tight mt-0.5">
            {{ headerSubtitle }}
          </span>
        </div>
      </div>

      <div class="flex items-center gap-1.5 shrink-0">
        <button
          type="button"
          :disabled="switching !== null || !isReasoningSupported"
          :class="[
            'flex items-center gap-1 rounded-[6px] px-2 py-0.5 text-[11px] font-medium transition-colors',
            isAuto && dragIndex === null
              ? 'bg-accent/20 text-accent border border-accent/40 shadow-xs'
              : 'text-faint hover:text-fg hover:bg-raised/70 border border-line/30 disabled:opacity-30'
          ]"
          @click="toggleAuto"
        >
          <Icon name="rotate-ccw" class="h-3 w-3" />
          <span>Auto</span>
        </button>

        <button
          type="button"
          class="rounded-[6px] px-1.5 py-0.5 text-[10.5px] text-faint/80 hover:text-fg hover:bg-raised/70 transition-colors"
          aria-label="Close"
          @click="emit('close')"
        >
          esc
        </button>
      </div>
    </div>

    <!-- Stepped Slider -->
    <div class="pt-1.5 pb-0.5">
      <!-- Interactive track container -->
      <div
        ref="trackRef"
        class="relative h-7 w-full rounded-full bg-raised/90 border border-line/40 transition-opacity"
        :class="[
          !isReasoningSupported ? 'opacity-30 cursor-not-allowed' : 'cursor-pointer hover:border-line-strong',
          isDragging ? 'cursor-grabbing' : ''
        ]"
        @pointerdown="onPointerDown"
      >
        <!-- Filled progress bar -->
        <div
          class="absolute top-0 bottom-0 left-0 rounded-full"
          :class="[
            isAuto && dragIndex === null
              ? 'bg-accent/35 border-r border-accent/50'
              : 'bg-accent',
            isDragging ? '' : 'transition-[width] duration-150 ease-out'
          ]"
          :style="{
            width: !isReasoningSupported || (displayedLevel === 'off' && !isAuto)
              ? '0%'
              : `calc(14px + ${activeStepRatio} * (100% - 28px))`
          }"
        />

        <!-- Step tick dots -->
        <div
          v-for="(step, idx) in steps"
          :key="step"
          class="absolute top-1/2 -translate-y-1/2 -translate-x-1/2 rounded-full pointer-events-none transition-colors"
          :class="[
            (idx <= activeIndex && displayedLevel !== 'off') || (isAuto && idx <= activeIndex && dragIndex === null)
              ? 'w-1.5 h-1.5 bg-white/70'
              : 'w-1.5 h-1.5 bg-fg/20'
          ]"
          :style="{ left: `calc(14px + ${(steps.length > 1 ? idx / (steps.length - 1) : 0)} * (100% - 28px))` }"
        />

        <!-- Thumb disc -->
        <div
          v-if="isReasoningSupported"
          class="absolute top-1/2 -translate-y-1/2 -translate-x-1/2 w-[22px] h-[22px] rounded-full bg-white shadow-md shadow-black/50 pointer-events-none flex items-center justify-center"
          :class="[
            isDragging ? 'scale-105 shadow-lg' : 'transition-transform hover:scale-105',
            isAuto && dragIndex === null ? 'ring-2 ring-accent ring-offset-2 ring-offset-surface' : ''
          ]"
          :style="{ left: `calc(14px + ${activeStepRatio} * (100% - 28px))` }"
        >
          <!-- Subtle center dot inside thumb for precision -->
          <div
            class="w-1.5 h-1.5 rounded-full"
            :class="isAuto && dragIndex === null ? 'bg-accent' : 'bg-black/30'"
          />
        </div>
      </div>

      <!-- Tick Labels below track -->
      <div v-if="steps.length > 1" class="relative w-full h-5 mt-1.5">
        <button
          v-for="(step, idx) in steps"
          :key="step"
          type="button"
          :disabled="switching !== null"
          class="absolute -translate-x-1/2 text-[10.5px] transition-colors hover:text-fg font-mono disabled:opacity-40"
          :class="[
            idx === activeIndex && !isAuto && dragIndex === null
              ? 'text-accent font-semibold'
              : 'text-faint'
          ]"
          :style="{ left: `calc(14px + ${(idx / (steps.length - 1))} * (100% - 28px))` }"
          @click.stop="choose(step)"
        >
          {{ effortStepShortLabel(step) }}
        </button>
      </div>
    </div>

    <!-- Quiet explanation footer -->
    <p class="pt-0.5 text-center text-[10.5px] text-faint select-none">
      {{
        isAuto && dragIndex === null
          ? "Auto evaluates complexity dynamically per turn"
          : isReasoningSupported
            ? "Drag slider or tap steps to adjust effort"
            : "Thinking is not supported by this model"
      }}
    </p>
  </div>
</template>
