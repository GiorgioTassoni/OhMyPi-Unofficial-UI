<script setup lang="ts">
/**
 * One node of the workspace tree (`docs/12` §8.2's Tree view), recursive by filename.
 *
 * A directory loads its own level the first time it is opened, which is the whole point of
 * asking the host one level at a time: nothing here walks a tree, so no threshold of the
 * workspace can stall the panel, and an unopened directory costs nothing.
 *
 * Nothing is filtered — a file browser that hides `.git` is lying about a directory the user
 * can see in their editor — and the two actions a row offers are the ones the platform has:
 * open it, or reveal its folder.
 */
import { computed, ref } from "vue";
import { openPath, workspaceTree, type WorkspaceEntry } from "../bridge";
import { formatBytes } from "../lib/panel";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  thread: string;
  /** The entry this node draws, or null for the workspace root itself. */
  node: WorkspaceEntry | null;
  /** How deep, for indentation only. */
  depth: number;
}>();

const emit = defineEmits<{ failed: [message: string] }>();

const children = ref<WorkspaceEntry[] | null>(null);
const open = ref(false);
const loading = ref(false);
const failure = ref<string | null>(null);

const label = computed(() => (props.node === null ? "workspace" : props.node.name));
const path = computed(() => props.node?.path ?? null);

async function toggle(): Promise<void> {
  if (children.value === null) {
    loading.value = true;
    try {
      children.value = await workspaceTree(props.thread, path.value);
    } catch (cause) {
      failure.value = cause instanceof Error ? cause.message : String(cause);
      emit("failed", failure.value);
      return;
    } finally {
      loading.value = false;
    }
  }
  open.value = !open.value;
}

async function launch(target: string): Promise<void> {
  try {
    await openPath(target);
  } catch (cause) {
    failure.value = cause instanceof Error ? cause.message : String(cause);
    emit("failed", failure.value);
  }
}

/** The folder a path lives in, for the reveal action. */
function parent(target: string): string {
  const trimmed = target.replace(/\/+$/, "");
  const cut = trimmed.lastIndexOf("/");
  return cut <= 0 ? "/" : trimmed.slice(0, cut);
}
</script>

<template>
  <div class="flex flex-col pb-2">
    <!--
      One row per entry: a fold mark, the kind of thing it is, the name, and — for a file — the
      size, which is the one fact a listing adds nothing to. The fold mark and the name live in
      the same button so the whole label is the target, and the arrow turns rather than
      becoming a second glyph a reader has to learn.
    -->
    <div
      class="group flex items-center gap-1.5 rounded-[6px] py-1 pr-2 hover:bg-raised"
      :style="{ paddingLeft: `${1.5 + props.depth * 0.75}rem` }"
    >
      <button
        v-if="props.node === null || props.node.isDir"
        class="flex min-w-0 flex-1 items-center gap-1.5 text-left"
        :title="path ?? ''"
        :aria-expanded="open"
        @click="toggle"
      >
        <Icon
          :name="open ? 'chevron-down' : 'chevron-right'"
          class="h-3.5 w-3.5 shrink-0 text-faint"
        />
        <Icon :name="props.node === null ? 'folder-open' : 'folder'" class="h-3.5 w-3.5 shrink-0 text-faint" />
        <span class="min-w-0 truncate text-[12.5px] text-dim">{{ label }}</span>
      </button>
      <button
        v-else
        class="flex min-w-0 flex-1 items-center gap-1.5 text-left"
        :title="path ?? ''"
        @click="launch(props.node.path)"
      >
        <Icon name="file" class="h-3.5 w-3.5 shrink-0 text-faint" />
        <span class="min-w-0 truncate text-[12.5px] text-dim">{{ label }}</span>
      </button>

      <span v-if="props.node !== null && !props.node.isDir" class="shrink-0 font-mono text-[10.5px] text-faint">
        {{ formatBytes(props.node.size) }}
      </span>
      <button
        v-if="path !== null"
        class="grid h-5 w-5 shrink-0 place-items-center rounded-[5px] text-faint opacity-0 hover:bg-surface hover:text-fg group-hover:opacity-100"
        title="reveal this folder"
        @click="launch(props.node?.isDir ? path : parent(path))"
      >
        <Icon name="external" class="h-3.5 w-3.5" />
      </button>
    </div>

    <FileTree
      v-for="child in open ? (children ?? []) : []"
      :key="child.path"
      :thread="props.thread"
      :node="child"
      :depth="props.depth + 1"
      @failed="emit('failed', $event)"
    />

    <p
      v-if="loading"
      class="text-[11.5px] text-faint"
      :style="{ paddingLeft: `${1.5 + (props.depth + 1) * 0.75}rem` }"
    >
      reading…
    </p>
    <p
      v-else-if="open && (children ?? []).length === 0"
      class="text-[11.5px] text-faint"
      :style="{ paddingLeft: `${1.5 + (props.depth + 1) * 0.75}rem` }"
    >
      empty
    </p>
    <p v-if="failure" class="px-3 py-1 text-[11px] text-err">{{ failure }}</p>
  </div>
</template>
