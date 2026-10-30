<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { getOAuthUsageLimits, type OAuthUsageLimitDto } from "../bridge";
import Icon from "./ui/Icon.vue";

const limits = ref<OAuthUsageLimitDto[]>([]);
const activeIndex = ref(0);
const isExpanded = ref(true);
let pollTimer: ReturnType<typeof setInterval> | null = null;

const currentProvider = computed(() => limits.value[activeIndex.value] ?? null);

async function refreshLimits() {
  try {
    const res = await getOAuthUsageLimits();
    limits.value = res;
    if (activeIndex.value >= res.length) {
      activeIndex.value = Math.max(0, res.length - 1);
    }
  } catch {
    limits.value = [];
  }
}

function formatRemainingTime(seconds: number | null): string {
  if (seconds === null || seconds <= 0) return "ready";
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (hours > 24) {
    const days = Math.round(hours / 24);
    return `${days}d`;
  }
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${minutes}m`;
}

function remainingLabel(usedPercent: number): number {
  return Math.max(0, 100 - Math.round(usedPercent));
}

function strokeColor(usedPercent: number): string {
  const left = 100 - usedPercent;
  if (left <= 15) return "var(--color-err)";
  if (left <= 30) return "var(--color-warn)";
  return "var(--color-ok)";
}

function ringDash(usedPercent: number): string {
  const radius = 6;
  const circumference = 2 * Math.PI * radius;
  const fraction = Math.max(0, Math.min(100, 100 - usedPercent)) / 100;
  const filled = fraction * circumference;
  return `${filled} ${circumference}`;
}

onMounted(() => {
  void refreshLimits();
  pollTimer = setInterval(refreshLimits, 60_000);
});

onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer);
});
</script>

<template>
  <div v-if="limits.length > 0" class="relative px-3 pt-3 pb-2 text-[12px] bg-rail">
    <div
      class="pointer-events-none absolute -top-10 inset-x-0 h-10 z-10 select-none"
      style="background: linear-gradient(to top, var(--color-rail) 0%, var(--color-rail) 28%, color-mix(in srgb, var(--color-rail) 70%, transparent) 65%, transparent 100%)"
      aria-hidden="true"
    />

    <!-- Header: Provider title + collapsable toggle / tabs -->
    <div class="flex items-center justify-between gap-1 mb-2">
      <button
        type="button"
        class="flex items-center gap-1.5 font-medium text-fg/90 hover:text-fg transition-colors select-none text-left"
        @click="isExpanded = !isExpanded"
      >
        <Icon
          :name="isExpanded ? 'chevron-down' : 'chevron-right'"
          class="h-3 w-3 text-faint shrink-0"
        />
        <span class="truncate">{{ currentProvider?.displayName || 'OAuth Limits' }}</span>
      </button>

      <!-- Tabs if more than 1 OAuth provider -->
      <div v-if="limits.length > 1 && isExpanded" class="flex items-center gap-1">
        <button
          v-for="(p, idx) in limits"
          :key="p.provider"
          type="button"
          class="rounded-[4px] px-1.5 py-0.5 text-[10px] font-medium transition-colors"
          :class="activeIndex === idx ? 'bg-accent/20 text-accent font-semibold' : 'text-faint hover:text-dim'"
          @click="activeIndex = idx"
        >
          {{ p.provider === 'openai-codex' ? 'ChatGPT' : p.displayName }}
        </button>
      </div>

      <span
        v-else-if="currentProvider?.planType && isExpanded"
        class="rounded-[4px] bg-raised/80 border border-line/50 px-1.5 py-0.5 text-[10px] font-medium text-faint uppercase"
      >
        {{ currentProvider.planType }}
      </span>
    </div>

    <!-- Collapsible content: Clean rows with title, reset subtitle, % and circular ring (matching second screenshot) -->
    <div v-if="isExpanded && currentProvider" class="flex flex-col gap-2.5 pt-0.5">
      <div
        v-for="w in currentProvider.windows"
        :key="w.label"
        class="flex items-center justify-between gap-2"
      >
        <div class="flex flex-col min-w-0">
          <span class="text-[12px] font-medium text-fg/90 leading-tight">
            {{ w.label.includes('5h') ? 'Five Hour Limit Remaining' : 'Weekly Limit Remaining' }}
          </span>
          <span v-if="w.resetAfterSeconds" class="text-[11px] text-faint leading-tight mt-0.5">
            Resets in {{ formatRemainingTime(w.resetAfterSeconds) }}
          </span>
        </div>

        <div class="flex items-center gap-1.5 shrink-0">
          <span class="text-[12px] font-medium text-dim">
            {{ remainingLabel(w.usedPercent) }}%
          </span>
          <svg viewBox="0 0 16 16" class="h-3.5 w-3.5 shrink-0" aria-hidden="true">
            <circle
              cx="8"
              cy="8"
              r="6"
              fill="none"
              stroke="var(--color-line-strong)"
              stroke-width="1.8"
            />
            <circle
              cx="8"
              cy="8"
              r="6"
              fill="none"
              :stroke="strokeColor(w.usedPercent)"
              :stroke-dasharray="ringDash(w.usedPercent)"
              stroke-width="1.8"
              stroke-linecap="round"
              transform="rotate(-90 8 8)"
            />
          </svg>
        </div>
      </div>
    </div>
  </div>
</template>
