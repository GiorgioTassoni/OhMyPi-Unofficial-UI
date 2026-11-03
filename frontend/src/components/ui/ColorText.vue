<script setup lang="ts">
import { computed } from "vue";
import { parseTextSegments } from "../../lib/colors";

const props = defineProps<{
  text: string;
}>();

const segments = computed(() => parseTextSegments(props.text));
</script>

<template>
  <span>
    <template v-for="(seg, i) in segments" :key="i">
      <span v-if="seg.isColor" class="inline-flex items-center gap-1 font-mono font-medium">
        <span
          class="color-swatch"
          :style="{ backgroundColor: seg.text }"
          aria-hidden="true"
        />
        <span>{{ seg.text }}</span>
      </span>
      <span v-else>{{ seg.text }}</span>
    </template>
  </span>
</template>
