/**
 * The right panel's model (`docs/12` §8), derived from the transcript.
 *
 * Every view here is a function of rows the window already holds, rather than a second
 * source of truth the host has to keep in step with the conversation: the transcript *is*
 * the record of what a session did, and a file list read from anywhere else could disagree
 * with the cards on screen.
 *
 * The one thing this cannot see is a file a `bash` command wrote — the shell's own redirection
 * is not parsed, here or by the engine's reference renderer — so the Changed view says what
 * it is rather than presenting itself as a complete diff of the working tree.
 */

import type { RowSnapshot, SubagentFileChange, ToolSnapshot } from "../bridge";
import type { ConversationTurn } from "./turnActivity";
import { parseObject, touchedFiles, toolView } from "./toolView";
import { batchReviewPiece, orderedReviewPieces, type ReviewPiece } from "./reviewDiff";

export interface ChangedFile {
  /** Absolute whenever the engine resolved one, which it does for every write and edit. */
  path: string;
  /** `written` when any touch replaced the file's content, `edited` when only diffs did. */
  kind: "written" | "edited";
  /** How many calls touched it: "written then edited twice" is not the same change as one write. */
  touches: number;
  /**
   * The engine's diff for the call that touched it, and only when that call changed exactly
   * this file — a batched `edit` reports one joined diff for every file it touched, and
   * showing it against each of them would be a claim the engine did not make. The full
   * joined diff is on the tool card the row links to.
   */
  diff: string | null;
  /** Index into the transcript's rows: where the last touch was recorded. */
  row: number;
  /** The subagent that touched the file, if attributable to one. */
  agentName?: string | undefined;
  agentId?: string | undefined;
}

/**
 * Files this session changed, in the order it first touched them.
 *
 * First-touch order rather than most-recent-first, because the list is read while a turn
 * streams: a row that jumps position every time the agent edits it is a row nobody can click.
 */
export function changedFiles(rows: RowSnapshot[]): ChangedFile[] {
  const byPath = new Map<string, ChangedFile>();

  rows.forEach((row, index) => {
    const tool = row.tool;
    if (tool === null) return;
    // A call that has not ended has not changed anything yet: counting its arguments would
    // list a file the agent is still deciding about.
    if (!tool.finished || tool.isError) return;

    const touched = touchedFiles(tool);
    if (touched.length === 0) return;

    const diff = touched.length === 1 ? diffOf(tool) : null;

    for (const file of touched) {
      const existing = byPath.get(file.path);
      if (existing === undefined) {
        byPath.set(file.path, {
          path: file.path,
          kind: file.kind,
          touches: 1,
          diff,
          row: index,
        });
        continue;
      }

      existing.touches += 1;
      // A file the session wrote and then edited stays "written": that is the fact a
      // reviewer needs about it.
      if (file.kind === "written") existing.kind = "written";
      if (diff !== null) existing.diff = diff;
      existing.row = index;
    }
  });

  return [...byPath.values()];
}

export interface TurnChangedFile extends ChangedFile {
  /** Null when the tool gave no per-file diff (a write or a batched edit). */
  lines: { added: number; removed: number } | null;
}

export interface TurnFileSummary {
  /** Render after this transcript row, keeping search and panel row indices unchanged. */
  after: number;
  files: TurnChangedFile[];
  /** Shown only if every changed file has complete per-file counts. */
  lines: { added: number; removed: number } | null;
  /** Each successful mutating call, in order, for the Review disclosure. */
  changes: { row: number; paths: string[]; diff: string | null }[];
}

/** A request to show one turn's complete review in the right-hand Files panel. */
export interface TurnReviewRequest {
  thread: string;
  id: number;
  summary: TurnFileSummary;
}

export interface FileReview {
  /** Source-ordered hunks, with quiet markers for the untouched spans between them. */
  pieces: ReviewPiece[];
  /** Tool calls that changed the file but supplied no diff (notably whole-file writes). */
  withoutDiff: number[];
  /** A batch diff may also contain hunks for other files. */
  includesBatch: boolean;
}

export function fileReview(summary: TurnFileSummary, path: string): FileReview {
  const changes = summary.changes.filter((change) => change.paths.includes(path));
  const singleFileDiffs = changes
    .filter((change) => change.paths.length === 1)
    .map((change) => change.diff?.trimEnd())
    .filter((diff): diff is string => typeof diff === "string" && diff !== "");
  // A batch diff may contain several files. Keep that payload whole and in arrival order;
  // sorting its hunks as if they all belonged to this file would be misleading.
  const batchPieces: ReviewPiece[] = changes
    .filter((change) => change.paths.length > 1 && change.diff)
    .flatMap((change) => {
      const piece = batchReviewPiece(change.diff!);
      return piece === null ? [] : [piece];
    });

  return {
    pieces: [...orderedReviewPieces(singleFileDiffs), ...batchPieces],
    withoutDiff: changes
      .filter((change) => !change.diff || (
        change.paths.length === 1
          ? orderedReviewPieces([change.diff]).length === 0
          : batchReviewPiece(change.diff) === null
      ))
      .map((change) => change.row),
    includesBatch: changes.some((change) => change.diff !== null && change.paths.length > 1),
  };
}

