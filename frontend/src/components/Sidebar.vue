<script setup lang="ts">
/**
 * The left sidebar (`docs/12` §2): navigation, not content.
 *
 * Rows are grouped by project because that is how the engine files sessions (one directory
 * per working directory), and the group's `+` starts a thread where its sessions live. The
 * grouping and ordering are computed in `lib/threads.ts` — this is the rendering, plus the
 * one piece of state the sidebar owns: which group is collapsed.
 */
import { computed, ref, watch } from "vue";

import { pickDirectory } from "../bridge";
import type { ProjectGroup, ThreadRow } from "../lib/threads";
import ProjectMenu from "./ProjectMenu.vue";
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
  collapse: [];
}>();

const collapsed = ref(new Set<string>());

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
  if (path !== "") emit("open", path);
}

async function chooseDirectory(): Promise<void> {
  try {
    const path = await pickDirectory("Open Project");
    if (path) {
      adding.value = false;
      typed.value = "";
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
  emit("context", row.id, { x: event.clientX, y: event.clientY });
}

const projectMenu = ref<{ group: ProjectGroup; at: { x: number; y: number } } | null>(null);

function onProjectContext(event: MouseEvent, group: ProjectGroup): void {
  event.preventDefault();
  projectMenu.value = { group, at: { x: event.clientX, y: event.clientY } };
}

function toggle(path: string): void {
  const next = new Set(collapsed.value);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  collapsed.value = next;
}

function isOpen(path: string): boolean {
  return !collapsed.value.has(path);
}

/** Threads that need the user, so their group can say so while collapsed. */
function attention(rows: ThreadRow[]): number {
  return rows.filter((row) => row.dot === "attention" || row.unread).length;
}

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
          title="collapse sidebar"
          aria-label="collapse sidebar"
          type="button"
          @click="emit('collapse')"
        >
          <Icon name="panels" class="h-3.5 w-3.5" />
        </button>
        <span class="text-[12.5px] font-semibold tracking-tight text-fg/80">OhMyPi</span>
      </div>
      <button
        :disabled="props.busy || props.workspace === null"
        class="grid h-6 w-6 place-items-center rounded-[6px] text-dim transition-colors hover:bg-raised hover:text-fg disabled:opacity-30 disabled:hover:bg-transparent"
        :title="props.workspace ? `new thread in ${props.workspace}` : 'add a project first'"
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
        title="search titles, messages and tool activity across every thread"
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
        :title="props.workspace ?? 'add a project first'"
        data-row="new-thread"
        @click="props.workspace && emit('open', props.workspace)"
      >
        <Icon name="plus" class="h-4 w-4 text-faint group-hover:text-dim" />
        New thread
      </button>

      <button
        class="group flex items-center gap-2.5 rounded-[6px] px-2.5 py-2 text-left text-[13px] text-dim hover:bg-raised hover:text-fg"
        title="subagents running across every thread"
        data-row="agents"
        @click="emit('agents')"
      >
        <Icon name="sparkle" class="h-4 w-4 text-faint group-hover:text-dim" />
        Active agents
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
        title="open a project folder"
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
          title="browse folders…"
          type="button"
          @mousedown.prevent="chooseDirectory"
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

      <section v-for="group in props.groups" :key="group.path || group.name" class="mb-0.5">
        <div
          class="group flex items-center gap-1 rounded-[6px] px-1.5 py-1.5 hover:bg-raised"
          @contextmenu.prevent="onProjectContext($event, group)"
        >
          <button
            class="flex min-w-0 flex-1 items-center gap-1.5 text-left"
            :title="group.path"
            @click="toggle(group.path)"
          >
            <Icon
              :name="isOpen(group.path) ? 'chevron-down' : 'chevron-right'"
              class="h-3.5 w-3.5 shrink-0 text-faint"
            />
            <span class="truncate text-[12.5px] text-dim">{{ group.name }}</span>
          </button>
          <span
            v-if="!isOpen(group.path) && attention(group.threads) > 0"
            class="shrink-0 rounded-full bg-warn/15 px-1.5 font-mono text-[10px] text-warn"
          >
            {{ attention(group.threads) }}
          </span>
          <button
            :disabled="props.busy || group.path === ''"
            class="grid h-5 w-5 shrink-0 place-items-center rounded-[5px] text-faint opacity-0 hover:bg-surface hover:text-fg group-hover:opacity-100 disabled:opacity-0"
            title="new thread here"
            @click.stop="emit('open', group.path)"
          >
            <Icon name="plus" class="h-3.5 w-3.5" />
          </button>
          <button
            :disabled="props.busy"
            class="grid h-5 w-5 shrink-0 place-items-center rounded-[5px] text-faint opacity-0 hover:bg-surface hover:text-err group-hover:opacity-100 disabled:opacity-0"
            title="delete project and its sessions"
            @click.stop="emit('deleteProject', group)"
          >
            <Icon name="trash" class="h-3.5 w-3.5" />
          </button>
        </div>

        <ul v-if="isOpen(group.path)" class="flex flex-col gap-px">
          <li v-for="row in group.threads" :key="row.id">
            <button
              class="group/row flex w-full items-center gap-2 rounded-[6px] py-1.5 pr-1.5 text-left"
              :class="
                row.id === props.activeId
                  ? 'bg-selected'
                  : 'hover:bg-raised'
              "
              :style="{ paddingLeft: `${row.depth === 0 ? 0.75 : 1.5}rem` }"
              :title="`${row.title}${row.note ? ` · ${row.note}` : ''}${row.suspended ? ' · suspended' : ''}`"
              @click="emit('select', row.id)"
              @contextmenu="onContext($event, row)"
            >
              <!--
                One dot per thread (`docs/12` §2.2), driven by the sidecar: hollow for no
                process, pulsing while a turn streams, amber when the agent is waiting on an
                answer, red after a failed turn. The ring is app-owned — the engine has no
                read/unread concept.
              -->
              <span
                class="size-2 shrink-0 rounded-full transition-all"
                :class="{
                  'border border-line-strong': row.dot === 'cold',
                  'bg-faint/60': row.dot === 'idle',
                  'animate-pulse bg-accent streaming-dot': row.dot === 'streaming',
                  'bg-warn': row.dot === 'attention',
                  'bg-err': row.dot === 'error',
                }"
              />
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
                :class="row.unread ? 'font-medium text-fg' : 'text-dim group-hover/row:text-fg/90'"
              >
                {{ row.title }}
              </span>
              <!--
                One slot, and "suspended" takes it: the row is one line, and "where is my
                process" is the fresher question than "how did the last turn go" — which the
                tooltip still carries. A hole in the dot alone is too quiet to say the app
                released a 200 MB process on purpose.
              -->
              <span
                v-if="row.suspended"
                class="shrink-0 rounded bg-raised/80 px-1.5 py-0.5 font-mono text-[9.5px] text-faint"
                data-suspended
              >
                suspended
              </span>
              <span v-else-if="row.note" class="shrink-0 rounded bg-raised/80 px-1.5 py-0.5 font-mono text-[9.5px] text-faint">
                {{ row.note }}
              </span>
              <span
                v-if="row.unread"
                class="size-1.5 shrink-0 rounded-full bg-accent-bright"
                title="finished while you were elsewhere"
              />
              <Icon
                v-if="row.pinned"
                name="pin"
                class="h-3 w-3 shrink-0 text-faint"
                title="pinned"
              />
              <span
                class="grid h-5 w-5 shrink-0 place-items-center rounded-[5px] text-faint opacity-0 hover:bg-surface hover:text-err group-hover/row:opacity-100"
                title="delete conversation"
                @click.stop="emit('delete', row.id)"
              >
                <Icon name="trash" class="h-3 w-3" />
              </span>
            </button>
          </li>
        </ul>
      </section>
    </nav>

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
        :title="`settings (${isMac ? '⌘,' : 'Ctrl+,'})`"
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
  </aside>
</template>
