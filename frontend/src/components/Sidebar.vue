<script setup lang="ts">
/**
 * The left sidebar (`docs/12` §2): navigation, not content.
 *
 * Rows are grouped by project because that is how the engine files sessions (one directory
 * per working directory), and the group's `+` starts a thread where its sessions live. The
 * grouping and ordering are computed in `lib/threads.ts` — this is the rendering, plus the
 * one piece of state the sidebar owns: which group is collapsed.
 */
import { computed, onUnmounted, ref, watch } from "vue";

import { pickDirectory } from "../bridge";
import {
  formatFriendlyTime,
  formatFullDate,
  projectName,
  reorderItems,
  type ProjectGroup,
  type ThreadRow,
} from "../lib/threads";
import logo from "../assets/ohmypi-logo.svg";
import ProjectMenu from "./ProjectMenu.vue";
import OAuthUsageSidebarWidget from "./OAuthUsageSidebarWidget.vue";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  groups: ProjectGroup[];
  activeId: string | null;
  /** A thread is being opened, so the actions are held down. */
  busy: boolean;
  /** Where a top-level "new thread" would land: the directory most recently opened. */
  workspace: string | null;
  /** The OS user and app version, for the footer row (`docs/12` §2.2). */
  identity: { user: string | null; version: string };
  /** How many sessions the catalogue holds, which is what the footer's second line counts. */
  sessions: number;
  /** The thread whose title is being edited in place, if any. */
  renameId: string | null;
  /**
   * How many subagents are in flight across every live thread (`docs/12` §2.1).
   *
   * App-wide rather than per project, because a subagent is work running *beside* the
   * conversation: "3" here means the app is busy, which is the question this row answers.
   */
  agents: number;
}>();

const emit = defineEmits<{
  select: [id: string];
  /** Start a thread in this directory. */
  open: [workspace: string];
  /** Open cross-thread search (`docs/12` §2.1) — the overlay lives in the shell. */
  search: [];
  /** Open the global agents panel (`docs/12` §9) — also in the shell. */
  agents: [];
  /** Open the settings screen (`docs/12` §12), whose own rail carries this footer row too. */
  settings: [];
  /** Right-click opened the menu (`docs/12` §2.3) — the actions live in `ThreadActions`. */
  context: [id: string, at: { x: number; y: number }];
  /** The row's trash shortcut opens the destructive confirmation directly. */
  delete: [id: string];
  /** The inline rename committed (`docs/12` §1.8: "inline rename in the session list"). */
  renamed: [id: string, name: string];
  /** The inline rename was abandoned, so the row goes back to its title. */
  cancelRename: [];
  deleteProject: [group: ProjectGroup];
  reorderProjects: [paths: string[]];
  collapse: [];
}>();

/**
 * Projects currently expanded in the sidebar.
 * Initialized empty so every project starts minimized on launch to keep the workspace clean.
 */
const openProjects = ref(new Set<string>());

/**
 * The "open a directory" row, which is where a path is typed now.
 *
 * It lives here rather than in the titlebar because opening a directory is what a project *is*:
 * the row a user reaches for sits with the projects, and the empty state points at the same
 * place. Enter commits, Escape and a press elsewhere abandon it — a path that opened on blur
 * would fire while someone was still reading it.
 */
const adding = ref(false);
const typed = ref("");

function submitDirectory(): void {
  const path = typed.value.trim();
  adding.value = false;
  typed.value = "";
  if (path !== "") {
    openProjects.value.add(path);
    emit("open", path);
  }
}

async function chooseDirectory(preferredPath?: string | Event): Promise<void> {
  try {
    const raw = typeof preferredPath === "string" ? preferredPath : "";
    const candidate = raw.trim() || typed.value.trim() || undefined;
    const path = await pickDirectory("Open Project", candidate);
    if (path) {
      adding.value = false;
      typed.value = "";
      openProjects.value.add(path);
      emit("open", path);
    }
  } catch {
    // If native dialog is unavailable, fallback to manual text input
    adding.value = true;
  }
}

