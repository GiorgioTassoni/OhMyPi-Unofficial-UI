<script setup lang="ts">
/**
 * The agents panel (`docs/12` §9): a concise view of work still in flight,
 * plus the processes the engine's broker supervises.
 *
 * Global rather than scoped to one thread, because a subagent is work that runs *beside* the
 * conversation: a reader looking for "what is running right now" is asking about the app, not
 * about the thread on screen.
 *
 * The roster also remembers settled agents, but this window deliberately excludes them.
 * Jobs are derived from their starting cards and delivery messages; only undelivered jobs
 * belong in the live list.
 */
import { computed, nextTick, onMounted, ref, watch } from "vue";

import {
  brokerProcesses,
  stopBrokerProcess,
  type BrokerScope,
  type RowSnapshot,
  type ThreadAgents,
} from "../bridge";
import {
  activeCount,
  agentInFlight,
  context,
  cost,
  entries,
  jobs,
  label,
  modelFor,
  stats,
  statusLabel,
  subtitle,
  taskFor,
  tokens,
  tone,
  type AgentEntry,
  type AgentTone,
} from "../lib/agents";
import Modal from "./Modal.vue";

const props = defineProps<{
  /** Every live thread's roster, as the host last published it. */
  rosters: ThreadAgents[];
  /** Thread titles, for a heading a reader recognises. */
  titles: Record<string, string>;
  /** The window's active thread, which the panel opens on. */
  activeId: string | null;
  /**
   * The threads whose sidecar is still running.
   *
   * A closed thread's roster is what it *had*: nothing is running in a session whose engine
   * has gone, so its rows are marked as settled and counted as nothing. Without this the panel
   * claims work that no longer exists — which is exactly the disagreement a browser check
   * found between this panel's count and the sidebar's.
   */
  live: string[];
  /** The active thread's conversation, for the task text and the jump back. */
  rows: RowSnapshot[];
  /** A spawn notice can open this panel directly on its agent. */
  focus: { thread: string; id: string } | null;
}>();

const emit = defineEmits<{
  /** Reveal a conversation row: the spawning `task` call, or a job's card. */
  reveal: [thread: string, index: number];
  openAgent: [thread: string, id: string];
  close: [];
}>();

type Tab = "agents" | "jobs";

const tab = ref<Tab>("agents");
const failure = ref<string | null>(null);

/** The thread whose agent summaries are in view. */
const scope = ref<string | null>(props.focus?.thread ?? props.activeId ?? props.rosters[0]?.thread ?? null);

const agentList = ref<HTMLElement | null>(null);

const scopes = ref<BrokerScope[]>([]);
/** OMP retains exited daemons in `ps`; this panel is for processes still active. */
const activeScopes = computed(() => scopes.value
  .map((entry) => ({
    ...entry,
    daemons: entry.daemons.filter((daemon) => daemon.state !== "exited" && daemon.state !== "failed"),
  }))
  .filter((entry) => entry.daemons.length > 0));
const jobNotice = ref<string | null>(null);

/** Only threads with agents still in flight get a section. */
const listed = computed(() => {
  const threads = new Set<string>();
  for (const roster of props.rosters) {
    if (props.live.includes(roster.thread) && roster.agents.some(agentInFlight)) threads.add(roster.thread);
  }
  return [...threads];
});

const active = computed(() => activeCount(props.rosters, props.live));

/** Whether a row belongs to a thread that still has an engine. */
function running(thread: string): boolean {
  return props.live.includes(thread);
}

/** The tone a row is drawn with, given whether its thread is still live. */
function rowTone(entry: AgentEntry): AgentTone {
  if (entry.agent === null) return "unknown";
  return running(entry.thread) ? tone(entry.agent) : "gone";
}

/** The words beside the dot, with the closed-thread case made explicit. */
function rowStatus(entry: AgentEntry): string {
  if (entry.agent === null) return "";
  return running(entry.thread) ? statusLabel(entry.agent) : "this session's engine is gone";
}

/** The rows of one thread: only agents the running engine still lists. */
function rowsFor(thread: string): AgentEntry[] {
  if (!running(thread)) return [];
  const roster = props.rosters.find((entry) => entry.thread === thread) ?? null;
  return entries(thread, roster ? { ...roster, agents: roster.agents.filter(agentInFlight) } : null, []);
}

function model(entry: AgentEntry): string | null {
  return entry.agent?.progress?.resolvedModel ?? modelFor(props.rows, entry.id);
}

function assignment(entry: AgentEntry): string | null {
  return entry.agent?.assignment ?? taskFor(props.rows, entry.id);
}

/** The conversation's background jobs, from the cards and the deliveries. */
const derived = computed(() => jobs(props.rows));
const runningJobs = computed(() => props.activeId !== null && props.live.includes(props.activeId)
  ? derived.value.filter((job) => !job.delivered) : []);

