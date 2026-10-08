/**
 * The agents panel's model (`docs/12` §9), derived from what the host already holds.
 *
 * Three surfaces, and the difference between them is the point of the panel:
 *
 * - **The live roster** arrives from the engine — the frames while a subagent runs, and a
 *   poll that corrects them. Its rows are the engine's own words, including the status a
 *   settled agent keeps after the engine stops listing it.
 * - **The parked rows** are on disk: the transcript each subagent wrote, which is the only
 *   place an agent that finished before this window opened exists at all.
 * - **The transcript** is read by byte cursor, from the engine while it owns the id and from
 *   the file when it does not.
 *
 * Everything here is a pure function of those, plus the conversation rows the window already
 * has — which is where a subagent's *task* comes from: the `task` call that spawned it names
 * the agent id in its own details, so the panel joins them by string rather than inventing a
 * label the engine never sent.
 */

import type {
  AgentSnapshot,
  ParkedAgent,
  RowSnapshot,
  ThreadAgents,
  ToolSnapshot,
} from "../bridge";
import { parseObject } from "./toolView";

/** How a row is doing, in the terms the panel colours it. */
export type AgentTone = "running" | "queued" | "done" | "failed" | "gone" | "unknown";

/**
 * The tone for one agent.
 *
 * `gone` is its own case, not a failure: the engine stops listing a settled subagent, and if
 * no frame reported how it ended then the status it last carried is the only truth available.
 * Showing that as "failed" would invent an outcome, and as "completed" would invent a
 * success.
 */
export function tone(agent: AgentSnapshot): AgentTone {
  if (!agent.listed && (agent.status === "running" || agent.status === "pending")) return "gone";
  switch (agent.status) {
    case "running":
      return "running";
    case "pending":
      return "queued";
    case "completed":
      return "done";
    case "failed":
    case "aborted":
      return "failed";
    default:
      return "unknown";
  }
}

/** The words beside the dot. */
export function statusLabel(agent: AgentSnapshot): string {
  if (!agent.listed && (agent.status === "running" || agent.status === "pending")) {
    return agent.status === "pending"
      ? "queued, and no longer listed"
      : "settled — the engine stopped listing it without saying how it ended";
  }
  switch (agent.status) {
    case "running":
      return "running";
    case "pending":
      return "queued";
    case "completed":
      return "completed";
    case "failed":
      return "failed";
    case "aborted":
      return "aborted";
    default:
      return "no status was reported";
  }
}

/** Only agents the engine still lists as queued or running belong in the live UI. */
export function agentInFlight(agent: AgentSnapshot): boolean {
  return agent.listed && (agent.status === "running" || agent.status === "pending");
}

/**
 * How many agents are in flight, for the sidebar's `Active agents N`.
 *
 * Two filters, and both are the point:
 *
 * - a row the engine no longer lists is not running, whatever status it was last given — the
 *   panel shows it as settled, so a count that included it would contradict the list beside it;
 * - a thread whose sidecar has gone is not running anything either, so its last roster is
 *   history. `live` is the threads the host still holds, which is the only thing that knows.
 */
export function activeCount(threads: ThreadAgents[], live: string[] | null = null): number {
  return threads.reduce((total, entry) => {
    if (live !== null && !live.includes(entry.thread)) return total;

    return (
      total +
      entry.agents.filter(agentInFlight).length
    );
  }, 0);
}