/** The rename box's contents, seeded from the row that opened it. */
const draft = ref("");

// Seeded when the shell asks for a rename, so the box opens showing the current title
// rather than empty.
watch(
  () => props.renameId,
  (id) => {
    if (id === null) return;
    const row = props.groups.flatMap((group) => group.threads).find((thread) => thread.id === id);
    draft.value = row?.title ?? "";
  },
  { immediate: true },
);

// When a thread is active, ensure its parent project folder is expanded
watch(
  () => props.activeId,
  (activeId) => {
    if (!activeId) return;
    const parent = props.groups.find((group) => group.threads.some((t) => t.id === activeId));
    if (parent?.path) {
      openProjects.value.add(parent.path);
    }
  },
  { immediate: true },
);

function commitRename(id: string): void {
  const name = draft.value.trim();
  // An empty title is refused by the engine with prose and no code, so it is refused here
  // first: the row keeps its title instead of losing it to a failed round trip.
  if (name === "") {
    emit("cancelRename");
    return;
  }
  emit("renamed", id, name);
}

function onContext(event: MouseEvent, row: ThreadRow): void {
  event.preventDefault();
  onRowMouseLeave();
  emit("context", row.id, { x: event.clientX, y: event.clientY });
}

const projectMenu = ref<{ group: ProjectGroup; at: { x: number; y: number } } | null>(null);

function onProjectContext(event: MouseEvent, group: ProjectGroup): void {
  event.preventDefault();
  projectMenu.value = { group, at: { x: event.clientX, y: event.clientY } };
}

function toggle(path: string): void {
  const next = new Set(openProjects.value);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  openProjects.value = next;
}

function isOpen(path: string): boolean {
  return openProjects.value.has(path);
}

/** Static WorkBuddy-style LED visual configurations. */
const LED_CONFIG = {
  streaming: {
    color: "bg-emerald-500 shadow-[0_0_6px_rgba(16,185,129,0.5)]",
    title: "Running...",
    animated: true,
  },
  attention: {
    color: "bg-amber-500 shadow-[0_0_6px_rgba(245,158,11,0.5)]",
    title: "Action required",
    animated: false,
  },
  error: {
    color: "bg-rose-500 shadow-[0_0_6px_rgba(244,63,94,0.5)]",
    title: "Failed in this session",
    animated: false,
  },
} as const;

/** Threads that need the user, so their group can say so while collapsed. */
function attention(rows: ThreadRow[]): number {
  return rows.filter((row) => row.dot === "attention" || row.unread).length;
}

const draggingIndex = ref<number | null>(null);
const dropTargetIndex = ref<number | null>(null);
const dropPosition = ref<"above" | "below" | null>(null);
const longPressArmed = ref(false);
const longPressIndex = ref<number | null>(null);
let longPressTimer: ReturnType<typeof setTimeout> | null = null;
let dragStartPos = { x: 0, y: 0 };

function cancelLongPress(): void {
  if (longPressTimer) {
    clearTimeout(longPressTimer);
    longPressTimer = null;
  }
  longPressArmed.value = false;
  longPressIndex.value = null;
}

function onCardMouseDown(event: MouseEvent, index: number): void {
  if (event.button !== 0) return;
  const target = event.target as HTMLElement | null;
  if (target?.closest("button")) return;

  cancelLongPress();
  longPressIndex.value = index;
  dragStartPos = { x: event.clientX, y: event.clientY };

  longPressTimer = setTimeout(() => {
    longPressArmed.value = true;
  }, 350);
}

function onCardMouseMove(event: MouseEvent): void {
  if (!longPressTimer) return;
  const dist = Math.hypot(event.clientX - dragStartPos.x, event.clientY - dragStartPos.y);
  if (dist > 6 && !longPressArmed.value) {
    cancelLongPress();
  }
}

function onCardMouseUp(event: MouseEvent, path: string): void {
  const wasArmed = longPressArmed.value;
  const isDragging = draggingIndex.value !== null;
  cancelLongPress();

  const target = event.target as HTMLElement | null;
  if (!wasArmed && !isDragging && !target?.closest("button")) {
    toggle(path);
  }
}

