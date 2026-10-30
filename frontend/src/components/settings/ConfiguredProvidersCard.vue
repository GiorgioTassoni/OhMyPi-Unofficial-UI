<script setup lang="ts">
import { ref } from "vue";
import Icon from "../ui/Icon.vue";
import {
  testProviderConnection,
  type ProviderAccountDto,
  type ProviderTestResultDto,
} from "../../bridge";

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

const testingProvider = ref<string | null>(null);
const testingAll = ref(false);
const testResults = ref<Record<string, ProviderTestResultDto>>({});

function providerDisplayName(id: string): string {
  const map: Record<string, string> = {
    "openai-codex": "OpenAI Codex (ChatGPT)",
    openai: "OpenAI API",
    anthropic: "Anthropic Claude",
    google: "Google Gemini",
    "google-antigravity": "Google Antigravity",
    openrouter: "OpenRouter",
    commandcode: "Command Code",
    deepseek: "DeepSeek",
    groq: "Groq",
    xai: "xAI (Grok)",
    cerebras: "Cerebras",
    mistral: "Mistral AI",
    "github-copilot": "GitHub Copilot",
    perplexity: "Perplexity",
    zai: "z.ai (Zhipu GLM)",
    minimax: "MiniMax",
    "llama.cpp": "llama.cpp (Local)",
    ollama: "Ollama (Local)",
    "lm-studio": "LM Studio (Local)",
    vllm: "vLLM (Local)",
  };
  return map[id.toLowerCase()] ?? id;
}

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

async function handleTest(provider: string): Promise<void> {
  if (testingProvider.value) return;
  testingProvider.value = provider;
  try {
    const res = await testProviderConnection(provider);
    testResults.value[provider] = res;
    if (res.sampleModels.length > 0) {
      const row = props.providers.find((p) => p.provider === provider);
      if (row && row.models.length === 0) {
        row.models = [...res.sampleModels];
      }
    }
  } catch (err) {
    testResults.value[provider] = {
      ok: false,
      latencyMs: 0,
      modelCount: 0,
      sampleModels: [],
      message: String(err),
    };
  } finally {
    testingProvider.value = null;
  }
}

