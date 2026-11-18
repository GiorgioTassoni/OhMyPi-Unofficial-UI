<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { getConfiguredProviders, getOAuthUsageLimits, type OAuthUsageLimitDto } from "../bridge";

const CACHE_KEY = "omp:oauth-limits-cache";

function readCachedLimits(): OAuthUsageLimitDto[] {
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? (parsed as OAuthUsageLimitDto[]) : [];
  } catch {
    return [];
  }
}

function saveCachedLimits(next: OAuthUsageLimitDto[]): void {
  try {
    if (next.length === 0) localStorage.removeItem(CACHE_KEY);
    else localStorage.setItem(CACHE_KEY, JSON.stringify(next));
  } catch {}
}

const limits = ref<OAuthUsageLimitDto[]>(readCachedLimits());
const activeIndex = ref(0);
let pollTimer: ReturnType<typeof setInterval> | null = null;

const currentProvider = computed(() => limits.value[activeIndex.value] ?? null);

async function refreshLimits() {
  try {
    const res = await getOAuthUsageLimits();
    const valid = res.filter((entry) => entry.windows.length > 0);
    if (valid.length > 0) {
      limits.value = valid;
      saveCachedLimits(valid);
      if (activeIndex.value >= valid.length) {
        activeIndex.value = Math.max(0, valid.length - 1);
      }
      return;
    }

    const configured = await getConfiguredProviders().catch(() => null);
    if (configured !== null && !configured.some((p) => p.credentialType === "oauth")) {
      limits.value = [];
      saveCachedLimits([]);
    }
  } catch {
    // Preserve last valid cached snapshot across transient network failures.
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
  <div v-if="limits.length > 0" class="relative border-t border-line/50 px-3 py-2.5 text-[11.5px] bg-rail select-none">
    <div class="flex items-center justify-between gap-2 mb-2">
      <span class="truncate font-medium text-dim">
        {{ currentProvider?.displayName || 'OAuth Limits' }}
      </span>

      <div v-if="limits.length > 1" class="flex items-center gap-1">
        <button
          v-for="(p, idx) in limits"
          :key="p.provider"
          type="button"
          class="rounded-[4px] px-1.5 py-0.5 text-[10px] font-medium transition-colors"
          :class="activeIndex === idx ? 'bg-selected text-fg' : 'text-faint hover:text-dim'"
          @click="activeIndex = idx"
        >
          {{ p.provider === 'openai-codex' ? 'ChatGPT' : p.displayName }}
        </button>
      </div>

      <span
        v-else-if="currentProvider?.planType"
        class="rounded-[4px] border border-line bg-raised px-1.5 py-0.2 text-[10px] font-medium text-faint uppercase"
      >
        {{ currentProvider.planType }}
      </span>
    </div>

    <div v-if="currentProvider" class="flex flex-col gap-2">
      <div
        v-for="w in currentProvider.windows"
        :key="w.label"
        class="flex items-center justify-between gap-2"
      >
        <div class="flex flex-col min-w-0">
          <span class="text-[11.5px] text-fg/85 leading-tight truncate">
            {{ w.label.includes('5h') ? '5h limit' : 'Weekly limit' }}
          </span>
          <span v-if="w.resetAfterSeconds" class="text-[10.5px] text-faint leading-tight mt-0.5">
            Resets in {{ formatRemainingTime(w.resetAfterSeconds) }}
          </span>
        </div>

        <div class="flex items-center gap-1.5 shrink-0">
          <span class="text-[11.5px] font-medium text-fg/85 tabular-nums">
            {{ remainingLabel(w.usedPercent) }}%
          </span>
          <svg viewBox="0 0 16 16" class="h-3.5 w-3.5 shrink-0" aria-hidden="true">
            <circle
              cx="8"
              cy="8"
              r="6"
              fill="none"
              stroke="var(--color-line-strong)"
              stroke-width="1.6"
            />
            <circle
              cx="8"
              cy="8"
              r="6"
              fill="none"
              :stroke="strokeColor(w.usedPercent)"
              :stroke-dasharray="ringDash(w.usedPercent)"
              stroke-width="1.6"
              stroke-linecap="round"
              transform="rotate(-90 8 8)"
            />
          </svg>
        </div>
      </div>
    </div>
  </div>
</template>
