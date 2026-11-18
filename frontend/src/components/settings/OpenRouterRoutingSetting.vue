<script setup lang="ts">
// Read saved rules locally; discover endpoints only for the model being edited.
import { computed, inject, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  getOpenRouterProviders,
  getOpenRouterRouting,
  listOpenRouterRoutings,
  refreshModels,
  setOpenRouterRouting,
  type OpenRouterProvider,
  type OpenRouterRouting,
} from "../../bridge";
import ModelPicker from "../ModelPicker.vue";
import Popover from "../Popover.vue";
import Icon from "../ui/Icon.vue";
import { SETTINGS_CONTEXT } from "./context";

const context = inject(SETTINGS_CONTEXT);
if (context === undefined) {
  throw new Error("OpenRouterRoutingSetting is only usable inside the settings screen");
}

interface ProviderRow {
  slug: string;
  name: string;
  savedOnly: boolean;
}

interface RemoveError {
  modelId: string;
  message: string;
}

const models = computed(() => context.models.value.filter((model) => model.provider === "openrouter"));
const modelsById = computed(() => new Map(models.value.map((model) => [model.id, model])));

const rules = ref<OpenRouterRouting[]>([]);
const loadingRules = ref(false);
const rulesError = ref<string | null>(null);

const editingModel = ref<string | null>(null);
const restriction = ref<string[] | null>(null);
const automatic = ref(true);
const draftProviders = ref<string[]>([]);
const providers = ref<OpenRouterProvider[]>([]);
const routingLoaded = ref(false);
const loadingRouting = ref(false);
const loadingProviders = ref(false);
const saving = ref(false);
const mutatingModel = ref<string | null>(null);
const routingError = ref<string | null>(null);
const providersError = ref<string | null>(null);
const saveError = ref<string | null>(null);
const removeError = ref<RemoveError | null>(null);
const notice = ref<string | null>(null);

const pickingModel = ref(false);
const providerPickerOpen = ref(false);
const providerQuery = ref("");
const catalogError = ref<string | null>(null);
const refreshingCatalog = ref(false);

let rulesRequest = 0;
let routingRequest = 0;
let providersRequest = 0;

const editingModelName = computed(() =>
  editingModel.value === null ? "" : modelName(editingModel.value),
);
const selectedProviders = computed(() => [...new Set(draftProviders.value)]);
const selectedProviderSet = computed(() => new Set(selectedProviders.value));

const choiceLabel = computed(() => {
  if (!routingLoaded.value) return "Loading saved restriction…";
  if (automatic.value) return "Automatic routing";
  const count = selectedProviders.value.length;
  if (count === 0) return "No providers selected";
  return `${count} provider${count === 1 ? "" : "s"} selected`;
});

// Keep undiscovered saved slugs editable when discovery is offline or the catalog changes.
const providerRows = computed<ProviderRow[]>(() => {
  const discovered: ProviderRow[] = providers.value.map((provider) => ({
    slug: provider.slug,
    name: provider.name,
    savedOnly: false,
  }));
  const known = new Set(discovered.map((row) => row.slug));
  const savedOnly: ProviderRow[] = [];
  for (const slug of selectedProviders.value) {
    if (known.has(slug)) continue;
    known.add(slug);
    savedOnly.push({ slug, name: slug, savedOnly: true });
  }
  return [...savedOnly, ...discovered];
});

const hasSavedOnlyProviders = computed(() => providerRows.value.some((row) => row.savedOnly));

const filteredProviders = computed(() => {
  const tokens = providerQuery.value.trim().toLowerCase().split(/\s+/).filter((token) => token !== "");
  if (tokens.length === 0) return providerRows.value;
  return providerRows.value.filter((row) => {
    const haystack = `${row.name} ${row.slug}`.toLowerCase();
    return tokens.every((token) => haystack.includes(token));
  });
});