async function handleTestAll(): Promise<void> {
  if (testingAll.value || props.providers.length === 0) return;
  testingAll.value = true;
  try {
    for (const item of props.providers) {
      await handleTest(item.provider);
    }
  } finally {
    testingAll.value = false;
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
          v-if="props.providers.length > 0"
          type="button"
          class="flex items-center gap-1.5 rounded-[6px] border border-line px-2.5 py-1 text-[12px] text-dim hover:border-line-strong hover:text-fg transition-all active:scale-95 disabled:opacity-50"
          :disabled="testingAll || loading"
          @click="handleTestAll"
        >
          <Icon
            :name="testingAll ? 'spinner' : 'activity'"
            class="h-3.5 w-3.5 text-faint"
            :class="testingAll ? 'animate-spin text-accent' : ''"
          />
          {{ testingAll ? 'Testing All...' : 'Test All' }}
        </button>

        <button
          type="button"
          class="flex items-center gap-1.5 rounded-[6px] border border-line px-2.5 py-1 text-[12px] text-dim hover:border-line-strong hover:text-fg transition-all active:scale-95"
          @click="emit('openTerminalLogin')"
        >
          <Icon name="terminal" class="h-3.5 w-3.5 text-faint" />
          Terminal Login
        </button>

        <button
          type="button"
          class="flex items-center gap-1.5 rounded-[6px] bg-accent px-3 py-1 text-[12px] font-semibold text-canvas transition-all hover:brightness-110 active:scale-95 shadow-xs"
          @click="emit('add')"
        >
          <Icon name="plus" class="h-3.5 w-3.5" />
          Add Provider
        </button>

        <button
          type="button"
          class="rounded-[6px] p-1.5 text-faint hover:bg-raised hover:text-fg transition-all active:scale-95"
          aria-label="Refresh accounts"
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
          class="rounded-[6px] bg-raised border border-line px-3.5 py-1.5 text-[12px] font-medium text-fg hover:border-line-strong hover:bg-raised/80 transition-all active:scale-95 shadow-xs"
          @click="emit('add')"
        >
          + Add Your First Provider
        </button>
      </div>

      <div v-else class="flex flex-col gap-2.5">
        <div
          v-for="item in props.providers"
          :key="item.isCustom ? `custom-${item.provider}` : `cred-${item.id}`"
          class="flex flex-col gap-2 rounded-[8px] border border-line/60 bg-raised/40 p-3 transition-colors hover:border-line"
        >
          <div class="flex items-center justify-between gap-3">
            <div class="flex flex-wrap items-center gap-2 min-w-0">
              <span class="font-medium text-[13px] text-fg">
                {{ providerDisplayName(item.provider) }}
              </span>
              <span
                v-if="providerDisplayName(item.provider) !== item.provider"
                class="rounded-[4px] bg-canvas/80 px-1.5 py-0.2 text-[10px] font-mono text-faint border border-line/40"
              >
                {{ item.provider }}
              </span>
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

            <div class="flex items-center gap-1.5 shrink-0">
              <button
                type="button"
                class="flex items-center gap-1.5 rounded-[5px] border border-line/70 bg-surface/80 px-2 py-1 text-[11px] font-medium text-dim hover:border-line-strong hover:text-fg hover:bg-raised transition-all active:scale-95 disabled:opacity-50"
                :disabled="testingProvider === item.provider || testingAll"
                @click="handleTest(item.provider)"
              >
                <Icon
                  :name="testingProvider === item.provider ? 'spinner' : 'activity'"
                  class="h-3 w-3"
                  :class="testingProvider === item.provider ? 'animate-spin text-accent' : 'text-faint'"
                />
                <span>{{ testingProvider === item.provider ? 'Testing...' : 'Test' }}</span>
              </button>

              <button
                type="button"
                class="rounded-[5px] p-1.5 text-faint hover:bg-err/15 hover:text-err transition-colors"
                :aria-label="item.isCustom ? 'Remove custom provider' : 'Remove credential'"
                @click="handleRemove(item)"
              >
                <Icon name="close" class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>

          <!-- Models & Details -->
          <div class="flex flex-col gap-1 text-[11px]">
            <div v-if="item.models.length > 0" class="flex flex-wrap items-center gap-1">
              <span class="text-faint text-[10.5px]">Available models:</span>
              <span
                v-for="m in item.models.slice(0, 4)"
                :key="m"
                class="rounded-[3px] bg-canvas/80 border border-line/50 px-1.5 py-0.2 text-[10px] font-mono text-dim"
              >
                {{ m }}
              </span>
              <span
                v-if="item.models.length > 4"
                class="text-[10px] text-faint font-medium"
              >
                +{{ item.models.length - 4 }} more
              </span>
            </div>
            <div v-else class="text-[10.5px] text-faint italic">
              No models cached yet. Click “Test” to discover available models.
            </div>

            <div class="flex items-center gap-3 text-faint text-[10.5px] mt-0.5">
              <span v-if="item.baseUrl" class="truncate font-mono">{{ item.baseUrl }}</span>
              <span v-if="item.createdAt">Added {{ formatDate(item.createdAt) }}</span>
            </div>
          </div>

          <!-- Test Result Banner -->
          <div
            v-if="testResults[item.provider]"
            class="flex items-center gap-2 rounded-[5px] px-2 py-1 text-[11px] transition-all"
            :class="testResults[item.provider]?.ok ? 'bg-ok/10 text-ok border border-ok/25' : 'bg-err/10 text-err border border-err/25'"
          >
            <span class="font-medium shrink-0">
              {{ testResults[item.provider]?.ok ? '✓ Connected' : '✕ Connection Failed' }}
            </span>
            <span
              v-if="testResults[item.provider]?.ok && testResults[item.provider]?.latencyMs"
              class="font-mono text-[10px] text-faint shrink-0"
            >
              {{ testResults[item.provider]?.latencyMs }}ms
            </span>
            <span class="truncate opacity-90">
              {{ testResults[item.provider]?.message }}
            </span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>