<script setup lang="ts">
/**
 * A switch, the way the reference draws one (`docs/reference/02-settings.webp`): a small pill
 * track with a knob, the accent while it is on.
 *
 * A primitive rather than a class string because it is the one control with *state* baked into
 * its geometry — the knob moves — and two components hand-rolling it is how the off state ends
 * up one pixel different from the on state. `role="switch"` rather than a checkbox: what it
 * turns on is a mode, not a form value.
 */
const props = defineProps<{
  checked: boolean;
  /** Held down while the engine is being asked; the answer is the next `checked`. */
  disabled?: boolean;
  /** What it turns on, for assistive technology. */
  label?: string;
}>();

const emit = defineEmits<{ change: [next: boolean] }>();
</script>

<template>
  <button
    type="button"
    role="switch"
    :aria-checked="props.checked"
    :aria-label="props.label"
    :disabled="props.disabled"
    class="relative h-[18px] w-8 shrink-0 rounded-full transition-colors disabled:opacity-40"
    :class="props.checked ? 'bg-accent' : 'bg-line-strong'"
    @click="emit('change', !props.checked)"
  >
    <span
      class="absolute top-[2px] size-[14px] rounded-full bg-fg transition-all"
      :class="props.checked ? 'left-[16px]' : 'left-[2px]'"
    />
  </button>
</template>