/** The dot's colour, per tone. */
function toneClass(which: AgentTone): string {
  switch (which) {
    case "running":
      return "bg-accent animate-pulse";
    case "queued":
      return "bg-faint";
    case "done":
      return "bg-ok";
    case "failed":
      return "bg-err";
    case "gone":
      return "bg-warn";
    default:
      return "bg-faint";
  }
}

/** The status word's tint, in the same key as its dot. */
function statusClass(which: AgentTone): string {
  switch (which) {
    case "running":
      return "text-accent";
    case "done":
      return "text-ok";
    case "failed":
      return "text-err";
    default:
      return "text-faint";
  }
}

async function loadScopes(thread: string | null): Promise<void> {
  try {
    scopes.value = await brokerProcesses(thread);
  } catch (error) {
    failure.value = String(error);
  }
}

const requestedEntry = computed(() => {
  const target = props.focus;
  return target ? rowsFor(target.thread).find((entry) => entry.id === target.id) ?? null : null;
});
let focusRevealed = false;

async function revealFocusedAgent(): Promise<void> {
  if (focusRevealed) return;
  const target = requestedEntry.value;
  if (!target) return;
  await nextTick();
  const row = [...(agentList.value?.querySelectorAll<HTMLElement>("[data-agent-id]") ?? [])]
    .find((element) => element.dataset.agentId === target.id);
  if (row) {
    row.scrollIntoView({ block: "nearest" });
    focusRevealed = true;
  }
}

watch(requestedEntry, () => void revealFocusedAgent(), { flush: "post" });

onMounted(async () => {
  await loadScopes(scope.value);
  await revealFocusedAgent();
});

/** The card a job started from, for the jump into the conversation. */
function jobRow(toolCallId: string): number {
  return props.rows.findIndex((row) => row.tool?.toolCallId === toolCallId);
}

async function stop(name: string): Promise<void> {
  jobNotice.value = null;
  try {
    jobNotice.value = (await stopBrokerProcess(name, scope.value)) || `stopped ${name}`;
  } catch (error) {
    failure.value = String(error);
  }

  await loadScopes(scope.value);
}

function title(thread: string): string {
  return props.titles[thread] ?? thread;
}
</script>

