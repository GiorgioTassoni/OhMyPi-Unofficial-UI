<script setup lang="ts">
import Icon from "../ui/Icon.vue";
import type { ProviderAccountDto } from "../../bridge";

const props = defineProps<{
  providers: ProviderAccountDto[];
  loading: boolean;
}>();

const emit = defineEmits<{
  (e: "add"): void;
  (e: "removeCredential", id: number): void;
  (e: "removeCustom", providerId: string): void;
  (e: "refresh"): void;
  (e: "openTerminalLogin"): void;
}>();

function formatDate(epochSec?: number | null): string {
  if (!epochSec) return "";
  try {
    return new Date(epochSec * 1000).toLocaleDateString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  } catch {
    return "";
  }
}

function handleRemove(item: ProviderAccountDto): void {
  if (item.isCustom) {
    emit("removeCustom", item.provider);
  } else if (item.id != null) {
    emit("removeCredential", item.id);
  }
}
</script>

<template>
  <div class="mb-5 rounded-[10px] border border-line bg-surface p-4 shadow-sm">
    <div class="flex flex-wrap items-center justify-between gap-3 border-b border-line/60 pb-3">
      <div>
        <h3 class="text-[13.5px] font-medium text-fg flex items-center gap-2">
          <Icon name="plug" class="h-4 w-4 text-accent" />
          Configured Providers & Accounts
        </h3>
        <p class="text-[11.5px] text-faint">
          Active credentials stored in <code class="text-dim">agent.db</code> and custom endpoints in <code class="text-dim">models.yml</code>
        </p>
      </div>

      <div class="flex items-center gap-2">
        <button
          type="button"
          class="flex items-center gap-1.5 rounded-[6px] border border-line px-2.5 py-1 text-[12px] text-dim hover:border-line-strong hover:text-fg transition-colors"
          title="Open interactive login in terminal"
          @click="emit('openTerminalLogin')"
        >
          <Icon name="terminal" class="h-3.5 w-3.5 text-faint" />
          Terminal Login
        </button>

        <button
          type="button"
          class="flex items-center gap-1.5 rounded-[6px] bg-accent px-3 py-1 text-[12px] font-medium text-canvas hover:opacity-90 transition-opacity"
          @click="emit('add')"
        >
          <Icon name="plus" class="h-3.5 w-3.5" />
          Add Provider
        </button>

        <button
          type="button"
          class="rounded-[6px] p-1.5 text-faint hover:bg-raised hover:text-fg transition-colors"
          title="Refresh accounts"
          @click="emit('refresh')"
        >
          <Icon name="refresh" class="h-3.5 w-3.5" :class="loading ? 'animate-spin' : ''" />
        </button>
      </div>
    </div>

    <!-- Provider Account List -->
    <div class="pt-3">
      <div v-if="loading && props.providers.length === 0" class="py-4 text-center text-[12px] text-faint">
        Loading configured accounts...
      </div>

      <div v-else-if="props.providers.length === 0" class="py-6 text-center">
        <p class="text-[12.5px] text-dim mb-2">No providers configured in agent storage yet.</p>
        <p class="text-[11.5px] text-faint mb-3">Add an API key, OAuth account, or custom model endpoint to get started.</p>
        <button
          type="button"
          class="rounded-[6px] bg-raised border border-line px-3 py-1.5 text-[12px] text-fg hover:border-line-strong transition-colors"
          @click="emit('add')"
        >
          + Add Your First Provider
        </button>
      </div>

      <div v-else class="flex flex-col gap-2">
        <div
          v-for="item in props.providers"
          :key="item.isCustom ? `custom-${item.provider}` : `cred-${item.id}`"
          class="flex items-center justify-between rounded-[7px] border border-line/60 bg-raised/40 px-3 py-2 transition-colors hover:border-line"
        >
          <div class="flex flex-col gap-0.5 min-w-0">
            <div class="flex items-center gap-2">
              <span class="font-medium text-[12.5px] text-fg">{{ item.provider }}</span>
              <span
                v-if="item.isCustom"
                class="rounded-[4px] bg-accent/15 px-1.5 py-0.5 text-[10px] font-medium text-accent"
              >
                Custom Endpoint
              </span>
              <span
                v-else-if="item.credentialType === 'oauth'"
                class="rounded-[4px] bg-ok/15 px-1.5 py-0.5 text-[10px] font-medium text-ok"
              >
                OAuth Account
              </span>
              <span
                v-else
                class="rounded-[4px] bg-line px-1.5 py-0.5 text-[10px] text-faint"
              >
                API Key
              </span>
            </div>

            <div class="flex items-center gap-3 text-[11px] text-faint">
              <span v-if="item.baseUrl" class="truncate font-mono">{{ item.baseUrl }}</span>
              <span v-if="item.models.length > 0" class="truncate">
                Models: {{ item.models.join(', ') }}
              </span>
              <span v-if="item.createdAt">Added {{ formatDate(item.createdAt) }}</span>
            </div>
          </div>

          <div class="flex items-center gap-2 shrink-0">
            <button
              type="button"
              class="rounded-[5px] p-1.5 text-faint hover:bg-err/15 hover:text-err transition-colors"
              :title="item.isCustom ? 'Remove custom provider' : 'Remove credential'"
              @click="handleRemove(item)"
            >
              <Icon name="close" class="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
