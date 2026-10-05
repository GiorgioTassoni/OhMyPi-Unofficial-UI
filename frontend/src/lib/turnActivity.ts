import type { RowSnapshot, ToolSnapshot } from "../bridge";
import { turnContainingRow } from "./virtualTurns";
import { parseObject, touchedFiles } from "./toolView";
import { activityDiffCounts } from "./activityDiff";

export interface IndexedRow { index: number; row: RowSnapshot }
export type GroupKind = "read" | "command" | "edit" | "other";
export interface ToolGroup {
  kind: GroupKind;
  rows: IndexedRow[];
  label: string;
  added: number | null;
  removed: number | null;
}
export type AgentCompletion = { kind: "agent-completion"; entry: IndexedRow; index: number; name: string; status: "completed" | "failed" | "aborted" };
export type ActivityItem = { kind: "group"; group: ToolGroup; index: number } | { kind: "row"; entry: IndexedRow; index: number } | AgentCompletion;
export interface ConversationTurn {
  start: number;
  end: number;
  user: IndexedRow | null;
  answer: IndexedRow | null;
  items: ActivityItem[];
  /** Confirmed spawns recorded in task tool details, kept visible when work is collapsed. */
  spawnedAgents: string[];
  working: boolean;
  durationMs: number | null;
}

function category(tool: ToolSnapshot): GroupKind {
  if (tool.toolName === "read") return "read";
  if (["bash", "bash_interactive", "eval"].includes(tool.toolName)) return "command";
  if (["edit", "write", "ast_edit", "apply_patch", "memory_edit"].includes(tool.toolName)) return "edit";
  return "other";
}

function makeGroup(kind: GroupKind, rows: IndexedRow[]): ToolGroup {
  const count = rows.length;
  let label: string;
  if (kind === "read") label = `Read ${count} file${count === 1 ? "" : "s"}`;
  else if (kind === "command") label = `Ran ${count} command${count === 1 ? "" : "s"}`;
  else if (kind === "edit") {
    const paths = new Set(rows.flatMap(({ row }) => row.tool ? touchedFiles(row.tool).map((file) => file.path) : []));
    const files = paths.size || count;
    label = `Edited ${files} file${files === 1 ? "" : "s"}`;
  } else label = `Used ${count} tool${count === 1 ? "" : "s"}`;

  const counts = kind === "edit" ? rows.map(({ row }) => {
    if (!row.tool?.finished || row.tool.isError) return null;
    return activityDiffCounts(row.tool);
  }) : [];
  return {
    kind, rows, label,
    added: counts.length > 0 && counts.every((value) => value !== null) ? counts.reduce((sum, value) => sum + value!.added, 0) : null,
    removed: counts.length > 0 && counts.every((value) => value !== null) ? counts.reduce((sum, value) => sum + value!.removed, 0) : null,
  };
}

function taskProgress(tool: ToolSnapshot): Array<{ id: string; status: string | null }> {
  const progress = parseObject(tool.details)?.progress;
  if (!Array.isArray(progress)) return [];
  return progress.flatMap((item) => {
    if (item === null || typeof item !== "object") return [];
    const record = item as Record<string, unknown>;
    return typeof record.id === "string" && record.id.trim() !== ""
      ? [{ id: record.id, status: typeof record.status === "string" ? record.status : null }]
      : [];
  });
}

function taskJobNames(rows: RowSnapshot[]): Map<string, string[]> {
  const names = new Map<string, string[]>();
  for (const row of rows) {
    const tool = row.tool;
    if (tool?.toolName !== "task") continue;
    const asyncJob = parseObject(tool.details)?.async;
    if (asyncJob === null || typeof asyncJob !== "object") continue;
    const id = (asyncJob as Record<string, unknown>).jobId;
    if (typeof id === "string") names.set(id, [...new Set(taskProgress(tool).map((agent) => agent.id))]);
  }
  return names;
}

export function isSystemReminder(text?: string | null): boolean {
  if (!text) return false;
  return text.trim().startsWith("<system-reminder");
}