/** Files changed between one user message and the next (or the settled transcript tail). */
export function turnFileSummaries(rows: RowSnapshot[], streaming: boolean): TurnFileSummary[] {
  const summaries: TurnFileSummary[] = [];
  let start = 0;

  function finish(end: number): void {
    if (end < start) return;
    const turn = rows.slice(start, end + 1);
    const files = changedFilesInTurn(turn).map((file) => ({ ...file, row: file.row + start }));
    if (files.length === 0) return;

    summaries.push({
      after: end,
      files,
      changes: turn.flatMap((row, index) => {
        const tool = row.tool;
        if (tool === null || !tool.finished || tool.isError) return [];
        const paths = touchedFiles(tool).map((file) => file.path);
        return paths.length === 0
          ? []
          : [{ row: index + start, paths, diff: diffOf(tool) }];
      }),
      lines: files.every((file) => file.lines !== null)
        ? files.reduce(
            (sum, file) => ({
              added: sum.added + (file.lines?.added ?? 0),
              removed: sum.removed + (file.lines?.removed ?? 0),
            }),
            { added: 0, removed: 0 },
          )
        : null,
    });
  }

  rows.forEach((row, index) => {
    if (row.role !== "user" || index === start) return;
    finish(index - 1);
    start = index;
  });

  // The session status, not a tool's row, decides whether the turn ended: stopped turns
  // can retain an unfinished card even though their completed edits should be summarized.
  if (!streaming) finish(rows.length - 1);
  return summaries;
}

/** Find the row index of the task tool call that spawned the given agent id. */
export function findTaskRowForAgent(
  rows: RowSnapshot[],
  start: number,
  end: number,
  agentId: string,
): number {
  for (let i = start; i <= end && i < rows.length; i++) {
    const tool = rows[i]?.tool;
    if (tool?.toolName === "task") {
      const details = parseObject(tool.details);
      const progress = details?.progress;
      if (Array.isArray(progress)) {
        for (const item of progress) {
          if (item && typeof item === "object" && (item as Record<string, unknown>).id === agentId) {
            return i;
          }
        }
      }
    }
  }
  return -1;
}

/** Merge subagent file mutations into a turn's file summary. */
export function mergeSubagentTurnFiles(
  base: TurnFileSummary | undefined,
  turnEnd: number,
  subagentChanges: SubagentFileChange[],
  defaultRowOrResolver: number | ((agentId: string) => number) = -1,
): TurnFileSummary {
  const byPath = new Map<string, TurnChangedFile>();
  const changes = base ? [...base.changes] : [];

  if (base) {
    for (const file of base.files) {
      byPath.set(file.path, { ...file });
    }
  }

  for (const sc of subagentChanges) {
    const scAgentName = sc.agentName && sc.agentName !== "task"
      ? sc.agentName
      : (sc.agentId && sc.agentId !== "task" ? sc.agentId : undefined);
    const existing = byPath.get(sc.path);
    const scLines = sc.diff ? { added: sc.linesAdded, removed: sc.linesRemoved } : null;
    const row = typeof defaultRowOrResolver === "function"
      ? defaultRowOrResolver(sc.agentId)
      : defaultRowOrResolver;

    if (existing) {
      existing.touches += sc.touches;
      if (sc.kind === "written") existing.kind = "written";
      if (!existing.diff && sc.diff) existing.diff = sc.diff;
      if (existing.lines !== null && scLines !== null) {
        existing.lines = {
          added: existing.lines.added + scLines.added,
          removed: existing.lines.removed + scLines.removed,
        };
      } else {
        existing.lines = null;
      }
      if (!existing.agentName) {
        existing.agentName = scAgentName;
      } else if (scAgentName && !existing.agentName.includes(scAgentName)) {
        existing.agentName = `${existing.agentName}, ${scAgentName}`;
      }
    } else {
      byPath.set(sc.path, {
        path: sc.path,
        kind: sc.kind,
        touches: sc.touches,
        diff: sc.diff,
        row,
        lines: scLines,
        agentName: scAgentName,
        agentId: sc.agentId,
      });
    }

    changes.push({
      row,
      paths: [sc.path],
      diff: sc.diff,
    });
  }

  const files = [...byPath.values()];
  const allCounted = files.every((f) => f.lines !== null);
  const lines = allCounted
    ? files.reduce(
        (sum, f) => ({
          added: sum.added + (f.lines?.added ?? 0),
          removed: sum.removed + (f.lines?.removed ?? 0),
        }),
        { added: 0, removed: 0 },
      )
    : null;

  return {
    after: turnEnd,
    files,
    changes,
    lines,
  };
}

