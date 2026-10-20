<script setup lang="ts">
import { computed, ref } from "vue";
import Modal from "../Modal.vue";
import Icon from "../ui/Icon.vue";
import {
  createCustomSkill,
  importRemoteSkill,
  inspectRemoteSkill,
  type InspectSkillResultDto,
} from "../../bridge";

const props = defineProps<{
  workspace?: string | null | undefined;
}>();

const emit = defineEmits<{
  (e: "close"): void;
  (e: "added", skillName: string): void;
}>();

const tab = ref<"import" | "custom">("import");
const busy = ref(false);
const errorMessage = ref<string | null>(null);

// Import Tab State
const sourceUrl = ref("");
const inspecting = ref(false);
const inspectedResult = ref<InspectSkillResultDto | null>(null);
const selectedSkillName = ref<string>("");
const importScope = ref<"user" | "project">("user");

// Custom Tab State
const customName = ref("");
const customDescription = ref("");
const customScope = ref<"user" | "project">("user");
const customContent = ref(`# Role & Objective
Describe the main objective of this skill.

# Guidelines & Workflow
1. Analyze the context and constraints before taking action.
2. Formulate a plan and follow project conventions.
3. Validate all changes with tests.
`);

const nameError = computed(() => {
  const trimmed = customName.value.trim();
  if (!trimmed) return null;
  if (!/^[a-zA-Z0-9_-]+$/.test(trimmed)) {
    return "Skill name may only contain letters, numbers, hyphens, and underscores.";
  }
  return null;
});

const canSubmitCustom = computed(() => {
  return (
    customName.value.trim().length > 0 &&
    nameError.value === null &&
    customDescription.value.trim().length > 0 &&
    customContent.value.trim().length > 0 &&
    !busy.value
  );
});

async function onInspect(): Promise<void> {
  const url = sourceUrl.value.trim();
  if (!url) {
    errorMessage.value = "Please enter a GitHub repository URL or shorthand.";
    return;
  }

  inspecting.value = true;
  errorMessage.value = null;
  inspectedResult.value = null;

  try {
    const result = await inspectRemoteSkill(url);
    inspectedResult.value = result;
    if (result.skills.length > 0) {
      selectedSkillName.value = result.skills[0].name;
    }
  } catch (err) {
    errorMessage.value = String(err);
  } finally {
    inspecting.value = false;
  }
}

async function onInstallImported(): Promise<void> {
  if (!inspectedResult.value || !selectedSkillName.value) return;

  busy.value = true;
  errorMessage.value = null;

  try {
    const installed = await importRemoteSkill({
      sourceUrl: sourceUrl.value.trim(),
      skillName: selectedSkillName.value,
      scope: importScope.value,
      workspace: props.workspace ?? null,
    });
    emit("added", installed.name);
    emit("close");
  } catch (err) {
    errorMessage.value = String(err);
  } finally {
    busy.value = false;
  }
}

