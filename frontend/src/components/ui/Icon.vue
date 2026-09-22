<script setup lang="ts">
/** One consistently configured icon from the app's Lucide adapter. */
import { computed } from "vue";

import { ICONS, SOLID, type IconName } from "../../lib/icons";

const props = defineProps<{
  name: IconName;
  /** Tailwind size classes. 16px is the row default; the titlebar uses 16 too. */
  size?: string;
  /** Force the solid rendering an outlined glyph would otherwise get. */
  solid?: boolean;
}>();

const icon = computed(() => ICONS[props.name]);
const filled = computed(() => props.solid === true || SOLID.has(props.name));
</script>

<template>
  <component
    :is="icon"
    :class="props.size ?? 'h-4 w-4'"
    :data-icon="props.name"
    :fill="filled ? 'currentColor' : 'none'"
    :stroke-width="1.7"
    aria-hidden="true"
    focusable="false"
  />
</template>