/** A duration as a reader compares it: seconds, then minutes, then hours. */
export function age(sinceMs: number, now: number): string {
  const seconds = Math.max(0, Math.round((now - sinceMs) / 1000));
  if (seconds < 2) return "just now";
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.round(hours / 24)}d ago`;
}

/** A run's length, which is the engine's own measurement. */
export function duration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(1)}s`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes}m ${Math.round(seconds % 60)}s`;
}

/**
 * A cost, at a precision a reader can act on.
 *
 * The precision rises as the number gets smaller, because that is where it carries
 * information: a subagent that cost $0.0042 shown as `$0.00` reads as free, and one that cost
 * $0.0125 shown as `$0.01` loses a fifth of itself. The whole reason to put a price on a row
 * is to let someone decide whether the work was worth it.
 */
export function cost(cost: number): string {
  if (cost === 0) return "$0";
  if (cost < 0.01) return `$${cost.toFixed(4)}`;
  if (cost < 1) return `$${cost.toFixed(3)}`;
  return `$${cost.toFixed(2)}`;
}

/** A token count that fits a row. */
export function tokens(count: number): string {
  if (count < 1000) return `${count}`;
  if (count < 1_000_000) return `${(count / 1000).toFixed(1)}k`;
  return `${(count / 1_000_000).toFixed(1)}M`;
}

/** The context gauge a running agent's row carries, when the engine reported one. */
export function context(agent: AgentSnapshot): string | null {
  const used = agent.progress?.contextTokens;
  const window = agent.progress?.contextWindow;
  if (used === undefined || used === null || !window) return null;

  return `${tokens(used)} / ${tokens(window)} (${Math.round((used / window) * 100)}%)`;
}

/**
 * What an agent was asked to do, from the `task` call that spawned it.
 *
 * The card's own details are the source: the engine records `progress[].id` and the task text
 * there, so the join is the id — the same string the frames carry. A subagent whose card is
 * not in the transcript (a resumed thread whose history predates it, a transcript that was
 * trimmed) gets its row's own `task` instead, or nothing.
 */
export function taskFor(rows: RowSnapshot[], id: string): string | null {
  for (const row of rows) {
    const details = toolDetails(row.tool);
    if (!details) continue;
    const progress = details.progress;
    if (!Array.isArray(progress)) continue;
    for (const entry of progress) {
      if (entry === null || typeof entry !== "object") continue;
      const record = entry as Record<string, unknown>;
      if (record.id !== id) continue;
      const text = record.task ?? record.description;
      if (typeof text === "string" && text !== "") return text;
    }
  }
  return null;
}

/** The model recorded for a spawn, even after its live roster has gone away. */
export function modelFor(rows: RowSnapshot[], id: string): string | null {
  for (const row of rows) {
    const progress = toolDetails(row.tool)?.progress;
    if (!Array.isArray(progress)) continue;
    for (const entry of progress) {
      if (entry === null || typeof entry !== "object") continue;
      const record = entry as Record<string, unknown>;
      if (record.id === id && typeof record.resolvedModel === "string" && record.resolvedModel !== "") {
        return record.resolvedModel;
      }
    }
  }
  return null;
}

/** A tool card's `details`, parsed, or null. */
function toolDetails(tool: ToolSnapshot | null): Record<string, unknown> | null {
  if (!tool || tool.finished === false || tool.details === "") return null;
  return parseObject(tool.details);
}

// ------------------------------------------------------------------ jobs

/** One background job, as the transcript shows it. */
export interface JobRow {
  /** The engine's id, which is what a delivery names. */
  id: string;
  /** `bash` | `eval` | `task`. */
  kind: string;
  /** The tool call that started it. */
  toolCallId: string;
  toolName: string;
  /** The command or task, for the row's second line. */
  detail: string;
  /** True when a delivery has accounted for it. */
  delivered: boolean;
  /** How long it took, once its delivery says. */
  durationMs: number | null;
}

/**
 * The background jobs a conversation shows, open first.
 *
 * A job is visible in exactly two places, and neither is a command: the card that started it
 * carries `details.async` (`{jobId, state, type}` — measured against a real auto-backgrounded
 * `bash` call), and its result arrives later as a message of its own, `customType:
 * "async-result"`, holding `details.jobs[]` with the same `jobId` and the run's duration.
 * Matching those two is the whole of this function, and it is why a job row can stop saying
 * "running" without the app polling anything.
 *
 * The engine exposes no way for a host to list or cancel jobs — `hub` is a tool the agent
 * calls, and there is no RPC command for jobs at all — so this is a *derivation*, and the
 * panel says so where it matters.
 */
export function jobs(rows: RowSnapshot[]): JobRow[] {
  const delivered = new Map<string, number>();
  for (const row of rows) {
    if (row.customType !== "async-result") continue;
    for (const job of row.jobs ?? []) {
      delivered.set(job.jobId, job.durationMs);
    }
  }

  const seen = new Map<string, JobRow>();
  for (const row of rows) {
    const tool = row.tool;
    const details = toolDetails(tool);
    if (!tool || !details) continue;
    const async_ = details.async;
    if (async_ === null || typeof async_ !== "object") continue;

    const record = async_ as Record<string, unknown>;
    const id = typeof record.jobId === "string" ? record.jobId : null;
    if (id === null) continue;

    const args = parseObject(tool.args) ?? {};
    const command = args.command ?? args.code ?? args.prompt ?? args.task;
    seen.set(id, {
      id,
      kind: typeof record.type === "string" ? record.type : tool.toolName,
      toolCallId: tool.toolCallId,
      toolName: tool.toolName,
      detail: typeof command === "string" ? command : "",
      delivered: delivered.has(id),
      durationMs: delivered.get(id) ?? null,
    });
  }

  // A job the engine settled, newest last: the delivered ones are history, the open ones are
  // what a reader is looking for.
  return [...seen.values()].sort((left, right) => Number(left.delivered) - Number(right.delivered));
}

// ----------------------------------------------------------- parked rows

/** One row of the panel's list: a live agent, or one only its file remembers. */
export interface AgentEntry {
  /** Unique across the panel: one agent id can exist in two threads. */
  key: string;
  thread: string;
  id: string;
  /** The live row, when the engine is (or was) reporting on it. */
  agent: AgentSnapshot | null;
  /** The transcript on disk, when there is one. */
  parked: ParkedAgent | null;
  /** One level of nesting, for an agent that spawned its own. */
  depth: number;
}

/**
 * A thread's agents: the live roster joined with what is on disk.
 *
 * One row per agent id, because they are the same agent seen twice — the roster knows how it
 * is doing, the file knows where its transcript is. A live row with no file yet (a subagent
 * that has not written anything) keeps its place; a file with no live row is a settled agent
 * this window never watched run, which is most of them on a resumed session.
 *
 * Nesting comes from the child's own header, which names its parent's *file*: the parent's id
 * is that file's stem. The engine writes a child's transcript one directory down from its
 * parent, and its id is the whole stem (`Parent.Child`), so the join is on the parent's id.
 */
export function entries(thread: string, live: ThreadAgents | null, parked: ParkedAgent[]): AgentEntry[] {
  const byId = new Map<string, AgentEntry>();
  for (const agent of live?.agents ?? []) {
    byId.set(agent.id, { key: `${thread}:${agent.id}`, thread, id: agent.id, agent, parked: null, depth: 0 });
  }
  for (const file of parked) {
    const existing = byId.get(file.id);
    if (existing) {
      existing.parked = file;
      continue;
    }
    byId.set(file.id, { key: `${thread}:${file.id}`, thread, id: file.id, agent: null, parked: file, depth: 0 });
  }

  const list = [...byId.values()];
  for (const entry of list) {
    const parent = parentOf(entry);
    if (parent !== null && byId.has(parent)) entry.depth = 1;
  }

  // A depth-first order so a child sits under its parent, and everything else keeps the
  // index the engine dispatched it in.
  const ordered: AgentEntry[] = [];
  for (const entry of list.filter((candidate) => candidate.depth === 0)) {
    ordered.push(entry);
    for (const child of list) {
      if (child.depth === 1 && parentOf(child) === entry.id) ordered.push(child);
    }
  }
  for (const orphan of list.filter((candidate) => !ordered.includes(candidate))) ordered.push(orphan);

  return ordered;
}

/**
 * The agent that spawned this one, when it is knowable.
 *
 * Two shapes, because the engine writes them differently and both are measured:
 *
 * - **A transcript on disk** names its parent in its own header (`parentSession`, the parent's
 *   *file*), so the parent is that file's stem.
 * - **A live row** has no parent field at all — but a nested child's transcript lives one
 *   directory down, in a directory named after its parent (`<artifacts>/<Parent>/<Parent>.<Child>.jsonl`),
 *   so the directory is the link. This is the case a naive "the file it is writing is its
 *   parent's" reading gets wrong, which is how the first version of this missed every nesting.
 */
function parentOf(entry: AgentEntry): string | null {
  const named = stem(entry.parked?.parent ?? null);
  if (named !== null && named !== entry.id) return named;

  const path = entry.parked?.path ?? entry.agent?.sessionFile ?? null;
  if (path === null) return null;

  const segments = path.split("/");
  if (segments.length < 2) return null;
  const directory = segments[segments.length - 2];
  // The artifacts directory itself is named after the *session*, so a directory that is not
  // one of the agents here is not a parent.
  return directory !== undefined && directory !== "" && directory !== entry.id ? directory : null;
}

/** The id a transcript path names: its file stem. */
function stem(path: string | null): string | null {
  if (path === null) return null;
  const name = path.split("/").pop() ?? "";
  return name.endsWith(".jsonl") ? name.slice(0, -".jsonl".length) : null;
}

/** What an agent's row is called: the engine's label, else its id. */
export function label(entry: AgentEntry): string {
  const agent = entry.agent?.agent;
  if (agent !== undefined && agent !== null && agent !== "") return agent;
  if (entry.parked?.advisor) {
    return entry.parked.advisorSlug === null ? "advisor" : `advisor (${entry.parked.advisorSlug})`;
  }
  return entry.id;
}

/** The one line under the label: what it is doing, or what it was asked to do. */
export function subtitle(entry: AgentEntry): string | null {
  const progress = entry.agent?.progress;
  if (progress?.lastIntent) return progress.lastIntent;
  if (progress?.currentTool) return `${progress.currentTool}${progress.currentToolArgs ? ` ${progress.currentToolArgs}` : ""}`;
  if (entry.agent?.description) return entry.agent.description;
  if (entry.agent?.task) return entry.agent.task;
  if (entry.parked) return `${entry.parked.bytes} bytes of transcript`;
  return null;
}

/** The four numbers a row shows when the engine reported a run. */
export function stats(entry: AgentEntry): { cost: number; tools: number; requests: number; tokens: number } | null {
  const progress = entry.agent?.progress;
  if (!progress) return null;
  return {
    cost: progress.cost,
    tools: progress.toolCount,
    requests: progress.requests,
    tokens: progress.tokens,
  };
}

/** A subagent called within a conversation, live or settled. */
export interface ChatSubagent {
  id: string;
  index: number;
  thread: string;
  name: string;
  status: "running" | "completed" | "failed" | "aborted" | "settled";
  running: boolean;
  spawnedAt: number;
  finishedAt: number | null;
  lastUpdateMs: number;
  description: string | null;
  task: string | null;
  hasTranscript: boolean;
}

/** Extracts spawn timestamp and index for agents mentioned in tool calls. */
export function spawnTimesFromRows(rows: RowSnapshot[] = []): Map<string, { timestamp: number; index: number }> {
  const result = new Map<string, { timestamp: number; index: number }>();
  for (const row of rows) {
    const details = toolDetails(row.tool);
    if (!details) continue;
    const progress = details.progress;
    if (!Array.isArray(progress)) continue;
    const baseTime = row.timestamp ?? 0;
    progress.forEach((entry, idx) => {
      if (entry && typeof entry === "object") {
        const record = entry as Record<string, unknown>;
        if (typeof record.id === "string" && !result.has(record.id)) {
          result.set(record.id, {
            timestamp: baseTime,
            index: typeof record.index === "number" ? record.index : idx,
          });
        }
      }
    });
  }
  return result;
}

/**
 * Builds the complete list of subagents for a thread, finished and not,
 * sorted with in-flight ones first (newer spawned on top, older under),
 * then finished subagents (newer on top).
 */
export function buildChatSubagents(
  thread: string,
  liveAgents: AgentSnapshot[] = [],
  parked: ParkedAgent[] = [],
  history: AgentSnapshot[] = [],
  rows: RowSnapshot[] = [],
  spawnTimes?: Map<string, number>,
): ChatSubagent[] {
  const map = new Map<string, ChatSubagent>();
  const rowSpawns = spawnTimesFromRows(rows);

  function resolveSpawnedAt(id: string, index: number, fallback: number): number {
    if (spawnTimes && spawnTimes.has(id)) {
      return spawnTimes.get(id)!;
    }
    const fromRow = rowSpawns.get(id);
    if (fromRow && fromRow.timestamp > 0) {
      const ts = fromRow.timestamp + (fromRow.index ?? index);
      spawnTimes?.set(id, ts);
      return ts;
    }
    const val = fallback > 0 ? fallback : Date.now();
    spawnTimes?.set(id, val);
    return val;
  }

  const liveInFlight = new Set(
    liveAgents.filter(agentInFlight).map((a) => a.id),
  );

  // 1. Parked transcripts on disk (settled from past runs or finished)
  for (const file of parked) {
    const name = file.advisor
      ? (file.advisorSlug ? `advisor (${file.advisorSlug})` : "advisor")
      : file.id;
    const spawnedAt = resolveSpawnedAt(file.id, 0, file.modifiedMs || 0);
    map.set(file.id, {
      id: file.id,
      index: 0,
      thread,
      name,
      status: "completed",
      running: false,
      spawnedAt,
      finishedAt: file.modifiedMs || 0,
      lastUpdateMs: file.modifiedMs || 0,
      description: null,
      task: null,
      hasTranscript: true,
    });
  }

  // 2. Historical run cache for this app session
  for (const h of history) {
    const running = liveInFlight.has(h.id);
    const existing = map.get(h.id);
    const status = running
      ? "running"
      : (h.status === "running" || h.status === "pending"
          ? "completed"
          : (h.status as "completed" | "failed" | "aborted") || existing?.status || "completed");
    const spawnedAt = existing?.spawnedAt ?? resolveSpawnedAt(h.id, h.index ?? 0, h.createdAtMs || h.lastUpdateMs || 0);
    map.set(h.id, {
      id: h.id,
      index: h.index ?? existing?.index ?? 0,
      thread,
      name: h.agent || existing?.name || h.id,
      status,
      running,
      spawnedAt,
      finishedAt: running ? null : (h.lastUpdateMs || existing?.finishedAt || Date.now()),
      lastUpdateMs: h.lastUpdateMs || existing?.lastUpdateMs || 0,
      description: h.progress?.lastIntent || h.description || existing?.description || null,
      task: h.task || existing?.task || null,
      hasTranscript: true,
    });
  }

  // 3. Current live roster
  for (const a of liveAgents) {
    const running = agentInFlight(a);
    const existing = map.get(a.id);
    const status = running
      ? "running"
      : (a.status === "running" || a.status === "pending"
          ? "completed"
          : (a.status as "completed" | "failed" | "aborted") || existing?.status || "completed");
    const spawnedAt = existing?.spawnedAt ?? resolveSpawnedAt(a.id, a.index ?? 0, a.createdAtMs || a.lastUpdateMs || 0);
    map.set(a.id, {
      id: a.id,
      index: a.index ?? existing?.index ?? 0,
      thread,
      name: a.agent || existing?.name || a.id,
      status,
      running,
      spawnedAt,
      finishedAt: running ? null : (a.lastUpdateMs || existing?.finishedAt || Date.now()),
      lastUpdateMs: a.lastUpdateMs || existing?.lastUpdateMs || 0,
      description: a.progress?.lastIntent || a.description || existing?.description || null,
      task: a.task || existing?.task || null,
      hasTranscript: true,
    });
  }

  // Sort: running on top (newer spawned on top, older under), then finished (newer finished on top)
  return [...map.values()].sort((a, b) => {
    // 1. Running subagents stay on top of finished subagents
    if (a.running && !b.running) return -1;
    if (!a.running && b.running) return 1;

    // 2. Both running: newer spawned on top, older under (stable while running!)
    if (a.running && b.running) {
      if (b.spawnedAt !== a.spawnedAt) {
        return b.spawnedAt - a.spawnedAt;
      }
      if (b.index !== a.index) {
        return b.index - a.index;
      }
      return b.id.localeCompare(a.id);
    }

    // 3. Both finished: newer finished on top, fallback to newer spawned
    const timeA = a.finishedAt ?? a.spawnedAt;
    const timeB = b.finishedAt ?? b.spawnedAt;
    if (timeB !== timeA) {
      return timeB - timeA;
    }
    if (b.spawnedAt !== a.spawnedAt) {
      return b.spawnedAt - a.spawnedAt;
    }
    if (b.index !== a.index) {
      return b.index - a.index;
    }
    return b.id.localeCompare(a.id);
  });
}
