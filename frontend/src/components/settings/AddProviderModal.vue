<script setup lang="ts">
import { computed, ref, watch } from "vue";
import Modal from "../Modal.vue";
import Icon from "../ui/Icon.vue";
import {
  addCustomProvider,
  addProviderApiKey,
  type CatalogProviderDto,
} from "../../bridge";

const props = defineProps<{
  catalog: CatalogProviderDto[];
}>();

const emit = defineEmits<{
  (e: "close"): void;
  (e: "added", providerId: string): void;
  (e: "openTerminalLogin", providerId?: string): void;
}>();

const tab = ref<"catalog" | "custom">("catalog");

// Catalog Tab State
const searchFilter = ref("");
const selectedId = ref<string>("");
const apiKey = ref("");
const showKey = ref(false);
const busy = ref(false);
const errorMessage = ref<string | null>(null);

// Custom Tab State
const customId = ref("");
const customBaseUrl = ref("");
const customApiKey = ref("");
const customApiFormat = ref("openai-completions");
const customModelId = ref("");
const customModelName = ref("");

// Set initial selected catalog provider
watch(
  () => props.catalog,
  (list) => {
    if (list.length > 0 && !selectedId.value) {
      selectedId.value = list[0].id;
    }
  },
  { immediate: true }
);

const filteredCatalog = computed(() => {
  const q = searchFilter.value.trim().toLowerCase();
  if (!q) return props.catalog;
  return props.catalog.filter(
    (p) =>
      p.name.toLowerCase().includes(q) ||
      p.id.toLowerCase().includes(q) ||
      (p.description && p.description.toLowerCase().includes(q))
  );
});

const selectedProvider = computed(() => {
  return props.catalog.find((p) => p.id === selectedId.value) ?? null;
});

const isOAuth = computed(() => {
  return selectedProvider.value?.authType === "oauth";
});

async function saveCatalogKey(): Promise<void> {
  if (!selectedId.value) return;
  const key = apiKey.value.trim();
  if (!key) {
    errorMessage.value = "Please enter a valid API key.";
    return;
  }

  busy.value = true;
  errorMessage.value = null;

  try {
    await addProviderApiKey(selectedId.value, key);
    emit("added", selectedId.value);
    emit("close");
  } catch (err) {
    errorMessage.value = String(err);
  } finally {
    busy.value = false;
  }
}

async function saveCustom(): Promise<void> {
  const id = customId.value.trim();
  const url = customBaseUrl.value.trim();
  const mid = customModelId.value.trim();

  if (!id) {
    errorMessage.value = "Provider ID is required (e.g. local-vllm).";
    return;
  }
  if (!url) {
    errorMessage.value = "Base URL is required (e.g. http://127.0.0.1:8000/v1).";
    return;
  }
  if (!mid) {
    errorMessage.value = "At least one Model ID is required (e.g. llama3).";
    return;
  }

  busy.value = true;
  errorMessage.value = null;

  try {
    await addCustomProvider({
      id,
      baseUrl: url,
      apiKey: customApiKey.value.trim() || null,
      api: customApiFormat.value,
      modelId: mid,
      modelName: customModelName.value.trim() || null,
    });
    emit("added", id);
    emit("close");
  } catch (err) {
    errorMessage.value = String(err);
  } finally {
    busy.value = false;
  }
}

function handleOAuthLogin(): void {
  emit("openTerminalLogin", selectedId.value);
  emit("close");
}
</script>

