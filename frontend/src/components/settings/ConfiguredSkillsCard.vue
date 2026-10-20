<script setup lang="ts">
import { onMounted, ref } from "vue";
import { listSkills, openSkillsFolder, type SkillInfoDto } from "../../bridge";
import Icon from "../ui/Icon.vue";

const props = defineProps<{
  workspace?: string | null | undefined;
}>();

const emit = defineEmits<{
  (e: "openAddModal"): void;
}>();

const skills = ref<SkillInfoDto[]>([]);
const loading = ref(false);

async function load(): Promise<void> {
  loading.value = true;
  try {
    skills.value = await listSkills(props.workspace ?? null);
  } catch {
    // best-effort
  } finally {
    loading.value = false;
  }
}

async function handleOpenFolder(scope: "user" | "project"): Promise<void> {
  try {
    await openSkillsFolder(scope, props.workspace ?? null);
  } catch {
    // ignore
  }
}

onMounted(() => {
  void load();
});

defineExpose({
  reload: load,
});
</script>

<template>
  <div class="mb-6 rounded-[10px] border border-line bg-surface p-4 shadow-sm">
    <div class="flex items-center justify-between border-b border-line/50 pb-3">
      <div class="flex items-center gap-2">
        <Icon name="zap" class="h-4 w-4 text-accent" />
        <h3 class="text-[13px] font-semibold text-fg">Active Skills</h3>
        <span
          v-if="!loading"
          class="rounded-full bg-raised px-2 py-0.5 text-[11px] font-medium text-dim"
        >
          {{ skills.length }}
        </span>
      </div>

      <div class="flex items-center gap-2">
        <button
          type="button"
          class="flex items-center gap-1.5 rounded-[6px] border border-line/60 bg-canvas px-2.5 py-1 text-[11.5px] font-medium text-dim transition-colors hover:bg-raised hover:text-fg"
          title="Open global skills directory in file manager"
          @click="handleOpenFolder('user')"
        >
          <Icon name="folder" class="h-3.5 w-3.5" />
          <span>Skills Folder</span>
        </button>

        <button
          type="button"
          class="flex items-center gap-1.5 rounded-[6px] bg-accent px-3 py-1 text-[12px] font-medium text-accent-contrast transition-opacity hover:opacity-90"
          @click="emit('openAddModal')"
        >
          <Icon name="plus" class="h-3.5 w-3.5" />
          <span>Add Skill</span>
        </button>
      </div>
    </div>

    <!-- Skill List -->
    <div v-if="loading" class="flex items-center justify-center py-6 text-[12px] text-faint">
      <Icon name="refresh" class="mr-2 h-3.5 w-3.5 animate-spin" />
      Loading skills…
    </div>

    <div v-else-if="skills.length === 0" class="flex flex-col items-center justify-center py-6 text-center">
      <p class="text-[12.5px] text-dim">No skills installed yet</p>
      <p class="mt-1 max-w-sm text-[11.5px] text-faint">
        Add custom skills or import existing agent skills from GitHub (like <code>pbakaus/impeccable</code>) to give your assistant specialized workflows.
      </p>
    </div>

    <div v-else class="mt-3 divide-y divide-line/40 max-h-72 overflow-auto">
      <div
        v-for="skill in skills"
        :key="skill.path"
        class="flex items-start justify-between py-2.5 first:pt-1 last:pb-1"
      >
        <div class="flex min-w-0 flex-1 flex-col gap-0.5 pr-3">
          <div class="flex items-center gap-2">
            <span class="font-mono text-[12.5px] font-semibold text-fg">
              {{ skill.name }}
            </span>
            <span
              class="rounded px-1.5 py-0.2 text-[10px] font-medium uppercase tracking-wider"
              :class="
                skill.scope === 'user'
                  ? 'bg-accent/15 text-accent'
                  : skill.scope === 'project'
                    ? 'bg-success/15 text-success'
                    : 'bg-raised text-faint'
              "
            >
              {{ skill.scope }}
            </span>
            <span
              v-if="skill.hasScripts"
              class="rounded bg-surface border border-line/60 px-1 py-0.2 text-[9.5px] text-dim"
            >
              scripts
            </span>
            <span
              v-if="skill.hasReferences"
              class="rounded bg-surface border border-line/60 px-1 py-0.2 text-[9.5px] text-dim"
            >
              references
            </span>
          </div>
          <p class="text-[11.5px] leading-relaxed text-dim line-clamp-2">
            {{ skill.description || "(No description)" }}
          </p>
          <span class="truncate font-mono text-[10.5px] text-faint" :title="skill.path">
            {{ skill.path }}
          </span>
        </div>
      </div>
    </div>
  </div>
</template>
