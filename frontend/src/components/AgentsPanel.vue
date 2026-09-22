<script setup lang="ts">
/**
 * The agents panel (`docs/12` §9): every subagent this window can account for, their
 * transcripts, and the processes the engine's broker supervises.
 *
 * Global rather than scoped to one thread, because a subagent is work that runs *beside* the
 * conversation: a reader looking for "what is running right now" is asking about the app, not
 * about the thread on screen.
 *
 * Three things about the engine shape this:
 *
 * - **The roster is live-only.** Its registry forgets a settled subagent immediately, so the
 *   threads' live rosters (pushed by the host) are joined with what is on disk — the
 *   transcript each subagent wrote — and an agent that finished before this window opened
 *   still gets a row.
 * - **A transcript is read by byte cursor.** The engine serves it while it owns the id;
 *   afterwards the host reads the same file itself. A page whose `reset` is set *replaces*
 *   what is shown, which is the engine's own rule for a file that shrank.
 * - **Jobs are derived, not listed.** The engine exposes no command to list or cancel one —
 *   `hub` is a tool the *agent* calls — so the rows here come from the cards that started
 *   them and the deliveries that finished them, and the panel says so instead of implying a
 *   control it does not have.
 */
import { computed, onMounted, onUnmounted, ref, watch } from "vue";

import {
  agentMessages,
  brokerProcesses,
  parkedAgents,
  stopBrokerProcess,
  type BrokerScope,
  type RowSnapshot,
  type ThreadAgents,
} from "../bridge";
import {
  activeCount,
  age,
  context,
  cost,
  duration,
  entries,
  jobs,
  label,
  spawnRow,
  stats,
  statusLabel,
  subtitle,
  taskFor,
  tokens,
  tone,
  type AgentEntry,
  type AgentTone,
} from "../lib/agents";
import ConversationRow from "./ConversationRow.vue";
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
}>();

const emit = defineEmits<{
  /** Reveal a conversation row: the spawning `task` call, or a job's card. */
  reveal: [thread: string, index: number];
  close: [];
}>();

type Tab = "agents" | "jobs";

const tab = ref<Tab>("agents");
const now = ref(Date.now());
const failure = ref<string | null>(null);

/** The thread whose detail pane is open. */
const scope = ref<string | null>(props.activeId ?? props.rosters[0]?.thread ?? null);

/** Every parked transcript for the threads the panel is looking at. */
const parked = ref<Record<string, ParkedAgentLike[]>>({});

/** The selected agent, and its transcript. */
const selected = ref<AgentEntry | null>(null);
const transcript = ref<RowSnapshot[]>([]);
const transcriptNote = ref<string | null>(null);
let cursor = 0;

const scopes = ref<BrokerScope[]>([]);
const jobNotice = ref<string | null>(null);

type ParkedAgentLike = Awaited<ReturnType<typeof parkedAgents>>[number];

