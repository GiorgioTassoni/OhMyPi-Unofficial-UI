<script setup lang="ts">
/**
 * An anchored panel the composer's chips open (`docs/12` §7).
 *
 * Teleported and measured rather than nested: the composer lives inside a scrolling
 * column, and an absolutely-positioned child of it would be clipped by that column's
 * `overflow` exactly when the popover is tall enough to matter. So the trigger is
 * measured on open, the panel is placed in viewport coordinates, and its height
 * is capped by the space available on the requested side of the trigger.
 *
 * One at a time is the parent's job, not this component's: the parent owns *which*
 * chip is open, which is also what makes reopening the same chip a toggle rather than
 * a fight between two components over a boolean.
 *
 * Closing: `Escape` and a press outside, both of which a user reaches for before they
 * look for a close button. The trigger sits inside the wrapper, so pressing it again
 * is *not* an outside press — the parent decides whether that reopens or toggles.
 */
import { onUnmounted, ref, watch } from "vue";

const props = defineProps<{
  open: boolean;
  /** The panel's name, for its header and for assistive technology. */
  label: string;
  /** Panel width in pixels; the popovers are wide enough for a model name and a tag. */
  width?: number;
  /** Settings pickers open below their selector; composer chips keep opening above. */
  side?: "above" | "below";
  /** Use the selector's measured width instead of a fixed panel width. */
  matchTriggerWidth?: boolean;
  /** Focus the first control on open — right for a popover you came to type in. */
  focusOnOpen?: boolean;
}>();

const emit = defineEmits<{ (event: "close"): void }>();

const trigger = ref<HTMLElement | null>(null);
const panel = ref<HTMLElement | null>(null);

/** Viewport coordinates, recomputed whenever the panel opens or the window moves. */
const placement = ref<{ left: string; top: string; bottom: string; width: string; maxHeight: string }>({
  left: "0px",
  top: "auto",
  bottom: "0px",
  width: "320px",
  maxHeight: "60vh",
});

function place(event?: Event): void {
  // Ignore scrolls that originate inside the panel (e.g. scrolling the model list):
  // re-measuring the trigger every time the *content* scrolls causes a reactive
  // re-render that steals pointer events from rows under the cursor.
  if (event instanceof Event && panel.value?.contains(event.target as Node)) {
    return;
  }
  const rect = trigger.value?.getBoundingClientRect();
  if (rect === undefined) {
    return;
  }
  const width = props.matchTriggerWidth === true ? rect.width : (props.width ?? 320);
  // Inside the viewport on both sides: a chip near the right edge would otherwise
  // open a panel that runs off it.
  const left = Math.min(Math.max(8, rect.left), Math.max(8, window.innerWidth - width - 8));
  placement.value = {
    left: `${left}px`,
    top: props.side === "below" ? `${rect.bottom + 8}px` : "auto",
    bottom: props.side === "below" ? "auto" : `${window.innerHeight - rect.top + 8}px`,
    width: `${width}px`,
    // The panel scrolls within the space on its chosen side of the trigger.
    maxHeight: `${props.side === "below"
      ? Math.max(0, window.innerHeight - rect.bottom - 16)
      : Math.max(160, rect.top - 16)}px`,
  };
}

function onPointerDown(event: PointerEvent): void {
  const target = event.target as Node | null;
  if (target === null) {
    return;
  }
  if (panel.value?.contains(target) === true || trigger.value?.contains(target) === true) {
    return;
  }
  emit("close");
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key !== "Escape") {
    return;
  }
  // Capture phase, so this runs before the composer's own Escape — which stops a turn.
  // An open panel is the topmost thing on screen, so it is the one Escape should
  // close; without this, dismissing the popover would also abort a running turn.
  event.preventDefault();
  event.stopPropagation();
  emit("close");
}

function listen(listening: boolean): void {
  // Written out rather than dispatched through an indexed `document[method]`: the
  // dynamic form erases the handler's type, and these four listeners are the whole
  // subscription surface anyway.
  if (listening) {
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKeydown, true);
    document.addEventListener("scroll", place, true);
    window.addEventListener("resize", place);
    return;
  }
  document.removeEventListener("pointerdown", onPointerDown, true);
  document.removeEventListener("keydown", onKeydown, true);
  // Capture phase on the document: a scroll inside the conversation must move the
  // panel with its trigger, and scroll events do not bubble.
  document.removeEventListener("scroll", place, true);
  window.removeEventListener("resize", place);
}

watch(
  () => props.open,
  (open) => {
    if (!open) {
      listen(false);
      return;
    }
    place();
    listen(true);
    if (props.focusOnOpen !== false) {
      // After the panel exists in the DOM, which is the next tick.
      requestAnimationFrame(() => {
        panel.value
          ?.querySelector<HTMLElement>("input, button, [tabindex]")
          ?.focus();
      });
    }
  },
  { immediate: true },
);

onUnmounted(() => listen(false));
</script>

<template>
  <span ref="trigger" class="inline-flex">
    <slot name="trigger" />
  </span>

  <Teleport to="body">
    <div
      v-if="open"
      ref="panel"
      role="dialog"
      :aria-label="label"
      class="fixed z-30 flex flex-col overflow-hidden rounded-[10px] border border-line bg-surface shadow-xl shadow-black/50"
      :style="placement"
    >
      <!--
        The panel's name, in the reference's own register: a small dim label at the top left
        with no rule under it, so the first row reads as part of the same block. The `esc` hint
        is kept — this window closes a popover with Escape and nothing else says so — but it is
        as quiet as the label.
      -->
      <header class="flex items-center gap-2 px-3.5 pb-0.5 pt-3">
        <span class="text-[11px] text-faint">{{ label }}</span>
        <button
          class="ml-auto text-[10.5px] text-faint/80 hover:text-dim"
          type="button"
          @click="emit('close')"
        >
          esc
        </button>
      </header>
      <div class="min-h-0 flex-1 overflow-auto p-1.5">
        <slot />
      </div>
    </div>
  </Teleport>
</template>
