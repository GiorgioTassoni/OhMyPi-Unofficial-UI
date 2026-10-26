<script setup lang="ts">
import Icon from "./ui/Icon.vue";

defineProps<{
  remaining: number;
  loading: boolean;
}>();

const emit = defineEmits<{
  (event: "load"): void;
}>();
</script>

<template>
  <div
    v-if="remaining > 0"
    class="mb-4 flex items-center justify-between rounded-[8px] border border-line bg-raised/40 px-3.5 py-2 text-[13px] text-dim transition-colors hover:bg-raised/70"
    data-archived-banner
  >
    <div class="flex items-center gap-2">
      <Icon name="clock" class="h-4 w-4 text-faint" />
      <span>
        <strong class="font-medium text-fg">{{ remaining }}</strong>
        {{ remaining === 1 ? "earlier turn archived" : "earlier turns archived" }}
      </span>
    </div>
    <button
      type="button"
      class="inline-flex items-center gap-1.5 rounded-[6px] bg-accent/15 px-2.5 py-1 text-[12px] font-medium text-accent hover:bg-accent/25 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent disabled:opacity-50 transition-colors cursor-pointer"
      :disabled="loading"
      @click="emit('load')"
    >
      <Icon v-if="loading" name="refresh" class="h-3.5 w-3.5 animate-spin" />
      <Icon v-else name="arrow-up" class="h-3.5 w-3.5" />
      <span>{{ loading ? "Loading…" : `Load earlier turns (${Math.min(5, remaining)})` }}</span>
    </button>
  </div>
</template>
