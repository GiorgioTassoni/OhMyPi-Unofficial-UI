<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import Icon from "./ui/Icon.vue";
import { computeQuotePopup, type QuotePopupPosition } from "../lib/selectionQuote";

const props = defineProps<{
  container: HTMLElement | null;
  active: boolean;
}>();

const emit = defineEmits<{
  (e: "quote", text: string): void;
}>();

const quotePopup = ref<QuotePopupPosition | null>(null);
let listenersAttached = false;

function update(): void {
  if (!props.active || !props.container) {
    quotePopup.value = null;
    return;
  }
  quotePopup.value = computeQuotePopup(window.getSelection(), props.container);
}

function onSelectionChange(): void {
  if (quotePopup.value && window.getSelection()?.isCollapsed) {
    quotePopup.value = null;
  }
}

function onQuoteClick(): void {
  if (!quotePopup.value) return;
  const text = quotePopup.value.text;
  quotePopup.value = null;
  window.getSelection()?.removeAllRanges();
  emit("quote", text);
}

function attachListeners(): void {
  if (listenersAttached || typeof document === "undefined") return;
  document.addEventListener("mouseup", update);
  document.addEventListener("selectionchange", onSelectionChange);
  document.addEventListener("keyup", update);
  listenersAttached = true;
}

function detachListeners(): void {
  if (!listenersAttached || typeof document === "undefined") return;
  document.removeEventListener("mouseup", update);
  document.removeEventListener("selectionchange", onSelectionChange);
  document.removeEventListener("keyup", update);
  listenersAttached = false;
}

watch(
  () => props.active,
  (isActive) => {
    if (isActive) {
      attachListeners();
    } else {
      quotePopup.value = null;
      detachListeners();
    }
  },
  { immediate: true },
);

onMounted(() => {
  if (props.active) attachListeners();
});

onUnmounted(() => {
  detachListeners();
});
</script>

<template>
  <Transition name="quote-fade">
    <button
      v-if="quotePopup"
      type="button"
      class="absolute z-50 flex items-center gap-1.5 rounded-[8px] border border-line-strong/80 bg-raised px-3 py-1.5 text-[12px] font-medium text-fg shadow-xl shadow-black/50 backdrop-blur-md transition-all hover:border-accent hover:bg-surface hover:text-accent active:scale-95 cursor-pointer select-none"
      :style="{
        left: quotePopup.x + 'px',
        top: quotePopup.y + 'px',
        transform: quotePopup.above ? 'translate(-50%, -100%)' : 'translate(-50%, 0)',
      }"
      @mousedown.prevent="onQuoteClick"
    >
      <Icon name="text-quote" size="h-3.5 w-3.5" />
      Add in message
    </button>
  </Transition>
</template>

<style scoped>
.quote-fade-enter-active,
.quote-fade-leave-active {
  transition: opacity 0.12s ease, transform 0.12s ease;
}
.quote-fade-enter-from,
.quote-fade-leave-to {
  opacity: 0;
  transform: translate(-50%, -80%) scale(0.95);
}
</style>
