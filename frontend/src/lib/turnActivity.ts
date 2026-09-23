import type { RowSnapshot, ToolSnapshot } from "../bridge";
import { touchedFiles } from "./toolView";
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
export type ActivityItem = { kind: "group"; group: ToolGroup; index: number } | { kind: "row"; entry: IndexedRow; index: number };
export interface ConversationTurn {
  start: number;
  end: number;
  user: IndexedRow | null;
  answer: IndexedRow | null;
  items: ActivityItem[];
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

function turn(segment: IndexedRow[], working: boolean): ConversationTurn {
  const user = segment.find(({ row }) => row.role === "user") ?? null;
  const lastTool = segment.reduce((last, entry) => entry.row.role === "tool" ? entry.index : last, -1);
  const answer = [...segment].reverse().find(({ row, index }) => row.role === "assistant" && !!row.text && index > lastTool) ?? null;
  const activity = segment.filter((entry) => entry !== user && entry !== answer);
  const items: ActivityItem[] = [];
  let runKind: GroupKind = "other";
  let runRows: IndexedRow[] = [];
  for (const entry of activity) {
    if (entry.row.role === "tool" && entry.row.tool) {
      const kind = category(entry.row.tool);
      // Only adjacent calls share a disclosure. An intervening thought, notice, or
      // different tool kind starts a fresh run and keeps transcript order intact.
      if (runRows.length > 0 && runKind !== kind) {
        items.push({ kind: "group", group: makeGroup(runKind, runRows), index: runRows[0].index });
        runRows = [];
      }
      runKind = kind;
      runRows.push(entry);
    } else {
      if (runRows.length > 0) {
        items.push({ kind: "group", group: makeGroup(runKind, runRows), index: runRows[0].index });
        runRows = [];
      }
      items.push({ kind: "row", entry, index: entry.index });
    }
  }
  if (runRows.length > 0) items.push({ kind: "group", group: makeGroup(runKind, runRows), index: runRows[0].index });

  const startTime = user?.row.timestamp;
  const endTime = answer?.row.timestamp;
  const durationMs = !working && typeof startTime === "number" && Number.isFinite(startTime)
    && typeof endTime === "number" && Number.isFinite(endTime) && endTime >= startTime
    ? endTime - startTime : null;
  return { start: segment[0].index, end: segment[segment.length - 1].index, user, answer, items, working, durationMs };
}

function turnsFrom(rows: RowSnapshot[], streaming: boolean, start: number): ConversationTurn[] {
  const segments: IndexedRow[][] = [];
  for (let index = start; index < rows.length; index++) {
    const entry = { index, row: rows[index] };
    if (rows[index].role === "user" || segments.length === 0) segments.push([]);
    segments[segments.length - 1].push(entry);
  }
  return segments.map((segment, index) => turn(segment, streaming && index === segments.length - 1));
}

export function conversationTurns(rows: RowSnapshot[], streaming: boolean): ConversationTurn[] {
  return turnsFrom(rows, streaming, 0);
}

/** Reuse every turn strictly before the patch; the latest turn is rebuilt on an append. */
export function patchedConversationTurns(
  previous: readonly ConversationTurn[], rows: RowSnapshot[], streaming: boolean, from: number,
): { turns: ConversationTurn[]; rebuiltFrom: number } {
  if (previous.length === 0 || from <= 0) return { turns: conversationTurns(rows, streaming), rebuiltFrom: 0 };
  const affected = previous.findIndex((item) => item.end >= from);
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