<template>
  <Modal title="Add / Authenticate Provider" :busy="busy" @close="emit('close')">
    <div class="flex flex-col gap-4 text-[12.5px]">
      <!-- Tab Header -->
      <div class="flex border-b border-line pb-1">
        <button
          type="button"
          class="rounded-[6px] px-3 py-1.5 font-medium transition-colors"
          :class="tab === 'catalog' ? 'bg-raised text-fg' : 'text-dim hover:text-fg'"
          @click="tab = 'catalog'; errorMessage = null"
        >
          Catalog Provider
        </button>
        <button
          type="button"
          class="rounded-[6px] px-3 py-1.5 font-medium transition-colors"
          :class="tab === 'custom' ? 'bg-raised text-fg' : 'text-dim hover:text-fg'"
          @click="tab = 'custom'; errorMessage = null"
        >
          Custom Endpoint (models.yml)
        </button>
      </div>

      <!-- Error banner if any -->
      <div
        v-if="errorMessage"
        class="rounded-[6px] border border-err/40 bg-err/15 px-3 py-2 text-[12px] text-err"
      >
        {{ errorMessage }}
      </div>

      <!-- TAB 1: CATALOG PROVIDER -->
      <div v-if="tab === 'catalog'" class="flex flex-col gap-3">
        <!-- Search filter -->
        <div>
          <label class="mb-1 block text-[11px] font-medium uppercase tracking-wider text-faint">
            Select Provider
          </label>
          <div class="flex items-center gap-2 rounded-[6px] bg-raised px-2.5 py-1.5">
            <Icon name="search" class="h-3.5 w-3.5 text-faint shrink-0" />
            <input
              v-model="searchFilter"
              type="text"
              placeholder="Search providers (Anthropic, OpenRouter, DeepSeek...)"
              class="w-full bg-transparent text-[12.5px] text-fg outline-none placeholder:text-faint"
            />
          </div>
        </div>

        <!-- Provider list -->
        <div class="max-h-40 overflow-y-auto rounded-[6px] border border-line bg-canvas/60 p-1">
          <button
            v-for="p in filteredCatalog"
            :key="p.id"
            type="button"
            class="flex w-full items-center justify-between rounded-[5px] px-2.5 py-1.5 text-left transition-colors"
            :class="selectedId === p.id ? 'bg-selected text-fg font-medium' : 'text-dim hover:bg-raised hover:text-fg'"
            @click="selectedId = p.id; errorMessage = null"
          >
            <div class="flex items-center gap-2 truncate">
              <span class="truncate">{{ p.name }}</span>
              <span class="text-[10.5px] text-faint">({{ p.id }})</span>
            </div>
            <span
              class="shrink-0 rounded-[4px] px-1.5 py-0.5 text-[10px] uppercase tracking-wider"
              :class="p.authType === 'oauth' ? 'bg-accent/15 text-accent' : 'bg-line/60 text-faint'"
            >
              {{ p.authType === 'oauth' ? 'OAuth' : 'API Key' }}
            </span>
          </button>
        </div>

        <!-- Selected Provider Details & Input -->
        <div v-if="selectedProvider" class="mt-1 flex flex-col gap-2 rounded-[6px] bg-raised/50 p-3 border border-line/50">
          <div class="text-[12px] text-dim">
            {{ selectedProvider.description }}
          </div>

          <!-- OAuth Provider instructions -->
          <div v-if="isOAuth" class="flex flex-col gap-2.5 pt-1">
            <p class="text-[11.5px] text-faint leading-relaxed">
              {{ selectedProvider.name }} uses OAuth authorization. Clicking below will open the app's terminal to complete the authorization flow in your browser.
            </p>
            <button
              type="button"
              class="flex items-center justify-center gap-2 rounded-[6px] bg-accent px-3 py-2 font-medium text-canvas hover:opacity-90 transition-opacity"
              @click="handleOAuthLogin"
            >
              <Icon name="external" class="h-3.5 w-3.5" />
              Authorize {{ selectedProvider.name }}
            </button>
          </div>

          <!-- API Key Input -->
          <div v-else class="flex flex-col gap-2 pt-1">
            <label class="text-[11px] font-medium uppercase tracking-wider text-faint">
              API Key
            </label>
            <div class="flex items-center gap-2 rounded-[6px] bg-raised px-3 py-1.5 border border-line focus-within:border-accent">
              <input
                v-model="apiKey"
                :type="showKey ? 'text' : 'password'"
                spellcheck="false"
                autocomplete="off"
                placeholder="Paste API Key (sk-...)"
                class="min-w-0 flex-1 bg-transparent font-mono text-[12px] text-fg outline-none placeholder:text-faint"
                @keydown.enter.prevent="saveCatalogKey"
              />
              <button
                type="button"
                class="text-faint hover:text-fg text-[11px]"
                @click="showKey = !showKey"
              >
                {{ showKey ? 'Hide' : 'Show' }}
              </button>
            </div>
            <p v-if="selectedProvider.envVar" class="text-[11px] text-faint">
              Alternatively, you can set <code class="text-accent">{{ selectedProvider.envVar }}</code> in your environment.
            </p>

            <div class="flex justify-end gap-2 pt-2">
              <button
                type="button"
                class="rounded-[6px] px-3 py-1.5 text-dim hover:bg-raised hover:text-fg"
                @click="emit('close')"
              >
                Cancel
              </button>
              <button
                type="button"
                class="rounded-[6px] bg-accent px-4 py-1.5 font-medium text-canvas hover:opacity-90 disabled:opacity-40"
                :disabled="!apiKey.trim() || busy"
                @click="saveCatalogKey"
              >
                {{ busy ? "Saving..." : "Save Key" }}
              </button>
            </div>
          </div>
        </div>
      </div>

      <!-- TAB 2: CUSTOM PROVIDER (models.yml) -->
      <div v-if="tab === 'custom'" class="flex flex-col gap-3">
        <p class="text-[11.5px] text-faint leading-relaxed">
          Configure an OpenAI-compatible, Anthropic-compatible, or local engine (Ollama, LM Studio, vLLM). This is persisted into <code class="text-accent">~/.omp/agent/models.yml</code>.
        </p>

        <!-- Provider ID -->
        <div>
          <label class="mb-1 block text-[11px] font-medium uppercase tracking-wider text-faint">
            Provider ID *
          </label>
          <input
            v-model="customId"
            type="text"
            placeholder="e.g. local-vllm, lm-studio, my-proxy"
            class="w-full rounded-[6px] bg-raised px-3 py-1.5 text-[12px] text-fg outline-none border border-line focus:border-accent placeholder:text-faint"
          />
        </div>

        <!-- Base URL -->
        <div>
          <label class="mb-1 block text-[11px] font-medium uppercase tracking-wider text-faint">
            Base URL *
          </label>
          <input
            v-model="customBaseUrl"
            type="text"
            placeholder="e.g. http://127.0.0.1:8000/v1 or https://my-proxy.com/v1"
            class="w-full rounded-[6px] bg-raised px-3 py-1.5 text-[12px] text-fg outline-none border border-line focus:border-accent placeholder:text-faint font-mono"
          />
        </div>

        <!-- Wire Protocol -->
        <div>
          <label class="mb-1 block text-[11px] font-medium uppercase tracking-wider text-faint">
            Protocol / API Format
          </label>
          <select
            v-model="customApiFormat"
            class="w-full rounded-[6px] bg-raised px-3 py-1.5 text-[12px] text-fg outline-none border border-line focus:border-accent"
          >
            <option value="openai-completions">openai-completions (OpenAI compatible)</option>
            <option value="anthropic-messages">anthropic-messages (Anthropic compatible)</option>
            <option value="openai-responses">openai-responses (OpenAI structured)</option>
          </select>
        </div>

        <!-- API Key (optional) -->
        <div>
          <label class="mb-1 block text-[11px] font-medium uppercase tracking-wider text-faint">
            API Key (optional)
          </label>
          <input
            v-model="customApiKey"
            type="password"
            placeholder="Optional bearer token or key"
            class="w-full rounded-[6px] bg-raised px-3 py-1.5 text-[12px] text-fg outline-none border border-line focus:border-accent placeholder:text-faint font-mono"
          />
        </div>

        <!-- Model ID & Name -->
        <div class="grid grid-cols-2 gap-2">
          <div>
            <label class="mb-1 block text-[11px] font-medium uppercase tracking-wider text-faint">
              Model ID *
            </label>
            <input
              v-model="customModelId"
              type="text"
              placeholder="e.g. llama-3.3-70b"
              class="w-full rounded-[6px] bg-raised px-3 py-1.5 text-[12px] text-fg outline-none border border-line focus:border-accent placeholder:text-faint font-mono"
            />
          </div>
          <div>
            <label class="mb-1 block text-[11px] font-medium uppercase tracking-wider text-faint">
              Display Name (optional)
            </label>
            <input
              v-model="customModelName"
              type="text"
              placeholder="e.g. Llama 3.3 70B Local"
              class="w-full rounded-[6px] bg-raised px-3 py-1.5 text-[12px] text-fg outline-none border border-line focus:border-accent placeholder:text-faint"
            />
          </div>
        </div>

        <!-- Actions -->
        <div class="flex justify-end gap-2 pt-2">
          <button
            type="button"
            class="rounded-[6px] px-3 py-1.5 text-dim hover:bg-raised hover:text-fg"
            @click="emit('close')"
          >
            Cancel
          </button>
          <button
            type="button"
            class="rounded-[6px] bg-accent px-4 py-1.5 font-medium text-canvas hover:opacity-90 disabled:opacity-40"
            :disabled="!customId.trim() || !customBaseUrl.trim() || !customModelId.trim() || busy"
            @click="saveCustom"
          >
            {{ busy ? "Saving..." : "Add Custom Provider" }}
          </button>
        </div>
      </div>
    </div>
  </Modal>
</template>
