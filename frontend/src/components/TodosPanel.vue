<script setup lang="ts">
/**
 * The Todos tab (`docs/12` §8.1).
 *
 * The plan comes from `get_state.todoPhases` and nowhere else: there is no todo event in the
 * engine's stream (measured — only `todo_reminder` and `todo_auto_clear` exist), so the host
 * re-reads the state whenever the `todo` tool runs, and this renders whatever that returns.
 *
 * One write is offered — marking a task done or reopening it — because `set_todos` replaces the
 * whole list and the engine stores it **in memory only**: it persists nothing, and rebuilds the
 * plan from the transcript the next time the thread is resumed. The panel therefore renders the
 * engine's answer rather than keeping an optimistic copy of the request.
 */
import { computed, ref } from "vue";
import type { RowSnapshot, TodoPhaseSnapshot } from "../bridge";
import { setTodos } from "../bridge";
import { taskRow } from "../lib/panel";
import { phaseOpen, phaseProgress, todoLabel } from "../lib/todos";
import type { IconName } from "../lib/icons";
import Icon from "./ui/Icon.vue";

const props = defineProps<{
  /** The active thread, or null when nothing is open. */
  thread: string | null;
  phases: TodoPhaseSnapshot[];
  /** Whether a sidecar is running: without one there is nothing to write to. */
  live: boolean;
  /** The thread's transcript rows, for the jump back to where a task was written. */
  rows: RowSnapshot[];
}>();

const emit = defineEmits<{
  jump: [row: number];
  /** The plan was written: the shell re-reads the thread's state, which is where these come from. */
  written: [];
  failed: [message: string];
}>();

/** Phases the reader has opened or closed by hand, overriding the default. */
const overrides = ref<Record<string, boolean>>({});
const busy = ref(false);
const failure = ref<string | null>(null);

const phases = computed(() => props.phases);

/**
 * How one status is *drawn*, as opposed to how it is named.
 *
 * `lib/todos.ts` owns the vocabulary — the word in the tooltip, whether a phase opens — while
 * the glyph it also carries is the one a terminal can print. The window draws the same states
 * as icons, so the drawing lives here; a status this build has not seen keeps the neutral
 * empty circle and the pass-through word rather than a mark that claims to know it.
 */
interface Mark {
  /** The icon for this status, or null for the empty circle the unknown states share. */
  icon: IconName | null;
  tone: string;
  pulsing?: boolean;
}

const MARKS: Record<string, Mark> = {
  completed: { icon: "check", tone: "text-ok" },
  in_progress: { icon: null, tone: "text-warn", pulsing: true },
  blocked: { icon: "alert", tone: "text-warn" },
  abandoned: { icon: "close", tone: "text-faint" },
};

function mark(status: string): Mark {
  return MARKS[status] ?? { icon: null, tone: "text-faint" };
}

/** How a task's own words are drawn: the work in hand, finished quiet, or waiting. */
function tone(status: string): string {
  if (status === "completed") return "text-faint line-through";
  if (status === "in_progress") return "text-fg";
  return "text-dim";
}

function open(name: string, tasks: readonly { status: string }[]): boolean {
  return overrides.value[name] ?? phaseOpen(tasks);
}

function toggle(name: string, tasks: readonly { status: string }[]): void {
  overrides.value = { ...overrides.value, [name]: !open(name, tasks) };
}

function jump(task: string): void {
  const at = taskRow(props.rows, task);
  if (at === null) {
    failure.value = "that task is not in this conversation yet";
    return;
  }
  failure.value = null;
  emit("jump", at);
}

/**
 * Flip one task between done and pending, by sending the whole plan back.
 *
 * `set_todos` is a replace, not a patch, so this is the entire list with one status changed —
 * and what the panel shows afterwards is the engine's own answer, never this request.
 */
