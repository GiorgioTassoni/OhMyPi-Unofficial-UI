<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";

const props = defineProps<{ start: number }>();
const emit = defineEmits<{ measured: [start: number, height: number] }>();
const element = ref<HTMLElement | null>(null);
let observer: ResizeObserver | null = null;
let last = 0;
function measure(): void {
  const height = element.value?.getBoundingClientRect().height ?? 0;
  if (height > 0 && Math.abs(height - last) > 0.5) {
    last = height;
    emit("measured", props.start, height);
  }
}
onMounted(() => {
  measure();
  if (typeof ResizeObserver !== "undefined") {
    observer = new ResizeObserver(measure);
    if (element.value) observer.observe(element.value);
  }
});
onBeforeUnmount(() => observer?.disconnect());
</script>

<template>
  <div ref="element" class="pb-4" :data-turn-start="start"><slot /></div>
</template>