/** Merge subagent file changes across all turns. */
export function mergeSubagentFileSummaries(
  baseSummaries: Map<number, TurnFileSummary>,
  turns: ConversationTurn[],
  rows: RowSnapshot[],
  subagentChanges: SubagentFileChange[],
): Map<number, TurnFileSummary> {
  const result = new Map<number, TurnFileSummary>(baseSummaries);
  if (subagentChanges.length === 0) return result;

  for (const turn of turns) {
    if (turn.working) continue;
    if (turn.spawnedAgents.length === 0) continue;

    const turnChanges = subagentChanges.filter((sc) =>
      turn.spawnedAgents.includes(sc.agentId),
    );
    if (turnChanges.length === 0) continue;

    const rowResolver = (agentId: string) => {
      const taskRow = findTaskRowForAgent(rows, turn.start, turn.end, agentId);
      return taskRow >= 0 ? taskRow : turn.start;
    };
    const base = baseSummaries.get(turn.end);
    const merged = mergeSubagentTurnFiles(base, turn.end, turnChanges, rowResolver);
    result.set(turn.end, merged);
  }

  return result;
}

/** Keep counts honest: a joined batch diff or a whole-file write has no per-file line total. */
function changedFilesInTurn(rows: RowSnapshot[]): TurnChangedFile[] {
  const counts = new Map<string, { added: number; removed: number } | null>();

  for (const row of rows) {
    const tool = row.tool;
    if (tool === null || !tool.finished || tool.isError) continue;
    const touched = touchedFiles(tool);
    if (touched.length === 0) continue;
    const diff = touched.length === 1 ? diffOf(tool) : null;
    const next = diff === null ? null : diffLineCounts(diff);

    for (const file of touched) {
      const previous = counts.get(file.path);
      counts.set(
        file.path,
        previous === null || next === null
          ? null
          : {
              added: (previous?.added ?? 0) + next.added,
              removed: (previous?.removed ?? 0) + next.removed,
            },
      );
    }
  }

  return changedFiles(rows).map((file) => ({ ...file, lines: counts.get(file.path) ?? null }));
}

function diffLineCounts(diff: string): { added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const line of diff.split("\n")) {
    if (line.startsWith("+++ ") || line.startsWith("--- ")) continue;
    if (line.startsWith("+")) added += 1;
    else if (line.startsWith("-")) removed += 1;
  }
  return { added, removed };
}

/** The rows whose results spilled to an artifact, newest last. */
export interface ArtifactRef {
  /** The id in `artifact://<id>`. */
  id: string;
  tool: string;
  row: number;
}

export function artifacts(rows: RowSnapshot[]): ArtifactRef[] {
  const found: ArtifactRef[] = [];

  rows.forEach((row, index) => {
    const tool = row.tool;
    if (tool === null) return;

    const id = toolView(tool).artifact;
    if (id !== null) found.push({ id, tool: tool.toolName, row: index });
  });

  return found;
}

/**
 * Where a todo's own words appear in the transcript, if they appear at all.
 *
 * The phases arrive from `get_state`, which carries no row: the link back is the text the
 * `todo` tool was called with. A task the agent wrote and has not mentioned since still has
 * a row from the first write, which is why this looks for the *earliest* mention — that is
 * where a reader wants to be taken.
 */
export function taskRow(rows: RowSnapshot[], task: string): number | null {
  const needle = task.trim();
  if (needle === "") return null;

  for (const [index, row] of rows.entries()) {
    const tool = row.tool;
    if (tool === null || tool.toolName !== "todo") continue;
    if (
      tool.args.includes(needle) ||
      tool.details.includes(needle) ||
      tool.output.includes(needle)
    ) {
      return index;
    }
  }

  return null;
}

/** A byte count a reader can compare at a glance. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * A path as a reader wants it: relative to the workspace it is inside.
 *
 * The panel shows several threads at once, each with its own workspace, so a bare absolute path
 * wastes the width that says *which* file this is. A path outside the workspace is left alone —
 * shortening it against a root it does not share would invent a relationship.
 */
export function relativeTo(path: string, root: string | null): string {
  if (root === null || root === "") return path;
  const base = root.endsWith("/") ? root : `${root}/`;
  if (path.startsWith(base)) return path.slice(base.length);

  // If path is outside root, check if they share a common project/parent folder (e.g. sibling folders)
  const rootParts = root.split("/").filter(Boolean);
  const pathParts = path.split("/").filter(Boolean);
  let common = 0;
  while (common < rootParts.length && common < pathParts.length && rootParts[common] === pathParts[common]) {
    common++;
  }
  if (common >= 2 && rootParts.length - common <= 2) {
    const up = "../".repeat(rootParts.length - common);
    return `${up}${pathParts.slice(common).join("/")}`;
  }

  return path;
}

/** The engine's own unified diff for a call, when the tool reported one. */
function diffOf(tool: ToolSnapshot): string | null {
  for (const text of [tool.details, tool.args]) {
    const parsed = parseObject(text);
    const diff = parsed?.diff;
    if (typeof diff === "string" && diff !== "") return diff;
  }
  return null;
}