async function flip(phase: TodoPhaseSnapshot, content: string): Promise<void> {
  const thread = props.thread;
  if (thread === null) return;

  busy.value = true;
  failure.value = null;
  try {
    const next = phases.value.map((entry) => ({
      name: entry.name,
      tasks: entry.tasks.map((task) => ({
        content: task.content,
        // An unrecognised status is passed through unchanged rather than mapped onto one
        // this build happens to know.
        status:
          entry.name === phase.name && task.content === content
            ? task.status === "completed"
              ? "pending"
              : "completed"
            : task.status,
        blocker: task.blocker,
      })),
    }));

    await setTodos(thread, next);
    failure.value = null;
    emit("written");
  } catch (cause) {
    failure.value = cause instanceof Error ? cause.message : String(cause);
    emit("failed", failure.value);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col overflow-auto pb-2">
    <!--
      A stopped sidecar is a state of the plan, not a warning: the plan is still readable and
      the tick is what closes, so this sits with the list rather than above it in colour.
    -->
    <p v-if="!props.live" class="px-3.5 pb-1 pt-2 text-[11.5px] leading-relaxed text-faint">
      No session is running, so this is the plan as it was last read and a tick has nowhere to
      go. Resuming the thread rebuilds it from the conversation.
    </p>

    <div
      v-else-if="phases.length === 0"
      class="flex flex-1 flex-col items-center justify-center px-6 py-12 text-center"
    >
      <div class="mb-3 flex h-10 w-10 items-center justify-center rounded-xl border border-line/40 bg-raised/40 text-faint shadow-xs">
        <Icon name="list" class="h-5 w-5" />
      </div>
      <p class="text-[13px] font-medium text-main">No plan yet</p>
      <p class="mt-1 max-w-[220px] text-[12px] leading-relaxed text-faint">
        The agent writes tasks here as it plans work with its <span class="rounded bg-raised/80 px-1 py-0.5 font-mono text-[11px] text-muted">todo</span> tool.
      </p>
    </div>

    <ul v-if="phases.length > 0" class="flex flex-col">
      <li v-for="phase in phases" :key="phase.name" class="pt-2">
        <!-- The phase is the list's section label, and its own progress rides on the right. -->
        <button
          class="flex w-full items-center gap-1.5 px-3.5 py-1 text-left"
          :aria-expanded="open(phase.name, phase.tasks)"
          @click="toggle(phase.name, phase.tasks)"
        >
          <Icon
            :name="open(phase.name, phase.tasks) ? 'chevron-down' : 'chevron-right'"
            class="h-3.5 w-3.5 shrink-0 text-faint"
          />
          <span
            class="min-w-0 flex-1 truncate text-[10.5px] font-medium uppercase tracking-[0.09em] text-faint"
          >
            {{ phase.name }}
          </span>
          <span class="shrink-0 font-mono text-[10.5px] text-faint">
            {{ phaseProgress(phase.tasks).done }}/{{ phaseProgress(phase.tasks).total }}
          </span>
        </button>

        <ul v-if="open(phase.name, phase.tasks)" class="flex flex-col pt-0.5">
          <li v-for="task in phase.tasks" :key="task.content" class="px-2">
            <div class="flex items-start gap-2.5 rounded-[6px] px-1.5 py-1.5 hover:bg-raised">
              <!--
                The mark is the write (`set_todos` replaces the list, so a click sends the whole
                plan back). `sr-only` keeps its glyph as the button's label: the window draws a
                status as an icon, and the browser checks select this button by that glyph.
              -->
              <button
                class="mt-px grid size-4 shrink-0 place-items-center disabled:opacity-50"
                :class="mark(task.status).tone"
                :data-todo-mark="task.status"
                :title="`${todoLabel(task.status)} — mark ${task.status === 'completed' ? 'not done' : 'done'}`"
                :disabled="busy || !props.live"
                @click="flip(phase, task.content)"
              >
                <span
                  v-if="mark(task.status).pulsing"
                  class="size-2 rounded-full bg-warn animate-pulse"
                />
                <Icon
                  v-else-if="mark(task.status).icon !== null"
                  :name="mark(task.status).icon as IconName"
                  class="h-3.5 w-3.5"
                />
                <span v-else class="size-3 rounded-full border border-line-strong" />
                <span class="sr-only">{{ todoLabel(task.status) }}</span>
              </button>
              <button
                class="min-w-0 flex-1 text-left text-[12.5px] leading-relaxed"
                :class="tone(task.status)"
                title="show me where this was written"
                @click="jump(task.content)"
              >
                {{ task.content }}
              </button>
            </div>
            <p v-if="task.blocker" class="pl-8 pr-2 pb-1 text-[11px] text-warn">
              waiting on {{ task.blocker }}
            </p>
          </li>
        </ul>
      </li>
    </ul>

    <p v-if="failure" class="mt-auto px-3.5 py-2 text-[11.5px] text-err">{{ failure }}</p>
  </div>
</template>