/** The threads this panel lists: the ones with agents, and the one being looked at. */
const listed = computed(() => {
  const threads = new Set<string>();
  for (const roster of props.rosters) {
    if (roster.agents.length > 0 || roster.error !== null) threads.add(roster.thread);
  }
  if (scope.value !== null) threads.add(scope.value);

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

/** The rows of one thread: its live roster joined with the files on disk. */
function rowsFor(thread: string): AgentEntry[] {
  const roster = props.rosters.find((entry) => entry.thread === thread) ?? null;

  return entries(thread, roster, parked.value[thread] ?? []);
}

/** The conversation's background jobs, from the cards and the deliveries. */
const derived = computed(() => jobs(props.rows));

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

async function loadParked(thread: string): Promise<void> {
  try {
    parked.value = { ...parked.value, [thread]: await parkedAgents(thread) };
  } catch (error) {
    failure.value = String(error);
  }
}

async function loadScopes(thread: string | null): Promise<void> {
  try {
    scopes.value = await brokerProcesses(thread);
  } catch (error) {
    failure.value = String(error);
  }
}

/**
 * Read the selected agent's transcript, from the beginning or from the cursor.
 *
 * The engine is asked first and the host falls back to the file, so this says which of the
 * two answered: a page the engine served and a page read from a file it has forgotten are
 * different claims about the same conversation.
 */
async function readTranscript(entry: AgentEntry, from: number): Promise<void> {
  const page = await agentMessages(entry.thread, entry.id, from);

  if (page.reset) transcript.value = page.rows;
  else transcript.value = [...transcript.value, ...page.rows];

  cursor = page.nextByte;
  transcriptNote.value =
    page.source === "engine"
      ? `${page.rows.length} rows read from the engine (through byte ${page.nextByte})`
      : `${page.rows.length} rows read from ${page.path} — the engine no longer serves this agent's transcript`;
}

async function select(entry: AgentEntry): Promise<void> {
  selected.value = entry;
  transcript.value = [];
  cursor = 0;
  transcriptNote.value = null;
  failure.value = null;

  try {
    await readTranscript(entry, 0);
  } catch (error) {
    failure.value = String(error);
  }
}

/** Follow a live agent: the engine writes as it goes, and the cursor is the delta. */
async function tick(): Promise<void> {
  now.value = Date.now();

  const entry = selected.value;
  if (entry === null || !entry.agent?.status || entry.agent.status === "completed") return;
  if (entry.agent.status === "failed" || entry.agent.status === "aborted") return;

  try {
    await readTranscript(entry, cursor);
  } catch {
    // A read that fails mid-run is not worth a banner: the next tick tries again, and the
    // transcript already on screen stays readable.
  }
}

let poll: ReturnType<typeof setInterval> | null = null;

onMounted(async () => {
  poll = setInterval(() => void tick(), 2000);
  if (scope.value !== null) {
    await Promise.all([loadParked(scope.value), loadScopes(scope.value)]);
  }
});

onUnmounted(() => {
  if (poll !== null) clearInterval(poll);
});

watch(scope, async (thread) => {
  if (thread === null || parked.value[thread] !== undefined) return;
  await loadParked(thread);
});

watch(
  () => props.rosters.map((roster) => roster.thread).join(","),
  async () => {
    if (scope.value === null || !props.rosters.some((roster) => roster.thread === scope.value)) {
      scope.value = props.activeId ?? props.rosters[0]?.thread ?? scope.value;
    }
    if (scope.value !== null) await loadParked(scope.value);
  },
);

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
          {{ option === "agents" ? `agents ${active}` : `jobs ${derived.filter((job) => !job.delivered).length}` }}
        </button>

        <span v-if="selected && tab === 'agents'" class="ml-auto font-mono text-[10.5px] text-faint">
          {{ selected.id }}
        </span>
      </div>

      <div v-if="tab === 'agents'" class="grid min-h-0 flex-1 grid-cols-2 gap-3">
        <div class="flex min-h-0 flex-col gap-1 overflow-auto">
          <p v-if="listed.length === 0" class="px-1 py-3 text-[12px] leading-relaxed text-faint">
            no thread is running an agent. This lists subagents the engine is reporting and the
            transcripts sessions have left behind.
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

            <p
              v-for="roster in rosters.filter((entry) => entry.thread === thread && entry.error)"
              :key="`${thread}-error`"
              class="px-2.5 py-1 text-[11.5px] text-warn"
            >
              this session's engine is not publishing a roster: {{ roster.error }}
            </p>

            <button
              v-for="entry in rowsFor(thread)"
              :key="entry.key"
              type="button"
              class="flex w-full items-start gap-2 rounded-[6px] px-2 py-1.5 text-left hover:bg-raised"
              :style="{ paddingLeft: `${0.5 + entry.depth * 0.75}rem` }"
              @click="select(entry)"
            >
              <span
                class="mt-1 size-1.5 shrink-0 rounded-full"
                :class="entry.agent ? toneClass(rowTone(entry)) : 'bg-faint'"
              />
              <span class="min-w-0 flex-1">
                <span class="flex items-baseline gap-2">
                  <span class="truncate text-[12.5px] text-fg">{{ label(entry) }}</span>
                  <span
                    v-if="entry.agent"
                    class="shrink-0 text-[11.5px]"
                    :class="statusClass(rowTone(entry))"
                  >
                    {{ rowStatus(entry) }}
                  </span>
                  <span
                    v-else-if="entry.parked?.advisor"
                    class="shrink-0 text-[11.5px] text-faint"
                  >
                    advisor
                  </span>
                  <span v-else class="shrink-0 font-mono text-[10.5px] text-faint">
                    {{ age(entry.parked?.modifiedMs ?? 0, now) }}
                  </span>
                </span>
                <span v-if="subtitle(entry)" class="mt-0.5 block truncate text-[11.5px] text-dim">
                  {{ subtitle(entry) }}
                </span>
                <span v-if="stats(entry)" class="mt-0.5 block font-mono text-[10.5px] text-faint">
                  {{ stats(entry)!.tools }} tools · {{ stats(entry)!.requests }} req ·
                  {{ tokens(stats(entry)!.tokens) }} tokens · {{ cost(stats(entry)!.cost) }}
                  <template v-if="context(entry.agent!)"> · {{ context(entry.agent!) }}</template>
                  <template v-if="entry.agent && running(entry.thread) && !entry.agent.listed">
                    · no longer listed
                  </template>
                </span>
              </span>
            </button>
          </section>
        </div>

        <div class="flex min-h-0 flex-col overflow-auto rounded-[6px] bg-raised/40 p-2.5">
          <p v-if="selected === null" class="px-1 py-2 text-[12.5px] text-faint">
            pick an agent to read its transcript
          </p>

          <template v-else>
            <div class="mb-2 flex flex-col gap-1 border-b border-line pb-2.5">
              <span class="font-mono text-[11.5px] text-fg">{{ selected.id }}</span>
              <span class="text-[12px] text-dim">
                {{ taskFor(props.rows, selected.id) ?? selected.agent?.task ?? "no task recorded for it" }}
              </span>
              <span v-if="transcriptNote" class="text-[10.5px] text-faint">
                {{ transcriptNote }}
              </span>
              <button
                v-if="selected.agent && spawnRow(props.rows, selected.agent) !== null"
                type="button"
                class="mt-1 self-start rounded-[6px] px-2 py-1 text-[12px] text-dim hover:bg-raised hover:text-fg"
                @click="emit('reveal', selected.thread, spawnRow(props.rows, selected.agent!)!)"
              >
                show the task call
              </button>
            </div>

            <div v-for="(row, index) in transcript" :key="index" class="mb-2">
              <ConversationRow :row="row" @failed="failure = $event" />
            </div>

            <p v-if="transcript.length === 0" class="px-1 py-2 text-[12.5px] text-faint">
              this transcript is empty
            </p>
          </template>
        </div>
      </div>

      <div v-else class="flex min-h-0 flex-1 flex-col gap-3 overflow-auto">
        <p class="px-1 text-[11.5px] leading-relaxed text-faint">
          The engine has no command that lists or cancels a background job — `hub` is a tool the
          agent calls — so these rows come from the calls that started them and the deliveries
          that finished them.
        </p>

        <p v-if="jobNotice" class="rounded-[6px] bg-raised px-2.5 py-2 text-[12px] text-dim">
          {{ jobNotice }}
        </p>

        <section v-if="derived.length > 0" class="flex flex-col gap-1">
          <h3 class="px-1 text-[10.5px] font-medium uppercase tracking-[0.09em] text-faint">
            background jobs in this conversation
          </h3>
          <button
            v-for="job in derived"
            :key="job.id"
            type="button"
            class="flex w-full items-center gap-2 rounded-[6px] px-2 py-1.5 text-left hover:bg-raised"
            @click="jobRow(job.toolCallId) >= 0 && emit('reveal', props.activeId ?? '', jobRow(job.toolCallId))"
          >
            <span
              class="size-1.5 shrink-0 rounded-full"
              :class="job.delivered ? 'bg-ok' : 'bg-accent animate-pulse'"
            />
            <span class="min-w-0 flex-1">
              <span class="flex items-baseline gap-2">
                <span class="font-mono text-[12.5px] text-fg">{{ job.id }}</span>
                <span class="shrink-0 text-[11.5px] text-dim">
                  {{ job.kind }} · {{ job.delivered ? `delivered in ${duration(job.durationMs ?? 0)}` : "running" }}
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
          <p v-if="scopes.every((entry) => entry.daemons.length === 0)" class="px-1 py-1 text-[11.5px] text-faint">
            no supervised process in this scope
          </p>
          <template v-for="entry in scopes" :key="entry.runtimeDir">
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

      <p v-if="selected && tab === 'agents'" class="px-1 text-[11.5px] leading-relaxed text-faint">
        A subagent's transcript can be read after it finishes; steering or cancelling one cannot,
        because the engine exposes no command for either — ask the agent that spawned it.
      </p>
    </div>
  </Modal>
</template>
