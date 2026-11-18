<script setup lang="ts">
/**
 * One section of the settings screen: its title, its blurb, and its groups (`docs/12` §12).
 *
 * The group heading is the engine's own group name, kept uppercase over a hairline rule — the
 * reference's section header. Rows are drawn by `SettingsRow`, which owns the anatomy; this
 * file owns the vertical rhythm between them. Their already-prioritized order arrives from the
 * screen model, so this renderer contains no second ordering policy.
 */
import { computed, onMounted, ref, watch } from "vue";
import {
  getCatalogProviders,
  getConfiguredProviders,
  refreshModels,
  removeCustomProvider,
  removeProviderCredential,
  type CatalogProviderDto,
  type ProviderAccountDto,
  type SettingsRow as SettingsRowModel,
  type SettingsSection,
} from "../../bridge";
import type { AppTheme } from "../../lib/appTheme";
import { partitionSectionGroups, rowId } from "../../lib/settings";
import Icon from "../ui/Icon.vue";
import AddProviderModal from "./AddProviderModal.vue";
import AddSkillModal from "./AddSkillModal.vue";
import AppThemeSetting from "./AppThemeSetting.vue";
import ConfiguredProvidersCard from "./ConfiguredProvidersCard.vue";
import ConfiguredSkillsCard from "./ConfiguredSkillsCard.vue";
import McpDesignChoiceSetting from "./McpDesignChoiceSetting.vue";
import OpenRouterRoutingSetting from "./OpenRouterRoutingSetting.vue";
import NotificationSoundSetting from "./NotificationSoundSetting.vue";
import SettingsRow from "./SettingsRow.vue";

const props = defineProps<{
  section: SettingsSection;
  /** The keys with a write in flight, so the row can show it. */
  pending: Set<string>;
  /** The host's sentence per row, after a write. Absent for rows nobody has written. */
  messages: Record<string, string | undefined>;
  /** The host's refusal per row. */
  errors: Record<string, string | undefined>;
  /** The row a search hit landed on, highlighted until the next navigation. */
  highlight: string | null;
  /** Where a gated row's `needs` key lives, resolved against the whole catalog. */
  locate: (key: string) => { sectionId: string; title: string } | null;
  notificationSoundEnabled: boolean;
  appTheme: AppTheme;
  workspace?: string | null | undefined;
  mcpDesignChoiceEnabled?: boolean;
}>();

const emit = defineEmits<{
  (event: "write", row: SettingsRowModel, value: unknown): void;
  (event: "reset", row: SettingsRowModel): void;
  (event: "navigate", sectionId: string, rowId: string): void;
  (event: "action", action: string): void;
  (event: "changed"): void;
  (event: "notificationSound", enabled: boolean): void;
  (event: "appTheme", theme: AppTheme): void;
  (event: "terminalLogin", providerId?: string): void;
  (event: "mcpDesignChoice", enabled: boolean): void;
}>();

const configuredProviders = ref<ProviderAccountDto[]>([]);
const catalogProviders = ref<CatalogProviderDto[]>([]);
const loadingProviders = ref(false);
const showAddModal = ref(false);
const showAddSkillModal = ref(false);
const skillsCardRef = ref<InstanceType<typeof ConfiguredSkillsCard> | null>(null);

function onSkillAdded(): void {
  skillsCardRef.value?.reload();
  emit("changed");
}

async function loadProviders(): Promise<void> {
  if (props.section.id !== "model-and-providers") return;
  loadingProviders.value = true;
  try {
    const [conf, cat] = await Promise.all([
      getConfiguredProviders(),
      getCatalogProviders(),
    ]);
    configuredProviders.value = conf;
    catalogProviders.value = cat;
    void refreshModels().catch(() => {});
  } catch {
    // best-effort
  } finally {
    loadingProviders.value = false;
  }
}

const advancedOpen = ref(false);

const partitioned = computed(() => partitionSectionGroups(props.section, props.highlight));

watch(
  () => props.section.id,
  (id) => {
    advancedOpen.value = false;
    if (id === "model-and-providers") {
      void loadProviders();
    }
  },
  { immediate: true }
);

onMounted(() => {
  if (props.section.id === "model-and-providers") {
    void loadProviders();
  }
});

async function onProviderAdded(): Promise<void> {
  await loadProviders();
  try {
    await refreshModels();
  } catch {
    // ignore
  }
  emit("changed");
}

async function handleRemoveCredential(id: number): Promise<void> {
  try {
    await removeProviderCredential(id);
    await loadProviders();
    try {
      await refreshModels();
    } catch {
      // ignore
    }
    emit("changed");
  } catch {
    // ignore
  }
}

async function handleRemoveCustom(providerId: string): Promise<void> {
  try {
    await removeCustomProvider(providerId);
    await loadProviders();
    try {
      await refreshModels();
    } catch {
      // ignore
    }
    emit("changed");
  } catch {
    // ignore
  }
}

/** A gated row's link target, or `null` when the host named no key to reach. */
function needsFor(row: SettingsRowModel): { sectionId: string; title: string } | null {
  if (row.role !== "setting" || row.gated?.needs == null) return null;
  return props.locate(row.gated.needs);
}
</script>