async function onCreateCustom(): Promise<void> {
  if (!canSubmitCustom.value) return;

  busy.value = true;
  errorMessage.value = null;

  try {
    const created = await createCustomSkill({
      name: customName.value.trim(),
      description: customDescription.value.trim(),
      content: customContent.value.trim(),
      scope: customScope.value,
      workspace: props.workspace ?? null,
    });
    emit("added", created.name);
    emit("close");
  } catch (err) {
    errorMessage.value = String(err);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <Modal title="Add Skill" :busy="busy || inspecting" @close="emit('close')">
    <div class="flex flex-col gap-4">
      <!-- Error banner -->
      <div
        v-if="errorMessage"
        class="flex items-start gap-2 rounded-[8px] border border-err/30 bg-err/10 p-2.5 text-[12px] text-err"
      >
        <Icon name="alert" class="mt-0.5 h-3.5 w-3.5 shrink-0" />
        <span class="break-words">{{ errorMessage }}</span>
      </div>

      <!-- Tab selector -->
      <div class="flex rounded-[8px] bg-canvas/70 p-1">
        <button
          type="button"
          class="flex-1 rounded-[6px] py-1.5 text-[12.5px] font-medium transition-colors"
          :class="tab === 'import' ? 'bg-surface text-fg shadow-sm' : 'text-faint hover:text-dim'"
          @click="tab = 'import'"
        >
          Import from GitHub / URL
        </button>
        <button
          type="button"
          class="flex-1 rounded-[6px] py-1.5 text-[12.5px] font-medium transition-colors"
          :class="tab === 'custom' ? 'bg-surface text-fg shadow-sm' : 'text-faint hover:text-dim'"
          @click="tab = 'custom'"
        >
          Create Custom Skill
        </button>
      </div>

      <!-- Tab 1: Import from GitHub -->
      <div v-if="tab === 'import'" class="flex flex-col gap-3.5">
        <div class="flex flex-col gap-1.5">
          <label class="text-[12px] font-medium text-dim">
            GitHub Repository or Shorthand
          </label>
          <div class="flex gap-2">
            <input
              v-model="sourceUrl"
              type="text"
              placeholder="e.g. pbakaus/impeccable or https://github.com/..."
              class="min-w-0 flex-1 rounded-[7px] border border-line bg-canvas px-3 py-1.5 text-[13px] text-fg placeholder:text-faint focus:border-accent focus:outline-none"
              :disabled="inspecting || busy"
              @keydown.enter.prevent="onInspect"
            />
            <button
              type="button"
              class="flex items-center gap-1.5 rounded-[7px] bg-accent px-3 py-1.5 text-[12.5px] font-medium text-accent-contrast transition-opacity hover:opacity-90 disabled:opacity-50"
              :disabled="!sourceUrl.trim() || inspecting || busy"
              @click="onInspect"
            >
              <Icon v-if="inspecting" name="refresh" class="h-3.5 w-3.5 animate-spin" />
              <Icon v-else name="search" class="h-3.5 w-3.5" />
              <span>{{ inspecting ? "Inspecting…" : "Inspect" }}</span>
            </button>
          </div>
          <span class="text-[11px] text-faint">
            Enter a repository with an Agent Skill (e.g. <code>pbakaus/impeccable</code>).
          </span>
        </div>

        <!-- Inspected skills result -->
        <div
          v-if="inspectedResult"
          class="flex flex-col gap-2.5 rounded-[8px] border border-line/60 bg-canvas/40 p-3"
        >
          <div class="flex items-center justify-between border-b border-line/40 pb-2 text-[12px]">
            <span class="font-medium text-fg">
              Found {{ inspectedResult.skills.length }} skill{{ inspectedResult.skills.length > 1 ? "s" : "" }} in
              <span class="text-accent">{{ inspectedResult.repoName }}</span>
            </span>
          </div>

          <div class="flex flex-col gap-2 max-h-48 overflow-auto">
            <label
              v-for="skill in inspectedResult.skills"
              :key="skill.name"
              class="flex items-start gap-2.5 rounded-[6px] border border-line/40 p-2 cursor-pointer transition-colors"
              :class="selectedSkillName === skill.name ? 'border-accent bg-accent/5' : 'hover:bg-canvas'"
            >
              <input
                v-model="selectedSkillName"
                type="radio"
                name="selected-skill"
                :value="skill.name"
                class="mt-1 text-accent focus:ring-accent"
              />
              <div class="flex min-w-0 flex-1 flex-col gap-0.5">
                <div class="flex items-center gap-2">
                  <span class="font-mono text-[12.5px] font-semibold text-fg">
                    {{ skill.name }}
                  </span>
                  <span
                    v-if="skill.hasScripts"
                    class="rounded bg-accent/10 px-1.5 py-0.2 text-[10px] font-medium text-accent"
                  >
                    scripts
                  </span>
                  <span
                    v-if="skill.hasReferences"
                    class="rounded bg-surface px-1.5 py-0.2 text-[10px] font-medium text-dim"
                  >
                    reference rules
                  </span>
                </div>
                <p class="text-[11.5px] leading-relaxed text-dim line-clamp-2">
                  {{ skill.description || "(No description provided)" }}
                </p>
              </div>
            </label>
          </div>

          <!-- Scope selector -->
          <div class="mt-1 flex flex-col gap-1.5 border-t border-line/40 pt-2.5">
            <span class="text-[11.5px] font-medium text-dim">Install Location</span>
            <div class="flex gap-4 text-[12px]">
              <label class="flex items-center gap-1.5 cursor-pointer text-fg">
                <input
                  v-model="importScope"
                  type="radio"
                  value="user"
                  class="text-accent focus:ring-accent"
                />
                <span>Global (~/.omp/agent/skills)</span>
              </label>
              <label
                class="flex items-center gap-1.5 cursor-pointer text-fg"
                :class="{ 'opacity-50 cursor-not-allowed': !props.workspace }"
              >
                <input
                  v-model="importScope"
                  type="radio"
                  value="project"
                  :disabled="!props.workspace"
                  class="text-accent focus:ring-accent"
                />
                <span>Project (.omp/skills)</span>
              </label>
            </div>
          </div>
        </div>

        <!-- Footer Actions for Import -->
        <div class="mt-2 flex justify-end gap-2 border-t border-line/40 pt-3">
          <button
            type="button"
            class="rounded-[7px] border border-line px-3.5 py-1.5 text-[12.5px] font-medium text-dim hover:bg-raised hover:text-fg"
            @click="emit('close')"
          >
            Cancel
          </button>
          <button
            type="button"
            class="flex items-center gap-1.5 rounded-[7px] bg-accent px-4 py-1.5 text-[12.5px] font-medium text-accent-contrast transition-opacity hover:opacity-90 disabled:opacity-40"
            :disabled="!inspectedResult || !selectedSkillName || busy"
            @click="onInstallImported"
          >
            <Icon v-if="busy" name="refresh" class="h-3.5 w-3.5 animate-spin" />
            <Icon v-else name="download" class="h-3.5 w-3.5" />
            <span>{{ busy ? "Installing…" : "Install Skill" }}</span>
          </button>
        </div>
      </div>

      <!-- Tab 2: Create Custom Skill -->
      <div v-else class="flex flex-col gap-3">
        <div class="flex flex-col gap-1">
          <label class="text-[12px] font-medium text-dim">Skill Name</label>
          <input
            v-model="customName"
            type="text"
            placeholder="e.g. test-runner, pr-audit, react-refactor"
            class="rounded-[7px] border border-line bg-canvas px-3 py-1.5 text-[13px] text-fg placeholder:text-faint focus:border-accent focus:outline-none"
            :class="{ 'border-err': nameError }"
          />
          <span v-if="nameError" class="text-[11px] text-err">{{ nameError }}</span>
          <span v-else class="text-[11px] text-faint">
            Use lowercase letters, numbers, and hyphens. This becomes <code>/skill:{{ customName.trim() || 'name' }}</code>.
          </span>
        </div>

        <div class="flex flex-col gap-1">
          <label class="text-[12px] font-medium text-dim">Description</label>
          <input
            v-model="customDescription"
            type="text"
            placeholder="e.g. Run and debug unit tests, reporting broken assertions"
            class="rounded-[7px] border border-line bg-canvas px-3 py-1.5 text-[13px] text-fg placeholder:text-faint focus:border-accent focus:outline-none"
          />
          <span class="text-[11px] text-faint">
            Explains to the AI model and in the command palette when this skill should be used.
          </span>
        </div>

        <div class="flex flex-col gap-1.5">
          <span class="text-[12px] font-medium text-dim">Install Location</span>
          <div class="flex gap-4 text-[12px]">
            <label class="flex items-center gap-1.5 cursor-pointer text-fg">
              <input
                v-model="customScope"
                type="radio"
                value="user"
                class="text-accent focus:ring-accent"
              />
              <span>Global (~/.omp/agent/skills)</span>
            </label>
            <label
              class="flex items-center gap-1.5 cursor-pointer text-fg"
              :class="{ 'opacity-50 cursor-not-allowed': !props.workspace }"
            >
              <input
                v-model="customScope"
                type="radio"
                value="project"
                :disabled="!props.workspace"
                class="text-accent focus:ring-accent"
              />
              <span>Project (.omp/skills)</span>
            </label>
          </div>
        </div>

        <div class="flex flex-col gap-1">
          <label class="text-[12px] font-medium text-dim">Skill Instructions (Markdown)</label>
          <textarea
            v-model="customContent"
            rows="7"
            class="rounded-[7px] border border-line bg-canvas p-2.5 font-mono text-[12px] leading-relaxed text-fg focus:border-accent focus:outline-none"
          />
        </div>

        <!-- Footer Actions for Custom -->
        <div class="mt-2 flex justify-end gap-2 border-t border-line/40 pt-3">
          <button
            type="button"
            class="rounded-[7px] border border-line px-3.5 py-1.5 text-[12.5px] font-medium text-dim hover:bg-raised hover:text-fg"
            @click="emit('close')"
          >
            Cancel
          </button>
          <button
            type="button"
            class="flex items-center gap-1.5 rounded-[7px] bg-accent px-4 py-1.5 text-[12.5px] font-medium text-accent-contrast transition-opacity hover:opacity-90 disabled:opacity-40"
            :disabled="!canSubmitCustom"
            @click="onCreateCustom"
          >
            <Icon v-if="busy" name="refresh" class="h-3.5 w-3.5 animate-spin" />
            <Icon v-else name="sparkle" class="h-3.5 w-3.5" />
            <span>{{ busy ? "Creating…" : "Create Skill" }}</span>
          </button>
        </div>
      </div>
    </div>
  </Modal>
</template>