function onCardMouseLeave(): void {
  if (draggingIndex.value === null) {
    cancelLongPress();
  }
}

function onDragStart(event: DragEvent, index: number): void {
  if (!longPressArmed.value || longPressIndex.value !== index) {
    event.preventDefault();
    return;
  }
  draggingIndex.value = index;
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData("text/plain", String(index));
  }
}

function onDragOver(event: DragEvent, index: number): void {
  if (draggingIndex.value === null || draggingIndex.value === index) {
    dropTargetIndex.value = null;
    dropPosition.value = null;
    return;
  }
  const targetEl = event.currentTarget as HTMLElement | null;
  if (!targetEl) return;
  const rect = targetEl.getBoundingClientRect();
  const midY = rect.top + rect.height / 2;
  dropTargetIndex.value = index;
  dropPosition.value = event.clientY < midY ? "above" : "below";
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = "move";
  }
}

function onDragLeave(index: number): void {
  if (dropTargetIndex.value === index) {
    dropTargetIndex.value = null;
    dropPosition.value = null;
  }
}

function onDrop(): void {
  const fromIndex = draggingIndex.value;
  const toIndex = dropTargetIndex.value;
  const position = dropPosition.value;
  cancelLongPress();
  draggingIndex.value = null;
  dropTargetIndex.value = null;
  dropPosition.value = null;

  if (fromIndex === null || toIndex === null || position === null) return;
  const reordered = reorderItems(props.groups.map((g) => g.path), fromIndex, toIndex, position);
  emit("reorderProjects", reordered);
}

function onDragEnd(): void {
  cancelLongPress();
  draggingIndex.value = null;
  dropTargetIndex.value = null;
  dropPosition.value = null;
}

const hoveredRow = ref<ThreadRow | null>(null);
const hoverPosition = ref<{ top: number; left: number }>({ top: 0, left: 0 });
let hoverTimer: ReturnType<typeof setTimeout> | null = null;

function onRowMouseEnter(event: MouseEvent, row: ThreadRow): void {
  if (props.renameId === row.id) return;
  if (hoverTimer) clearTimeout(hoverTimer);
  const target = event.currentTarget as HTMLElement | null;
  if (!target) return;
  const rect = target.getBoundingClientRect();
  const top = Math.min(window.innerHeight - 180, Math.max(12, rect.top - 6));
  const left = rect.right + 10;

  hoverTimer = setTimeout(() => {
    hoverPosition.value = { top, left };
    hoveredRow.value = row;
  }, 220);
}

function onRowMouseLeave(): void {
  if (hoverTimer) {
    clearTimeout(hoverTimer);
    hoverTimer = null;
  }
  hoveredRow.value = null;
}

onUnmounted(() => {
  if (hoverTimer) clearTimeout(hoverTimer);
  cancelLongPress();
});

const total = computed(() => props.groups.reduce((count, group) => count + group.threads.length, 0));

const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPod|iPad/i.test(navigator.userAgent || "");
const searchShortcut = computed(() => (isMac ? "⌘K" : "Ctrl+K"));
const userInitial = computed(() => (props.identity.user ? props.identity.user.charAt(0).toUpperCase() : "π"));
</script>