<template>
  <div>
    <h1 class="text-[24px] font-semibold tracking-tight text-fg">{{ props.section.title }}</h1>
    <p v-if="props.section.blurb !== ''" class="mt-1.5 text-[13px] leading-relaxed text-dim">
      {{ props.section.blurb }}
    </p>

    <NotificationSoundSetting
      v-if="props.section.id === 'notifications'"
      :enabled="props.notificationSoundEnabled"
      @change="emit('notificationSound', $event)"
    />

    <AppThemeSetting
      v-if="props.section.id === 'appearance'"
      :theme="props.appTheme"
      @change="emit('appTheme', $event)"
    />

    <McpDesignChoiceSetting
      v-if="props.section.id === 'mcp'"
      :enabled="props.mcpDesignChoiceEnabled ?? true"
      @change="emit('mcpDesignChoice', $event)"
    />

    <div v-if="props.section.id === 'model-and-providers'" class="mt-6">
      <ConfiguredProvidersCard
        :providers="configuredProviders"
        :loading="loadingProviders"
        @add="showAddModal = true"
        @remove-credential="handleRemoveCredential"
        @remove-custom="handleRemoveCustom"
        @refresh="loadProviders"
        @open-terminal-login="emit('terminalLogin')"
      />
    </div>

    <div v-if="props.section.id === 'skills'" class="mt-6">
      <ConfiguredSkillsCard
        ref="skillsCardRef"
        :workspace="props.workspace"
        @open-add-modal="showAddSkillModal = true"
      />
    </div>

    <AddProviderModal
      v-if="showAddModal"
      :catalog="catalogProviders"
      @close="showAddModal = false"
      @added="onProviderAdded"
      @open-terminal-login="emit('terminalLogin', $event)"
    />

    <AddSkillModal
      v-if="showAddSkillModal"
      :workspace="props.workspace"
      @close="showAddSkillModal = false"
      @added="onSkillAdded"
    />

    <template v-for="group in partitioned.essentialGroups" :key="group.name">
      <section class="mt-7">
        <h2 class="px-1 pb-2 text-[12px] font-medium text-dim">
          {{ group.name }}
        </h2>

        <div class="flex flex-col divide-y divide-line/60 overflow-hidden rounded-[10px] border border-line bg-raised/35">
          <SettingsRow
            v-for="row in group.rows"
            :key="rowId(row)"
            :row="row"
            :pending="props.pending.has(rowId(row))"
            :message="props.messages[rowId(row)] ?? null"
            :error="props.errors[rowId(row)] ?? null"
            :highlighted="props.highlight === rowId(row)"
            :needs="needsFor(row)"
            @write="emit('write', row, $event)"
            @reset="emit('reset', row)"
            @navigate="(sectionId, target) => emit('navigate', sectionId, target)"
            @action="emit('action', $event)"
            @changed="emit('changed')"
          />
        </div>
      </section>

      <OpenRouterRoutingSetting
        v-if="props.section.id === 'model-and-providers' && group.rows.some((r) => r.role === 'setting' && r.key === 'modelRoles')"
        class="mt-7"
      />
    </template>

    <details
      v-if="partitioned.advancedCount > 0"
      :open="partitioned.forceOpenAdvanced || advancedOpen"
      class="mt-8 group/adv"
      @toggle="advancedOpen = ($event.target as HTMLDetailsElement).open"
    >
      <summary class="flex cursor-pointer list-none items-center justify-between gap-3 rounded-[10px] border border-line/80 bg-raised/25 px-4 py-3 text-[13px] font-medium text-fg transition-colors hover:bg-raised/45 select-none [&::-webkit-details-marker]:hidden">
        <span class="flex items-center gap-2.5">
          <Icon name="sliders" class="h-4 w-4 text-dim" />
          <span>Advanced Tuning</span>
        </span>
        <span class="flex items-center gap-2">
          <span class="rounded-full border border-line bg-surface/70 px-2 py-0.5 tabular-nums text-[10.5px] text-faint">
            {{ partitioned.advancedCount }} {{ partitioned.advancedCount === 1 ? 'setting' : 'settings' }}
          </span>
          <Icon name="chevron-down" class="h-3.5 w-3.5 text-faint transition-transform group-open/adv:rotate-180" />
        </span>
      </summary>

      <div class="mt-4 flex flex-col gap-7">
        <section v-for="group in partitioned.advancedGroups" :key="group.name">
          <h2 class="px-1 pb-2 text-[12px] font-medium text-dim">
            {{ group.name }}
          </h2>

          <div class="flex flex-col divide-y divide-line/60 overflow-hidden rounded-[10px] border border-line bg-raised/35">
            <SettingsRow
              v-for="row in group.rows"
              :key="rowId(row)"
              :row="row"
              :pending="props.pending.has(rowId(row))"
              :message="props.messages[rowId(row)] ?? null"
              :error="props.errors[rowId(row)] ?? null"
              :highlighted="props.highlight === rowId(row)"
              :needs="needsFor(row)"
              @write="emit('write', row, $event)"
              @reset="emit('reset', row)"
              @navigate="(sectionId, target) => emit('navigate', sectionId, target)"
              @action="emit('action', $event)"
              @changed="emit('changed')"
            />
          </div>
        </section>
      </div>
    </details>

    <p v-if="props.section.groups.length === 0" class="mt-8 text-[12.5px] text-faint">
      This section has no rows in this build.
    </p>
  </div>
</template>
