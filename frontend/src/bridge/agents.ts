import { invoke, listenPayload, type UnlistenFn } from "./core";
import type { RowSnapshot, ThreadEvent } from "./sessions";

// ---------------------------------------------------------------- agents (`docs/12` §9)

/**
 * One subagent, as the engine last described it.
 *
 * Field names and units are the engine's: `cost` is USD, `durationMs` and `lastUpdateMs` are
 * milliseconds, `tokens` excludes cache reads. Nothing here converts, so a value the panel
 * shows is a value the engine reported.
 */
/** One job a delivery message reports on. */
export interface JobDeliverySnapshot {
  /** The same string the card that started it carries in `details.async.jobId`. */
  jobId: string;
  /** `bash` | `eval` | `task`. */
  kind: string;
  durationMs: number;
  label: string | null;
}

export interface AgentSnapshot {
  /** The engine's id for the spawn — also the file stem of its transcript. */
  id: string;
  /** Dispatch order within one `task` call. */
  index: number;
  agent: string;
  agentSource: string | null;
  /** `pending` | `running` | `completed` | `failed` | `aborted`, or null when nothing said. */
  status: string | null;
  description: string | null;
  task: string | null;
  assignment: string | null;
  sessionFile: string | null;
  /** The `task` tool call that spawned it: the link back into the conversation. */
  parentToolCallId: string | null;
  detached: boolean;
  /** When this agent was first spawned or discovered. */
  createdAtMs: number;
  /** When *this host* last heard about the row. */
  lastUpdateMs: number;
  /**
   * Whether the engine's last `get_subagents` answer contained it.
   *
   * False on a running status means the engine stopped listing it without reporting how it
   * ended. The panel says that rather than picking a status.
   */
  listed: boolean;
  progress: AgentProgressSnapshot | null;
}

export interface AgentProgressSnapshot {
  lastIntent: string | null;
  currentTool: string | null;
  currentToolArgs: string | null;
  toolCount: number;
  requests: number;
  tokens: number;
  contextTokens: number | null;
  contextWindow: number | null;
  cost: number;
  durationMs: number;
  resolvedModel: string | null;
  resolvedThinkingLevel: string | null;
  advisor: boolean;
  /** An auto-retry in flight: why an agent can look stalled without having failed. */
  retry: { attempt: number; maxAttempts: number; delayMs: number; errorMessage: string } | null;
  /** The retry that ended the run, when the engine gave up. */
  retryFailure: { attempt: number; errorMessage: string } | null;
}

/** Every agent the app can account for, for one thread. */
export interface ThreadAgents {
  thread: string;
  agents: AgentSnapshot[];
  /** Why this thread has no roster, when the engine refused the subscription at open. */
  error: string | null;
}

/** A subagent transcript found on disk, after its engine forgot about it. */
export interface ParkedAgent {
  id: string;
  path: string;
  bytes: number;
  modifiedMs: number;
  cwd: string;
  /** The `parentSession` its header recorded: the parent's own file path. */
  parent: string | null;
  advisor: boolean;
  advisorSlug: string | null;
}

/** One page of a subagent's transcript. */
export interface AgentTranscript {
  agent: string;
  path: string;
  nextByte: number;
  /** The file shrank under the cursor, so this page **replaces** what was shown. */
  reset: boolean;
  /** `engine` when the engine served it, `file` when it was read from disk instead. */
  source: string;
  rows: RowSnapshot[];
}

/** A file modified by a subagent during a session. */
export interface SubagentFileChange {
  agentId: string;
  agentName: string;
  path: string;
  kind: "written" | "edited";
  diff: string | null;
  linesAdded: number;
  linesRemoved: number;
  touches: number;
}

/** One supervised process from `omp ps --json`. */
export interface BrokerDaemon {
  /** The name `omp ps stop <name>` takes. */
  name: string;
  /** `running` | `exited` | …, verbatim. */
  state: string;
  /** The session that asked for it, when one did. */
  owner: string | null;
  command: string;
  cwd: string;
  supervised: boolean;
  persist: boolean;
  detached: boolean;
  restartCount: number;
  exitCode: number | null;
  outputBytes: number;
  startedAtMs: number;
  exitedAtMs: number | null;
}

/** One broker scope: a project (or the shared global one) its daemons run under. */
export interface BrokerScope {
  kind: string;
  projectDir: string;
  runtimeDir: string;
  brokerPid: number | null;
  daemons: BrokerDaemon[];
}

/** Every agent the app can account for, one entry per thread. */
export async function agents(): Promise<ThreadAgents[]> {
  return invoke<ThreadAgents[]>("agents");
}

/**
 * The subagent transcripts this session's file shows on disk.
 *
 * The engine's registry is live-only, so this is where everything that already finished is
 * found — including for a session this window has never opened.
 */
export async function parkedAgents(thread: string): Promise<ParkedAgent[]> {
  return invoke<ParkedAgent[]>("parked_agents", { thread });
}

/**
 * One page of a subagent's transcript, by byte cursor.
 *
 * Pass the previous page's `nextByte` to get the delta. A page with `reset` set is the
 * file's beginning, not a continuation, and must replace what the pane was showing.
 */
export async function agentMessages(
  thread: string,
  agent: string,
  fromByte?: number,
): Promise<AgentTranscript> {
  return invoke<AgentTranscript>("agent_messages", { thread, agent, fromByte });
}

/**
 * Files touched by subagents for this thread.
 *
 * If `agentId` is supplied, scans that specific subagent's transcript.
 * Otherwise, scans all subagent transcripts in the session's artifacts directory.
 */
const inFlightSubagentFileChanges = new Map<string, Promise<SubagentFileChange[]>>();

export async function subagentFileChanges(
  thread: string,
  agentId?: string | null,
): Promise<SubagentFileChange[]> {
  const key = `${thread}::${agentId ?? ""}`;
  const existing = inFlightSubagentFileChanges.get(key);
  if (existing) {
    return existing;
  }

  const promise = invoke<SubagentFileChange[]>("subagent_file_changes", {
    thread,
    agentId: agentId ?? null,
  }).finally(() => {
    inFlightSubagentFileChanges.delete(key);
  });

  inFlightSubagentFileChanges.set(key, promise);
  return promise;
}

/** Every broker-owned process the engine is supervising. */
export async function brokerProcesses(thread?: string | null): Promise<BrokerScope[]> {
  return invoke<BrokerScope[]>("broker_processes", { thread: thread ?? null });
}

/** Stop one broker-owned process. `omp ps` is the only supported path. */
export async function stopBrokerProcess(name: string, thread?: string | null): Promise<string> {
  return invoke<string>("stop_broker_process", { name, thread: thread ?? null });
}

/**
 * Subscribe to one thread's agent roster.
 *
 * Tagged with the thread, because a roster is exactly what changes when one of its subagents
 * starts, reports or settles — and the panel shows every thread's agents side by side.
 */
export async function onAgents(
  handler: (event: ThreadEvent<AgentSnapshot[]>) => void,
): Promise<UnlistenFn> {
  return listenPayload<ThreadEvent<AgentSnapshot[]>>("session-agents", handler);
}