function turn(segment: IndexedRow[], working: boolean, jobNames: ReadonlyMap<string, string[]>): ConversationTurn {
  const user = segment.find(({ row }) => row.role === "user" && !isSystemReminder(row.text)) ?? null;
  const lastTool = segment.reduce((last, entry) => entry.row.role === "tool" ? entry.index : last, -1);
  const answer = [...segment].reverse().find(({ row, index }) => row.role === "assistant" && row.customType === null && !!row.text && index > lastTool) ?? null;
  const activity = segment.filter((entry) => entry !== user && entry !== answer);
  const items: ActivityItem[] = [];
  let runKind: GroupKind = "other";
  let runRows: IndexedRow[] = [];
  function flushRun(): void {
    if (runRows.length === 0) return;
    items.push({ kind: "group", group: makeGroup(runKind, runRows), index: runRows[0].index });
    for (const entry of runRows) {
      const tool = entry.row.tool;
      if (tool?.toolName !== "task" || !tool.finished || parseObject(tool.details)?.async) continue;
      for (const agent of new Map(taskProgress(tool).map((item) => [item.id, item])).values()) {
        const status = agent.status === "failed" || agent.status === "aborted" ? agent.status : tool.isError ? "failed" : "completed";
        items.push({ kind: "agent-completion", entry, index: entry.index, name: agent.id, status });
      }
    }
    runRows = [];
  }
  for (const entry of activity) {
    if (isSystemReminder(entry.row.text)) {
      continue;
    }
    if (entry.row.role === "tool" && entry.row.tool) {
      const kind = category(entry.row.tool);
      // Only adjacent calls share a disclosure. An intervening thought, notice, or
      // different tool kind starts a fresh run and keeps transcript order intact.
      if (runRows.length > 0 && runKind !== kind) {
        flushRun();
      }
      runKind = kind;
      runRows.push(entry);
    } else {
      flushRun();
      if (entry.row.customType === "async-result" && entry.row.jobs.length > 0 && entry.row.jobs.every((job) => job.kind === "task")) {
        for (const job of entry.row.jobs) {
          for (const name of jobNames.get(job.jobId)?.length ? jobNames.get(job.jobId)! : [job.label || job.jobId]) {
            items.push({ kind: "agent-completion", entry, index: entry.index, name, status: "completed" });
          }
        }
        continue;
      }
      items.push({ kind: "row", entry, index: entry.index });
    }
  }
  flushRun();

  const spawnedAgents = new Set<string>();
  for (const { row } of segment) {
    if (row.tool?.toolName !== "task") continue;
    for (const agent of taskProgress(row.tool)) spawnedAgents.add(agent.id);
  }

  const startTime = user?.row.timestamp;
  const endTime = answer?.row.timestamp;
  const durationMs = !working && typeof startTime === "number" && Number.isFinite(startTime)
    && typeof endTime === "number" && Number.isFinite(endTime) && endTime >= startTime
    ? endTime - startTime : null;
  return { start: segment[0].index, end: segment[segment.length - 1].index, user, answer, items, spawnedAgents: [...spawnedAgents], working, durationMs };
}

function turnsFrom(rows: RowSnapshot[], streaming: boolean, start: number): ConversationTurn[] {
  const segments: IndexedRow[][] = [];
  for (let index = start; index < rows.length; index++) {
    const entry = { index, row: rows[index] };
    if ((rows[index].role === "user" && !isSystemReminder(rows[index].text)) || segments.length === 0) segments.push([]);
    segments[segments.length - 1].push(entry);
  }
  const jobNames = segments.some((segment) => segment.some(({ row }) => row.customType === "async-result" && row.jobs.some((job) => job.kind === "task")))
    ? taskJobNames(rows) : new Map<string, string[]>();
  return segments.map((segment, index) => turn(segment, streaming && index === segments.length - 1, jobNames));
}

export function conversationTurns(rows: RowSnapshot[], streaming: boolean): ConversationTurn[] {
  return turnsFrom(rows, streaming, 0);
}

/** Reuse every turn strictly before the patch; the latest turn is rebuilt on an append. */
export function patchedConversationTurns(
  previous: readonly ConversationTurn[], rows: RowSnapshot[], streaming: boolean, from: number,
): { turns: ConversationTurn[]; rebuiltFrom: number } {
  if (previous.length === 0 || from <= 0) return { turns: conversationTurns(rows, streaming), rebuiltFrom: 0 };
  const affected = turnContainingRow(previous, from);
  const keep = affected >= 0 ? affected : previous.length - 1;
  const prefix = previous.slice(0, keep);
  const rebuiltFrom = prefix.length ? prefix[prefix.length - 1].end + 1 : 0;
  return { turns: [...prefix, ...turnsFrom(rows, streaming, rebuiltFrom)], rebuiltFrom };
}

export function workLabel(item: ConversationTurn): string {
  if (item.working) return "Working…";
  if (item.durationMs === null) return "Completed turn";
  const seconds = Math.round(item.durationMs / 1000);
  return `Worked for ${Math.floor(seconds / 60)}m ${String(seconds % 60).padStart(2, "0")}s`;
}
