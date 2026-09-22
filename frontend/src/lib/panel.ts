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

import type { RowSnapshot, ToolSnapshot } from "../bridge";
import { parseObject, touchedFiles, toolView } from "./toolView";

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
  return path.startsWith(base) ? path.slice(base.length) : path;
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