const canSave = computed(() => {
  if (editingModel.value === null || !routingLoaded.value) return false;
  if (loadingRules.value || mutatingModel.value !== null) return false;
  if (!automatic.value && selectedProviders.value.length === 0) return false;
  return !sameChoice(normalizedDraft(), restriction.value);
});

function describeError(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

function modelName(modelId: string): string {
  return modelsById.value.get(modelId)?.name ?? modelId;
}

function modelInCatalog(modelId: string): boolean {
  return modelsById.value.has(modelId);
}

function removeErrorText(modelId: string): string | null {
  const error = removeError.value;
  return error !== null && error.modelId === modelId ? error.message : null;
}

function normalizedDraft(): string[] | null {
  return automatic.value ? null : [...selectedProviders.value];
}

function sameChoice(left: string[] | null, right: string[] | null): boolean {
  if (left === null || right === null) return left === right;
  return left.length === right.length && left.every((slug, at) => slug === right[at]);
}

function upsertRule(modelId: string, providers: string[] | null): void {
  const at = rules.value.findIndex((rule) => rule.modelId === modelId);
  if (providers === null) {
    if (at >= 0) rules.value = rules.value.filter((rule) => rule.modelId !== modelId);
    return;
  }
  const next = [...rules.value];
  if (at >= 0) {
    next[at] = { modelId, providers: [...providers] };
  } else {
    next.push({ modelId, providers: [...providers] });
  }
  rules.value = next;
}

function loadRules(): void {
  const request = ++rulesRequest;
  loadingRules.value = true;
  rulesError.value = null;
  void listOpenRouterRoutings()
    .then((value) => {
      if (request === rulesRequest) rules.value = value;
    })
    .catch((cause: unknown) => {
      if (request === rulesRequest) rulesError.value = describeError(cause);
    })
    .finally(() => {
      if (request === rulesRequest) loadingRules.value = false;
    });
}

function fetchRouting(modelId: string): void {
  const request = ++routingRequest;
  void getOpenRouterRouting(modelId)
    .then((value) => {
      if (request !== routingRequest || editingModel.value !== modelId) return;
      restriction.value = value;
      automatic.value = value === null;
      draftProviders.value = value === null ? [] : [...value];
      routingLoaded.value = true;
    })
    .catch((cause: unknown) => {
      if (request === routingRequest && editingModel.value === modelId) {
        routingError.value = describeError(cause);
      }
    })
    .finally(() => {
      if (request === routingRequest) loadingRouting.value = false;
    });
}

function fetchProviders(modelId: string): void {
  const request = ++providersRequest;
  void getOpenRouterProviders(modelId)
    .then((value) => {
      if (request !== providersRequest || editingModel.value !== modelId) return;
      providers.value = value;
    })
    .catch((cause: unknown) => {
      if (request === providersRequest && editingModel.value === modelId) {
        providersError.value = describeError(cause);
      }
    })
    .finally(() => {
      if (request === providersRequest) loadingProviders.value = false;
    });
}

function openEditor(modelId: string): void {
  if (mutatingModel.value !== null) return;
  pickingModel.value = false;
  providerPickerOpen.value = false;
  providerQuery.value = "";
  editingModel.value = modelId;
  restriction.value = null;
  automatic.value = true;
  draftProviders.value = [];
  providers.value = [];
  routingLoaded.value = false;
  loadingRouting.value = true;
  loadingProviders.value = true;
  routingError.value = null;
  providersError.value = null;
  saveError.value = null;
  notice.value = null;
  fetchRouting(modelId);
  fetchProviders(modelId);
}

function closeEditor(): void {
  if (mutatingModel.value !== null) return;
  routingRequest++;
  providersRequest++;
  editingModel.value = null;
  providerPickerOpen.value = false;
  providerQuery.value = "";
  saveError.value = null;
}

function chooseModel(modelId: unknown): void {
  if (typeof modelId !== "string" || mutatingModel.value !== null) return;
  pickingModel.value = false;
  if (modelId === editingModel.value) return;
  openEditor(modelId);
}

function toggleProviderPicker(): void {
  if (!routingLoaded.value || mutatingModel.value !== null) return;
  providerPickerOpen.value = !providerPickerOpen.value;
  if (providerPickerOpen.value) providerQuery.value = "";
}

function touched(): void {
  saveError.value = null;
  notice.value = null;
}

function toggleProvider(slug: string): void {
  if (mutatingModel.value !== null) return;
  automatic.value = false;
  draftProviders.value = selectedProviderSet.value.has(slug)
    ? draftProviders.value.filter((selected) => selected !== slug)
    : [...draftProviders.value, slug];
  touched();
}

function chooseAutomatic(): void {
  if (mutatingModel.value !== null) return;
  automatic.value = true;
  draftProviders.value = [];
  touched();
}

function retryCatalog(): void {
  refreshingCatalog.value = true;
  catalogError.value = null;
  void refreshModels()
    .catch((cause: unknown) => {
      catalogError.value = describeError(cause);
    })
    .finally(() => {
      refreshingCatalog.value = false;
    });
}

function retryEditor(): void {
  const modelId = editingModel.value;
  if (modelId === null || mutatingModel.value !== null) return;
  if (routingError.value === null && providersError.value === null) return;
  saveError.value = null;
  if (routingError.value !== null) {
    routingError.value = null;
    restriction.value = null;
    automatic.value = true;
    draftProviders.value = [];
    routingLoaded.value = false;
    loadingRouting.value = true;
    fetchRouting(modelId);
  }
  if (providersError.value !== null) {
    providersError.value = null;
    loadingProviders.value = true;
    fetchProviders(modelId);
  }
}

async function save(): Promise<void> {
  const modelId = editingModel.value;
  if (modelId === null || !canSave.value) return;
  const desired = normalizedDraft();
  if (desired !== null && desired.length === 0) return;
  saving.value = true;
  mutatingModel.value = modelId;
  saveError.value = null;
  notice.value = null;
  try {
    await setOpenRouterRouting(modelId, desired);
    if (editingModel.value !== modelId) return;
    restriction.value = desired;
    automatic.value = desired === null;
    draftProviders.value = desired === null ? [] : [...desired];
    upsertRule(modelId, desired);
    if (removeError.value?.modelId === modelId) removeError.value = null;
    notice.value = "Saved. Restart existing sessions for this routing change to take effect.";
  } catch (cause) {
    if (editingModel.value === modelId) saveError.value = describeError(cause);
  } finally {
    if (mutatingModel.value === modelId) mutatingModel.value = null;
    saving.value = false;
  }
}

async function removeRule(rule: OpenRouterRouting): Promise<void> {
  if (loadingRules.value || mutatingModel.value !== null) return;
  const modelId = rule.modelId;
  mutatingModel.value = modelId;
  removeError.value = null;
  notice.value = null;
  try {
    await setOpenRouterRouting(modelId, null);
    upsertRule(modelId, null);
    notice.value = `Removed the provider restriction for ${modelName(modelId)}.`;
    if (editingModel.value === modelId) {
      // A read started before removal must not restore the old restriction.
      routingRequest++;
      loadingRouting.value = false;
      routingError.value = null;
      routingLoaded.value = true;
      restriction.value = null;
      automatic.value = true;
      draftProviders.value = [];
      saveError.value = null;
    }
  } catch (cause) {
    removeError.value = { modelId, message: describeError(cause) };
  } finally {
    mutatingModel.value = null;
  }
}

watch(models, () => {
  catalogError.value = null;
});

onMounted(loadRules);

onBeforeUnmount(() => {
  rulesRequest++;
  routingRequest++;
  providersRequest++;
});
</script>

<template>
  <section class="rounded-[10px] border border-line bg-raised/35 p-4" data-setting="openrouter-routing">
    <div class="flex flex-wrap items-start justify-between gap-3 border-b border-line/60 pb-3">
      <div class="min-w-0">
        <h3 class="text-[13.5px] font-medium text-fg">OpenRouter execution providers</h3>
        <p class="mt-1 max-w-[70ch] text-[12px] leading-relaxed text-faint">
          Restrict each OpenRouter model to particular endpoint providers. Automatic removes
          only that model’s restriction; existing provider defaults remain in effect, and
          provider-wide or custom-model settings may still affect routing.
        </p>
      </div>

      <div class="flex shrink-0 flex-wrap items-center gap-2">
        <Popover
          v-if="models.length > 0"
          :open="pickingModel"
          label="Add OpenRouter model"
          side="below"
          :width="480"
          @close="pickingModel = false"
        >
          <template #trigger>
            <button
              type="button"
              :disabled="mutatingModel !== null"
              :aria-expanded="pickingModel"
              aria-haspopup="dialog"
              class="flex items-center gap-1.5 rounded-[6px] border border-line px-2.5 py-1.5 text-[12px] text-dim transition-colors hover:border-line-strong hover:text-fg focus-visible:border-accent/80 focus-visible:ring-1 focus-visible:ring-accent/40 disabled:opacity-40"
              data-action="add-openrouter-routing"
              @click="pickingModel = !pickingModel"
            >
              <Icon name="plus" class="h-3.5 w-3.5 text-faint" />
              Add model
            </button>
          </template>
          <ModelPicker
            :models="models"
            :favourites="context.favourites.value"
            :current="null"
            :refreshing="context.refreshing.value"
            @select="(picked) => chooseModel(picked.modelId)"
            @favourites="context.setFavourites"
            @close="pickingModel = false"
          />
        </Popover>
        <span
          v-else-if="context.refreshing.value"
          class="text-[11.5px] text-faint"
          role="status"
        >
          Loading model catalog…
        </span>
        <div v-else class="flex flex-wrap items-center gap-2 text-[11.5px]">
          <span class="text-faint">No OpenRouter models are available in the current model catalog.</span>
          <button
            type="button"
            class="rounded-[6px] border border-line px-2.5 py-1 text-dim transition-colors hover:border-line-strong hover:bg-raised hover:text-fg disabled:opacity-50"
            :disabled="refreshingCatalog"
            @click="retryCatalog"
          >
            {{ refreshingCatalog ? "Refreshing…" : "Refresh model catalog" }}
          </button>
        </div>
      </div>
    </div>

    <p v-if="catalogError" class="mt-2 text-[11.5px] text-err" role="alert">
      Could not refresh the model catalog: {{ catalogError }}
    </p>

    <div class="mt-4">
      <div class="flex flex-wrap items-center gap-2 border-b border-line/50 pb-2">
        <h4 class="text-[12px] font-medium text-dim">
          Saved restrictions
        </h4>
        <span v-if="rules.length > 0" class="text-[11px] text-faint">
          {{ rules.length }} {{ rules.length === 1 ? "model" : "models" }}
        </span>
        <button
          v-if="rulesError"
          type="button"
          class="ml-auto rounded-[6px] border border-line px-2.5 py-1 text-[11.5px] text-dim transition-colors hover:border-line-strong hover:bg-raised hover:text-fg disabled:opacity-50"
          :disabled="loadingRules || mutatingModel !== null"
          data-action="retry-openrouter-routings"
          @click="loadRules"
        >
          {{ loadingRules ? "Retrying…" : "Retry" }}
        </button>
      </div>

      <p v-if="rulesError" class="mt-2 text-[11.5px] text-err" role="alert">
        Could not load saved restrictions: {{ rulesError }}
      </p>

      <p v-if="loadingRules && rules.length === 0" class="mt-2 text-[11.5px] text-faint" role="status">
        Loading saved restrictions…
      </p>
      <p v-else-if="rules.length === 0 && rulesError === null" class="mt-2 text-[11.5px] text-faint">
        No model-level provider restrictions are saved. Add a model to restrict its execution providers.
      </p>

      <ul v-if="rules.length > 0" class="mt-1 flex flex-col divide-y divide-line/30">
        <li
          v-for="rule in rules"
          :key="rule.modelId"
          class="flex flex-wrap items-start gap-x-4 gap-y-2 px-1 py-2.5"
          data-routing-rule=""
          :data-model-id="rule.modelId"
        >
          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
              <span class="min-w-0 truncate text-[13px] text-fg">{{ modelName(rule.modelId) }}</span>
              <span
                v-if="modelName(rule.modelId) !== rule.modelId"
                class="min-w-0 truncate font-mono text-[10.5px] text-faint"
              >
                {{ rule.modelId }}
              </span>
              <span
                v-if="!modelInCatalog(rule.modelId)"
                class="rounded-full border border-warn/40 bg-warn/10 px-1.5 py-0.5 text-[10px] text-warn"
              >
                not in the current catalog
              </span>
            </div>

            <div v-if="rule.providers.length > 0" class="mt-1 flex flex-wrap gap-1">
              <span
                v-for="(slug, at) in rule.providers"
                :key="at"
                class="rounded-[4px] border border-line/40 bg-raised px-1.5 py-0.5 font-mono text-[10.5px] text-dim"
              >
                {{ slug }}
              </span>
            </div>
            <p v-else class="mt-0.5 max-w-[70ch] text-[11.5px] leading-relaxed text-err">
              No providers allowed; requests for this model fail until this restriction is cleared.
            </p>

            <p
              v-if="removeErrorText(rule.modelId)"
              class="mt-1 text-[11px] leading-relaxed text-err"
              role="alert"
            >
              Could not remove this restriction: {{ removeErrorText(rule.modelId) }}
            </p>
          </div>

          <div class="flex shrink-0 items-center gap-1.5 pt-0.5">
            <button
              type="button"
              :disabled="mutatingModel !== null"
              :aria-label="`Edit the provider restriction for ${modelName(rule.modelId)}`"
              class="flex items-center gap-1.5 rounded-[6px] border border-line/60 bg-raised px-2.5 py-1 text-[11.5px] text-dim transition-colors hover:border-line-strong hover:text-fg focus-visible:border-accent/80 focus-visible:ring-1 focus-visible:ring-accent/40 disabled:opacity-40"
              data-action="edit-openrouter-routing"
              @click="openEditor(rule.modelId)"
            >
              <Icon name="pencil" class="h-3.5 w-3.5" />
              Edit
            </button>
            <button
              type="button"
              :disabled="loadingRules || mutatingModel !== null"
              :aria-label="`Remove the provider restriction for ${modelName(rule.modelId)}`"
              class="flex items-center gap-1.5 rounded-[6px] border border-line/60 px-2.5 py-1 text-[11.5px] text-dim transition-colors hover:border-line-strong hover:bg-raised hover:text-err focus-visible:border-accent/80 focus-visible:ring-1 focus-visible:ring-accent/40 disabled:opacity-40"
              data-action="remove-openrouter-routing"
              @click="removeRule(rule)"
            >
              <Icon name="trash" class="h-3.5 w-3.5" />
              Remove
            </button>
          </div>
        </li>
      </ul>

      <p v-if="notice" class="mt-2 text-[11.5px] text-ok" role="status">{{ notice }}</p>
    </div>

    <div
      v-if="editingModel !== null"
      class="mt-4 rounded-[8px] border border-line/60 bg-raised/30 p-3"
      data-routing-editor=""
      :data-model-id="editingModel"
    >
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div class="min-w-0">
          <h4 class="truncate text-[12.5px] font-medium text-fg">
            Provider restriction for {{ editingModelName }}
          </h4>
          <p
            v-if="editingModelName !== editingModel"
            class="mt-0.5 truncate font-mono text-[10.5px] text-faint"
          >
            {{ editingModel }}
          </p>
        </div>
        <button
          type="button"
          :disabled="mutatingModel !== null"
          class="rounded-[6px] border border-line px-2.5 py-1 text-[11.5px] text-dim transition-colors hover:border-line-strong hover:bg-raised hover:text-fg disabled:opacity-40"
          data-action="close-openrouter-editor"
          @click="closeEditor"
        >
          Close
        </button>
      </div>

      <div class="mt-3 flex flex-wrap items-center gap-3">
        <div class="flex items-center gap-3 text-[12.5px] text-dim">
          <span>Execution providers</span>
          <Popover
            :open="providerPickerOpen"
            label="Execution providers"
            side="below"
            :width="360"
            @close="providerPickerOpen = false"
          >
            <template #trigger>
              <button
                type="button"
                :disabled="!routingLoaded || mutatingModel !== null"
                :aria-expanded="providerPickerOpen"
                aria-haspopup="dialog"
                aria-label="Choose execution providers"
                class="flex w-[260px] min-w-0 items-center gap-2 rounded-[6px] border border-line/60 bg-raised px-2.5 py-1.5 text-left text-[12.5px] text-fg transition-colors hover:border-line-strong hover:bg-raised/80 focus-visible:border-accent/80 focus-visible:ring-1 focus-visible:ring-accent/40 disabled:opacity-40"
                data-action="open-openrouter-providers"
                @click="toggleProviderPicker"
              >
                <span class="min-w-0 flex-1 truncate">{{ choiceLabel }}</span>
                <Icon name="chevron-down" class="h-3.5 w-3.5 shrink-0 text-faint" />
              </button>
            </template>

            <div class="flex flex-col">
              <div class="flex items-center gap-2 rounded-[6px] bg-raised px-2.5 py-2">
                <Icon name="search" class="h-3.5 w-3.5 shrink-0 text-faint" />
                <input
                  v-model="providerQuery"
                  type="search"
                  spellcheck="false"
                  placeholder="Search providers"
                  aria-label="Search endpoint providers"
                  class="min-w-0 flex-1 bg-transparent text-[12.5px] text-fg outline-none placeholder:text-faint"
                  data-field="openrouter-provider-search"
                />
                <span class="shrink-0 text-[11px] text-faint" role="status" aria-live="polite">
                  {{ automatic ? "automatic" : `${selectedProviders.length} selected` }}
                </span>
              </div>

              <button
                type="button"
                :aria-pressed="automatic"
                :disabled="mutatingModel !== null"
                class="mt-1.5 flex w-full items-start gap-2.5 rounded-[6px] px-2.5 py-2 text-left transition-colors disabled:opacity-40"
                :class="automatic ? 'bg-selected' : 'hover:bg-raised'"
                data-action="automatic-openrouter-routing"
                @click="chooseAutomatic"
              >
                <span class="w-3.5 shrink-0 pt-0.5 text-[11px] text-faint">
                  <Icon v-if="automatic" name="check" class="h-3.5 w-3.5 text-accent" />
                </span>
                <span class="min-w-0 flex-1">
                  <span class="block text-[13px] text-fg">Automatic routing</span>
                  <span class="mt-0.5 block text-[11px] leading-relaxed text-faint">
                    Removes this model’s provider restriction.
                  </span>
                </span>
              </button>

              <div v-if="selectedProviders.length > 0" class="mt-2 flex flex-wrap gap-1 px-2.5">
                <span
                  v-for="slug in selectedProviders"
                  :key="slug"
                  class="rounded-[4px] border border-line/40 bg-raised px-1.5 py-0.5 font-mono text-[10.5px] text-dim"
                >
                  {{ slug }}
                </span>
              </div>

              <div
                role="group"
                aria-label="Endpoint providers"
                class="mt-1.5 max-h-64 overflow-y-auto"
              >
                <label
                  v-for="provider in filteredProviders"
                  :key="provider.slug"
                  class="flex cursor-pointer items-center gap-2.5 rounded-[6px] px-2.5 py-2 hover:bg-raised"
                >
                  <input
                    type="checkbox"
                    class="h-3.5 w-3.5 shrink-0"
                    style="accent-color: var(--color-accent)"
                    :checked="selectedProviderSet.has(provider.slug)"
                    :disabled="mutatingModel !== null"
                    :data-provider="provider.slug"
                    @change="toggleProvider(provider.slug)"
                  />
                  <span class="min-w-0 flex-1 truncate text-[13px] text-fg">{{ provider.name }}</span>
                  <span
                    class="shrink-0 font-mono text-[10.5px]"
                    :class="provider.savedOnly ? 'text-warn' : 'text-faint'"
                  >
                    {{ provider.savedOnly ? "saved" : provider.slug }}
                  </span>
                </label>
                <p v-if="filteredProviders.length === 0" class="px-2.5 py-4 text-[12.5px] text-faint">
                  {{ providerQuery.trim() === "" ? "No endpoint providers available." : `No provider matches “${providerQuery.trim()}”.` }}
                </p>
              </div>

              <p class="px-2.5 pb-0.5 pt-2 text-[10.5px] leading-relaxed text-faint">
                Selections apply when you save.
                <template v-if="hasSavedOnlyProviders"> Providers marked saved are not in current discovery.</template>
              </p>
            </div>
          </Popover>
        </div>

        <span v-if="loadingRouting" class="text-[11.5px] text-faint" role="status">
          Loading saved restriction…
        </span>
        <span v-else-if="loadingProviders" class="text-[11.5px] text-faint" role="status">
          Loading endpoint providers…
        </span>

        <div class="ml-auto flex flex-wrap items-center gap-2">
          <button
            v-if="routingError || providersError"
            type="button"
            class="rounded-[6px] border border-line px-2.5 py-1 text-[11.5px] text-dim transition-colors hover:border-line-strong hover:bg-raised hover:text-fg disabled:opacity-50"
            :disabled="loadingRouting || loadingProviders || mutatingModel !== null"
            data-action="retry-openrouter-editor"
            @click="retryEditor"
          >
            Retry loading
          </button>
          <button
            type="button"
            class="rounded-[6px] bg-accent px-3 py-1.5 text-[12px] font-semibold text-accent-contrast transition-all hover:brightness-110 active:scale-95 disabled:opacity-40"
            :disabled="!canSave"
            data-action="save-openrouter-routing"
            @click="save"
          >
            {{ saving ? "Saving…" : "Save restriction" }}
          </button>
        </div>
      </div>

      <p
        v-if="routingLoaded && !automatic && selectedProviders.length > 0"
        class="mt-2 max-w-[70ch] text-[12px] leading-relaxed text-dim"
      >
        Only {{ selectedProviders.length === 1 ? "this execution provider is" : "these execution providers are" }}
        allowed for this model. If none are available, the request fails rather than using
        another provider. OMP model fallbacks can still select another model.
      </p>
      <p
        v-else-if="routingLoaded && !automatic && selectedProviders.length === 0"
        class="mt-2 max-w-[70ch] text-[11.5px] leading-relaxed text-warn"
      >
        <template v-if="restriction !== null && restriction.length === 0">
          The saved restriction allows no providers, so requests for this model fail. Choose
          providers or Automatic routing.
        </template>
        <template v-else>
          Select at least one provider, or choose Automatic routing to clear the restriction.
          Nothing is saved until then.
        </template>
      </p>

      <p v-if="routingError" class="mt-2 text-[11.5px] text-err" role="alert">
        Could not load this model’s saved routing: {{ routingError }}
      </p>
      <p v-if="providersError" class="mt-2 text-[11.5px] text-err" role="alert">
        Could not load endpoint providers: {{ providersError }}. Retry discovery, or choose
        Automatic to clear the restriction; saved provider slugs stay listed meanwhile.
      </p>
      <p
        v-else-if="!loadingProviders && providers.length === 0 && routingLoaded && !hasSavedOnlyProviders"
        class="mt-2 text-[11.5px] text-faint"
        role="status"
      >
        No endpoint providers were returned for this model. Automatic routing is still available.
      </p>
      <p v-if="saveError" class="mt-2 text-[11.5px] text-err" role="alert">
        Could not save routing: {{ saveError }}
      </p>
    </div>
  </section>
</template>
