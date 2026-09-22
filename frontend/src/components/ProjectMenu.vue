<script setup lang="ts">
/**
 * Context menu for a project group row in the sidebar.
 *
 * Appears on right-click, providing standard directory/project actions:
 * new thread, reveal in file manager, copy path, and delete project.
 */
import { onMounted, onUnmounted, ref } from "vue";
import { openPath } from "../bridge";
import type { ProjectGroup } from "../lib/threads";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  group: ProjectGroup;
  /** Where the press happened, in viewport coordinates. */
  at: { x: number; y: number };
}>();

const emit = defineEmits<{
  close: [];
  open: [workspace: string];
  delete: [group: ProjectGroup];
}>();

const panel = ref<HTMLElement | null>(null);
const placement = ref({ left: "0px", top: "0px" });

function place(): void {
  const width = panel.value?.offsetWidth ?? 200;
  const height = panel.value?.offsetHeight ?? 160;
  placement.value = {
    left: `${Math.min(Math.max(8, props.at.x), Math.max(8, window.innerWidth - width - 8))}px`,
    top: `${Math.min(Math.max(8, props.at.y), Math.max(8, window.innerHeight - height - 8))}px`,
  };
}

function onPointerDown(event: PointerEvent): void {
  const target = event.target as Node | null;
  if (target !== null && panel.value?.contains(target) === true) return;
  emit("close");
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key !== "Escape") return;
  event.preventDefault();
  event.stopPropagation();
  emit("close");
}

async function reveal(): Promise<void> {
  if (!props.group.path) return;
  emit("close");
  await openPath(props.group.path);
}

async function copyPath(): Promise<void> {
  if (!props.group.path) return;
  emit("close");
  await navigator.clipboard.writeText(props.group.path);
}

function newThread(): void {
  if (!props.group.path) return;
  emit("close");
  emit("open", props.group.path);
}

function deleteProject(): void {
  emit("close");
  emit("delete", props.group);
}

onMounted(() => {
  place();
  requestAnimationFrame(place);
  document.addEventListener("pointerdown", onPointerDown, true);
  document.addEventListener("keydown", onKeydown, true);
});

onUnmounted(() => {
  document.removeEventListener("pointerdown", onPointerDown, true);
  document.removeEventListener("keydown", onKeydown, true);
});
</script>

<template>
  <Teleport to="body">
    <div
      ref="panel"
      role="menu"
      :aria-label="`actions for project ${props.group.name}`"
      class="fixed z-40 min-w-52 overflow-hidden rounded-[10px] border border-line bg-surface p-1.5 shadow-xl shadow-black/50"
      :style="placement"
    >
      <button
        role="menuitem"
        :disabled="!props.group.path"
        class="flex w-full items-center gap-2.5 rounded-[6px] px-2.5 py-1.5 text-left text-[12.5px] text-dim transition-colors hover:bg-raised hover:text-fg disabled:cursor-not-allowed disabled:text-faint/50"
        @click="newThread"
      >
        <Icon name="plus" class="h-3.5 w-3.5 shrink-0" />
        <span class="min-w-0 flex-1 truncate">New thread</span>
      </button>

      <button
        role="menuitem"
        :disabled="!props.group.path"
        class="flex w-full items-center gap-2.5 rounded-[6px] px-2.5 py-1.5 text-left text-[12.5px] text-dim transition-colors hover:bg-raised hover:text-fg disabled:cursor-not-allowed disabled:text-faint/50"
        @click="reveal"
      >
        <Icon name="folder-open" class="h-3.5 w-3.5 shrink-0" />
        <span class="min-w-0 flex-1 truncate">Show in file manager</span>
      </button>

      <button
        role="menuitem"
        :disabled="!props.group.path"
        class="flex w-full items-center gap-2.5 rounded-[6px] px-2.5 py-1.5 text-left text-[12.5px] text-dim transition-colors hover:bg-raised hover:text-fg disabled:cursor-not-allowed disabled:text-faint/50"
        @click="copyPath"
      >
        <Icon name="copy" class="h-3.5 w-3.5 shrink-0" />
        <span class="min-w-0 flex-1 truncate">Copy path</span>
      </button>

      <hr class="my-1 border-t border-line" />

      <button
        role="menuitem"
        class="flex w-full items-center gap-2.5 rounded-[6px] px-2.5 py-1.5 text-left text-[12.5px] text-dim transition-colors hover:bg-raised hover:text-err"
        @click="deleteProject"
      >
        <Icon name="trash" class="h-3.5 w-3.5 shrink-0" />
        <span class="min-w-0 flex-1 truncate">Delete project…</span>
      </button>
    </div>
  </Teleport>
</template>