<template>
  <Modal :title="`Active agents ${active}`" @close="emit('close')">
    <div class="flex min-h-[22rem] flex-col gap-3">
      <p
        v-if="failure"
        class="rounded-[6px] bg-err/10 px-2.5 py-2 text-[12px] text-err"
      >
        {{ failure }}
      </p>

      <div class="flex items-center gap-2">
        <button
          v-for="option in (['agents', 'jobs'] as const)"
          :key="option"
          type="button"
          class="rounded-[6px] px-2.5 py-1 text-[12.5px]"
          :class="tab === option ? 'bg-raised text-fg' : 'text-dim hover:text-fg'"
          @click="tab = option"
        >
          {{ option === "agents" ? `agents ${active}` : `jobs ${runningJobs.length}` }}
        </button>
      </div>

      <div v-if="tab === 'agents'" ref="agentList" class="flex min-h-0 flex-1 flex-col gap-1 overflow-auto">
          <p v-if="listed.length === 0" class="px-1 py-3 text-[12px] leading-relaxed text-faint">
            No agents running.
          </p>

          <section v-for="thread in listed" :key="thread" class="flex flex-col gap-px">
            <button
              type="button"
              class="flex w-full items-center gap-2 rounded-[6px] px-2.5 py-1.5 text-left hover:bg-raised"
              @click="scope = thread"
            >
              <span
                class="min-w-0 flex-1 truncate text-[12.5px]"
                :class="thread === scope ? 'text-fg' : 'text-dim'"
              >
                {{ title(thread) }}
              </span>
              <span class="shrink-0 font-mono text-[10.5px] text-faint">
                {{ rowsFor(thread).length }}
              </span>
            </button>

            <button
              v-for="entry in rowsFor(thread)"
              :key="entry.key"
              type="button"
              :data-agent-id="entry.id"
              class="flex w-full min-w-0 items-start gap-2 rounded-[6px] px-2 py-2 text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent"
              :class="focus?.thread === thread && focus.id === entry.id ? 'bg-accent/10 ring-1 ring-accent/50' : 'hover:bg-raised/45'"
              :style="{ paddingLeft: `${0.5 + entry.depth * 0.75}rem` }"
              :disabled="!entry.agent"
              :title="entry.agent ? `Open ${label(entry)} agent activity` : undefined"
              @click="entry.agent && emit('openAgent', thread, entry.id)"
            >
              <span
                class="mt-1 size-1.5 shrink-0 rounded-full"
                :class="entry.agent ? toneClass(rowTone(entry)) : 'bg-faint'"
              />
              <span class="flex min-w-0 flex-1 flex-col gap-0.5">
                <span class="flex min-w-0 items-baseline gap-2">
                  <span class="shrink-0 text-[12.5px] text-fg">{{ label(entry) }}</span>
                  <span v-if="entry.agent" class="min-w-0 truncate font-mono text-[10.5px] text-faint" :title="entry.id">{{ entry.id }}</span>
                  <span
                    v-if="entry.agent"
                    class="ml-auto shrink-0 text-[11.5px]"
                    :class="statusClass(rowTone(entry))"
                  >
                    {{ rowStatus(entry) }}
                  </span>
                </span>
                <span v-if="assignment(entry)" class="block truncate text-[11.5px] text-dim">
                  {{ assignment(entry) }}
                </span>
                <span v-if="subtitle(entry)" class="block truncate text-[11.5px] text-dim">
                  {{ subtitle(entry) }}
                </span>
                <span class="min-w-0 truncate font-mono text-[10.5px] text-dim" :title="model(entry) ?? 'Model not reported'">
                  Model: <span :class="model(entry) ? 'text-fg' : 'text-faint'">{{ model(entry) ?? 'not reported' }}</span>
                </span>
                <span v-if="stats(entry)" class="block font-mono text-[10.5px] text-faint">
                  {{ stats(entry)!.tools }} tools · {{ stats(entry)!.requests }} req ·
                  {{ tokens(stats(entry)!.tokens) }} tokens · {{ cost(stats(entry)!.cost) }}
                  <template v-if="context(entry.agent!)"> · {{ context(entry.agent!) }}</template>
                </span>
              </span>
            </button>
          </section>
      </div>

      <div v-else class="flex min-h-0 flex-1 flex-col gap-3 overflow-auto">
        <p class="px-1 text-[11.5px] leading-relaxed text-faint">
          The engine has no command that lists or cancels a background job — `hub` is a tool the
          agent calls — so active rows come from calls that started them, minus jobs whose
          results have been delivered.
        </p>

        <p v-if="jobNotice" class="rounded-[6px] bg-raised px-2.5 py-2 text-[12px] text-dim">
          {{ jobNotice }}
        </p>

        <section v-if="runningJobs.length > 0" class="flex flex-col gap-1">
          <h3 class="px-1 text-[10.5px] font-medium uppercase tracking-[0.09em] text-faint">
            background jobs in this conversation
          </h3>
          <button
            v-for="job in runningJobs"
            :key="job.id"
            type="button"
            class="flex w-full items-center gap-2 rounded-[6px] px-2 py-1.5 text-left hover:bg-raised"
            @click="jobRow(job.toolCallId) >= 0 && emit('reveal', props.activeId ?? '', jobRow(job.toolCallId))"
          >
            <span
              class="size-1.5 shrink-0 animate-pulse rounded-full bg-accent"
            />
            <span class="min-w-0 flex-1">
              <span class="flex items-baseline gap-2">
                <span class="font-mono text-[12.5px] text-fg">{{ job.id }}</span>
                <span class="shrink-0 text-[11.5px] text-dim">
                  {{ job.kind }} · running
                </span>
              </span>
              <span v-if="job.detail" class="mt-0.5 block truncate font-mono text-[10.5px] text-faint">
                {{ job.detail }}
              </span>
            </span>
          </button>
        </section>

        <section class="flex flex-col gap-1">
          <h3 class="px-1 text-[10.5px] font-medium uppercase tracking-[0.09em] text-faint">
            broker-owned processes (`omp ps`)
          </h3>
          <p v-if="activeScopes.length === 0" class="px-1 py-1 text-[11.5px] text-faint">
            no active broker-owned processes
          </p>
          <template v-for="entry in activeScopes" :key="entry.runtimeDir">
            <div
              v-for="daemon in entry.daemons"
              :key="`${entry.runtimeDir}-${daemon.name}`"
              class="flex items-center gap-2 rounded-[6px] px-2 py-1.5 hover:bg-raised"
            >
              <span
                class="size-1.5 shrink-0 rounded-full"
                :class="daemon.state === 'running' ? 'bg-ok' : 'bg-faint'"
              />
              <span class="min-w-0 flex-1">
                <span class="flex items-baseline gap-2">
                  <span class="font-mono text-[12.5px] text-fg">{{ daemon.name }}</span>
                  <span class="shrink-0 text-[11.5px] text-dim">
                    {{ daemon.state }}<template v-if="daemon.exitCode !== null"> ({{ daemon.exitCode }})</template>
                    <template v-if="daemon.supervised"> · supervised</template>
                    <template v-if="daemon.owner"> · started by {{ title(daemon.owner) }}</template>
                  </span>
                </span>
                <span class="mt-0.5 block truncate font-mono text-[10.5px] text-faint">
                  {{ daemon.command }}
                </span>
              </span>
              <button
                type="button"
                class="shrink-0 rounded-[6px] px-2 py-1 text-[12px] text-dim hover:bg-raised hover:text-err"
                :title="`omp ps stop ${daemon.name}`"
                @click="stop(daemon.name)"
              >
                stop
              </button>
            </div>
          </template>
        </section>
      </div>

    </div>
  </Modal>
</template>