<template>
  <aside class="flex w-full h-full shrink-0 flex-col border-r border-line/40 bg-rail">
    <!-- Top window drag region matching TitleBar's height -->
    <div
      class="flex h-10 shrink-0 items-center justify-between border-b border-line/40 px-2.5 select-none"
      data-tauri-drag-region
    >
      <div class="flex items-center gap-1.5">
        <button
          class="grid h-6 w-6 place-items-center rounded-[6px] text-dim transition-colors hover:bg-raised hover:text-fg"
          aria-label="collapse sidebar"
          type="button"
          @click="emit('collapse')"
        >
          <Icon name="panels" class="h-3.5 w-3.5" />
        </button>
        <img :src="logo" alt="" class="h-4 w-4 shrink-0" />
        <span class="text-[12.5px] font-semibold tracking-tight text-fg/80">OhMyPi</span>
      </div>
      <button
        :disabled="props.busy || props.workspace === null"
        class="grid h-6 w-6 place-items-center rounded-[6px] text-dim transition-colors hover:bg-raised hover:text-fg disabled:opacity-30 disabled:hover:bg-transparent"
        aria-label="new thread"
        type="button"
        @click="props.workspace && emit('open', props.workspace)"
      >
        <Icon name="plus" class="h-3.5 w-3.5" />
      </button>
    </div>

    <!--
      `docs/12` §2.1's first three rows, in the reference's language: quiet icon rows that fill
      in on hover, no borders between them, and the shortcut named on the right where it is read
      rather than hunted for.
    -->
    <div class="flex flex-col gap-0.5 p-2">
      <button
        class="group flex items-center gap-2.5 rounded-[6px] px-2.5 py-2 text-left text-[13px] text-dim transition-colors hover:bg-raised hover:text-fg"
        data-row="search"
        @click="emit('search')"
      >
        <Icon name="search" class="h-4 w-4 text-faint group-hover:text-dim" />
        Search
        <span class="ml-auto text-[11.5px] text-faint group-hover:text-dim">
          {{ searchShortcut }}
        </span>
      </button>

      <button
        :disabled="props.busy || props.workspace === null"
        class="group flex items-center gap-2.5 rounded-[6px] px-2.5 py-2 text-left text-[13px] text-dim hover:bg-raised hover:text-fg disabled:opacity-40 disabled:hover:bg-transparent"
        data-row="new-thread"
        @click="props.workspace && emit('open', props.workspace)"
      >
        <Icon name="plus" class="h-4 w-4 text-faint group-hover:text-dim" />
        New thread
      </button>

      <button
        class="group flex items-center gap-2.5 rounded-[6px] px-2.5 py-2 text-left text-[13px] text-dim hover:bg-raised hover:text-fg"
        data-row="agents"
        type="button"
        @click="emit('agents')"
      >
        <Icon name="sparkle" class="h-4 w-4 text-faint group-hover:text-dim" />
        Subagents
        <!--
          The count is the point of the row: work happening beside the conversation has to be
          visible while the thread on screen is idle.
        -->
        <span
          class="ml-auto font-mono text-[11px]"
          :class="props.agents > 0 ? 'text-accent' : 'text-faint'"
        >
          {{ props.agents }}
        </span>
      </button>
    </div>

    <!--
      The projects. `+` here is where a directory nobody has opened yet comes from: it is the
      sidebar's own subject, and the titlebar is chrome.
    -->
    <div class="flex items-center gap-1 px-2 pb-1 pt-2">
      <span class="px-1.5 text-[10.5px] font-medium uppercase tracking-[0.09em] text-faint">
        Projects
      </span>
      <button
        class="ml-auto grid h-6 w-6 place-items-center rounded-[6px] text-faint hover:bg-raised hover:text-fg"
        aria-label="open a project folder"
        data-action="open-directory"
        type="button"
        @click="chooseDirectory"
      >
        <Icon name="plus" class="h-3.5 w-3.5" />
      </button>
    </div>

    <div v-if="adding" class="px-2 pb-1">
      <div class="flex items-center gap-2 rounded-[6px] bg-raised px-2.5 py-1.5">
        <Icon name="folder" class="h-3.5 w-3.5 shrink-0 text-faint" />
        <input
          :ref="(input) => (input as HTMLInputElement | null)?.focus()"
          v-model="typed"
          spellcheck="false"
          class="min-w-0 flex-1 bg-transparent font-mono text-[12px] text-fg outline-none placeholder:text-faint"
          placeholder="/path/to/a/project"
          data-input="directory"
          @keydown.enter="submitDirectory"
          @keydown.esc="((adding = false), (typed = ''))"
          @blur="adding = false"
        />
        <button
          class="grid h-5 w-5 shrink-0 place-items-center rounded text-faint hover:bg-surface hover:text-fg"
          aria-label="browse folders"
          type="button"
          @mousedown.prevent="chooseDirectory(typed)"
        >
          <Icon name="folder-open" class="h-3.5 w-3.5" />
        </button>
      </div>
    </div>

    <nav class="min-h-0 flex-1 overflow-auto px-2 pb-2">
      <button
        v-if="total === 0"
        class="w-full rounded-[6px] px-2 py-2 text-left text-[12px] text-faint hover:bg-raised hover:text-fg"
        type="button"
        @click="chooseDirectory"
      >
        No sessions yet — click to browse for a project.
      </button>

      <section
        v-for="(group, index) in props.groups"
        :key="group.path || group.name"
        class="mb-0.5 rounded-[6px] border-t-2 border-b-2 border-transparent transition-all"
        :class="{
          'opacity-40': draggingIndex === index,
          '!border-t-accent': dropTargetIndex === index && dropPosition === 'above',
          '!border-b-accent': dropTargetIndex === index && dropPosition === 'below',
        }"
        @dragover.prevent="onDragOver($event, index)"
        @dragleave="onDragLeave(index)"
        @drop.prevent="onDrop"
      >
        <div
          class="group flex items-center gap-1 rounded-[6px] px-1.5 py-1.5 hover:bg-raised select-none cursor-pointer transition-colors"
          :class="{
            'cursor-grab': longPressArmed && longPressIndex === index && draggingIndex === null,
            'cursor-grabbing ring-1 ring-accent/60': draggingIndex === index,
            'scale-[0.99] bg-raised/70': longPressArmed && longPressIndex === index,
          }"
          :draggable="longPressArmed && longPressIndex === index"
          tabindex="0"
          role="button"
          :aria-expanded="isOpen(group.path)"
          @keydown.enter="toggle(group.path)"
          @keydown.space.prevent="toggle(group.path)"
          @mousedown="onCardMouseDown($event, index)"
          @mousemove="onCardMouseMove"
          @mouseup="onCardMouseUp($event, group.path)"
          @mouseleave="onCardMouseLeave"
          @dragstart="onDragStart($event, index)"
          @dragend="onDragEnd"
          @contextmenu.prevent="onProjectContext($event, group)"
        >
          <div class="flex min-w-0 flex-1 items-center gap-1.5 text-left pointer-events-none">
            <Icon
              :name="isOpen(group.path) ? 'folder-open' : 'folder'"
              class="h-3.5 w-3.5 shrink-0 text-accent/80"
            />
            <span class="truncate text-[12.5px] font-medium text-dim group-hover:text-fg">{{ group.name }}</span>
          </div>
          <span
            v-if="!isOpen(group.path) && attention(group.threads) > 0"
            class="shrink-0 rounded-full bg-warn/15 px-1.5 font-mono text-[10px] text-warn pointer-events-none"
          >
            {{ attention(group.threads) }}
          </span>
          <button
            :disabled="props.busy || group.path === ''"
            class="grid h-5 w-5 shrink-0 place-items-center rounded-[5px] text-faint opacity-0 hover:bg-surface hover:text-fg group-hover:opacity-100 disabled:opacity-0"
            aria-label="new thread here"
            @mousedown.stop
            @click.stop="openProjects.add(group.path); emit('open', group.path)"
          >
            <Icon name="plus" class="h-3.5 w-3.5" />
          </button>
          <button
            :disabled="props.busy"
            class="grid h-5 w-5 shrink-0 place-items-center rounded-[5px] text-faint opacity-0 hover:bg-surface hover:text-err group-hover:opacity-100 disabled:opacity-0"
            aria-label="delete project and its sessions"
            @mousedown.stop
            @click.stop="emit('deleteProject', group)"
          >
            <Icon name="trash" class="h-3.5 w-3.5" />
          </button>
        </div>

        <ul v-if="isOpen(group.path)" class="my-0.5 flex flex-col gap-0.5">
          <li v-for="row in group.threads" :key="row.id">
            <button
              class="group/row relative flex w-full items-center justify-between gap-2 rounded-[6px] py-1.5 pr-2.5 text-left transition-colors"
              :class="
                row.id === props.activeId
                  ? 'bg-selected text-fg font-medium'
                  : 'text-dim hover:bg-raised hover:text-fg'
              "
              :style="{ paddingLeft: `${row.depth === 0 ? 1.875 : 2.625}rem` }"
              @mouseenter="onRowMouseEnter($event, row)"
              @mouseleave="onRowMouseLeave"
              @click="onRowMouseLeave(); emit('select', row.id)"
              @contextmenu="onContext($event, row)"
            >
              <!--
                The rename box replaces the title rather than opening a dialog: `docs/12`
                §1.8 asks for an inline rename, and the engine rewrites the title slot in
                place, so the row is exactly what changes.
              -->
              <input
                v-if="props.renameId === row.id"
                :ref="(input) => (input as HTMLInputElement | null)?.focus()"
                v-model="draft"
                spellcheck="false"
                class="min-w-0 flex-1 rounded-[5px] bg-raised px-1.5 text-[12.5px] text-fg outline-none ring-1 ring-accent"
                @click.stop
                @keydown.enter.stop="commitRename(row.id)"
                @keydown.esc.stop="emit('cancelRename')"
                @blur="commitRename(row.id)"
              />
              <span
                v-else
                class="min-w-0 flex-1 truncate text-[12.5px]"
                :class="row.unread ? 'font-medium text-fg' : 'group-hover/row:text-fg/90'"
              >
                {{ row.title }}
              </span>

              <!-- Right-hand status / meta area: LED dot and delete action -->
              <div class="flex items-center gap-1.5 shrink-0">
                <Icon
                  v-if="row.pinned"
                  name="pin"
                  class="h-3 w-3 text-faint"
                />
                <span
                  v-if="row.led"
                  class="size-2 rounded-full shrink-0"
                  :class="[LED_CONFIG[row.led].color, LED_CONFIG[row.led].animated ? 'animate-led-bounce' : '']"
                  :title="LED_CONFIG[row.led].title"
                />
                <span
                  class="grid h-5 w-5 place-items-center rounded-[5px] text-faint opacity-0 hover:bg-surface hover:text-err group-hover/row:opacity-100 transition-opacity"
                  aria-label="delete conversation"
                  @click.stop="emit('delete', row.id)"
                >
                  <Icon name="trash" class="h-3 w-3" />
                </span>
              </div>
            </button>
          </li>
        </ul>
      </section>
    </nav>

    <!-- Small collapsible OAuth quota & rate-limits widget (only visible if OAuth accounts exist) -->
    <OAuthUsageSidebarWidget />

    <!--
      The identity row (`docs/12` §2.2). Who this window is running as, and how much is in it:
      the reference's name-over-plan row, filled with the two things we can say honestly — the
      OS user and the size of the catalogue. The gear is the reference's own control in the
      same row, and it is here because this is where a user looks for it; `⌘,` does the same
      thing from anywhere.
    -->
    <div class="mt-auto flex items-center gap-2.5 border-t border-line/40 px-3 py-2.5">
      <span
        class="grid h-7 w-7 shrink-0 place-items-center rounded-full border border-accent/30 bg-accent/15 text-[12px] font-medium text-accent"
        aria-hidden="true"
      >
        {{ userInitial }}
      </span>
      <div class="min-w-0 flex-1">
        <p class="truncate text-[12.5px] font-medium text-fg/90">{{ props.identity.user ?? "local" }}</p>
        <p class="truncate text-[11px] text-faint">
          {{ props.sessions }} {{ props.sessions === 1 ? "session" : "sessions" }} · v{{
            props.identity.version
          }}
        </p>
      </div>
      <button
        class="grid h-7 w-7 shrink-0 place-items-center rounded-[6px] text-faint transition-colors hover:bg-raised hover:text-fg"
        aria-label="settings"
        data-action="settings"
        type="button"
        @click="emit('settings')"
      >
        <Icon name="gear" class="h-4 w-4" />
      </button>
    </div>

    <ProjectMenu
      v-if="projectMenu"
      :group="projectMenu.group"
      :at="projectMenu.at"
      @close="projectMenu = null"
      @open="emit('open', $event)"
      @delete="emit('deleteProject', $event)"
    />

    <!--
      Hover Preview Card:
      Floating context card showing thread title, prompt snippet, lifecycle status,
      turn count, and last interaction timestamp without cluttering the compact row.
    -->
    <Teleport to="body">
      <Transition
        enter-active-class="transition duration-150 ease-out"
        enter-from-class="opacity-0 translate-x-[-4px] scale-[0.98]"
        enter-to-class="opacity-100 translate-x-0 scale-100"
        leave-active-class="transition duration-100 ease-in"
        leave-from-class="opacity-100 translate-x-0 scale-100"
        leave-to-class="opacity-0 translate-x-[-4px] scale-[0.98]"
      >
        <div
          v-if="hoveredRow"
          class="fixed z-50 pointer-events-none w-72 max-w-[calc(100vw-280px)] rounded-xl border border-line/70 bg-surface/95 p-3 text-left shadow-2xl backdrop-blur-md"
          :style="{ top: `${hoverPosition.top}px`, left: `${hoverPosition.left}px` }"
        >
          <!-- Card header: icon + full title -->
          <div class="flex items-start gap-2">
            <Icon name="message" class="h-3.5 w-3.5 shrink-0 text-accent/80 mt-0.5" />
            <div class="min-w-0 flex-1">
              <h4 class="text-[12.5px] font-medium leading-snug text-fg line-clamp-2">
                {{ hoveredRow.title }}
              </h4>
              <p
                v-if="hoveredRow.firstMessage && hoveredRow.firstMessage !== hoveredRow.title"
                class="mt-1 text-[11px] leading-relaxed text-faint line-clamp-2 italic"
              >
                "{{ hoveredRow.firstMessage }}"
              </p>
            </div>
          </div>

          <!-- Card separator -->
          <div class="my-2.5 h-px bg-line/40" />

          <!-- Metadata list: status, turns, timestamp, workspace -->
          <div class="flex flex-col gap-1.5 text-[11px]">
            <!-- Live status / session lifecycle -->
            <div class="flex items-center justify-between gap-2">
              <div class="flex items-center gap-1.5 text-dim">
                <span
                  v-if="hoveredRow.led"
                  class="size-2 rounded-full shrink-0"
                  :class="LED_CONFIG[hoveredRow.led].color"
                />
                <Icon
                  v-else
                  :name="hoveredRow.live ? 'activity' : 'layers'"
                  class="h-3 w-3 shrink-0 text-faint"
                />
                <span>
                  {{
                    hoveredRow.led
                      ? LED_CONFIG[hoveredRow.led].title
                      : (hoveredRow.live ? 'Active session' : (hoveredRow.suspended ? 'Suspended' : 'Completed session'))
                  }}
                </span>
              </div>
              <span class="rounded bg-raised/80 px-1.5 py-0.5 font-mono text-[10px] text-faint border border-line/40 shrink-0">
                {{ hoveredRow.messageCount }} {{ hoveredRow.messageCount === 1 ? 'turn' : 'turns' }}
              </span>
            </div>

            <!-- Last updated timestamp -->
            <div class="flex items-center gap-1.5 text-dim">
              <Icon name="clock" class="h-3 w-3 shrink-0 text-faint" />
              <span class="text-faint">{{ formatFriendlyTime(hoveredRow.modifiedAt) }}</span>
              <span v-if="hoveredRow.modifiedAt" class="font-mono text-[10px] text-faint/70">
                · {{ formatFullDate(hoveredRow.modifiedAt) }}
              </span>
            </div>

            <!-- Project path segment -->
            <div v-if="hoveredRow.project" class="flex items-center gap-1.5 text-dim truncate">
              <Icon name="folder" class="h-3 w-3 shrink-0 text-faint" />
              <span class="truncate font-mono text-[10.5px] text-faint">
                {{ projectName(hoveredRow.project) }}
              </span>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>
  </aside>
</template>
