import type { ToolSnapshot } from "../bridge";
import { parseObject, touchedFiles } from "./toolView";
import { batchReviewPiece, orderedReviewPieces, type ReviewPiece } from "./reviewDiff";

export interface FileDiff {
  path: string;
  pieces: ReviewPiece[];
  added: number | null;
  removed: number | null;
}

export interface ActivityDiff {
  files: FileDiff[];
  /** A joined diff only when the engine did not provide safe per-file attribution. */
  batch: ReviewPiece | null;
  added: number | null;
  removed: number | null;
}

function counts(pieces: ReviewPiece[]): { added: number; removed: number } | null {
  if (pieces.length === 0) return null;
  let added = 0;
  let removed = 0;
  for (const piece of pieces) {
    if (piece.kind !== "diff") continue;
    for (const line of piece.text.split("\n")) {
      if (line.startsWith("+") && !line.startsWith("+++ ")) added++;
      else if (line.startsWith("-") && !line.startsWith("--- ")) removed++;
    }
  }
  return { added, removed };
}

function rawCounts(diff: string): { added: number; removed: number } | null {
  if (!diff) return null;
  let added = 0;
  let removed = 0;
  for (const line of diff.split("\n")) {
    if (line.startsWith("+") && !line.startsWith("+++ ")) added++;
    else if (line.startsWith("-") && !line.startsWith("--- ")) removed++;
  }
  return added + removed > 0 ? { added, removed } : null;
}

function countLines(content: string): number {
  if (!content) return 0;
  const end = content.endsWith("\n")
    ? (content.endsWith("\r\n") ? content.length - 2 : content.length - 1)
    : content.length;
  if (end <= 0) return 0;
  let lines = 1;
  for (let i = 0; i < end; i++) {
    if (content.charCodeAt(i) === 10) lines++;
  }
  return lines;
}

export function wholeFileDiffPiece(content: string): ReviewPiece | null {
  if (!content) return null;
  const rawLines = content.replace(/\r?\n$/, "").split(/\r?\n/);
  if (rawLines.length === 0 || (rawLines.length === 1 && rawLines[0] === "")) return null;
  const diffLines = [
    `@@ -0,0 +1,${rawLines.length} @@`,
    ...rawLines.map((line) => `+${line}`),
  ];
  return { kind: "diff", text: diffLines.join("\n") };
}

/** Counts for collapsed headers without constructing source-ordered diff rows. */
export function activityDiffCounts(tool: ToolSnapshot): { added: number; removed: number } | null {
  if (!tool.finished || tool.isError) return null;
  const details = parseObject(tool.details);
  const args = parseObject(tool.args);
  const touched = touchedFiles(tool);
  if (touched.length === 0) return null;
  if (touched.length === 1) {
    const diff = typeof details?.diff === "string" ? details.diff
      : typeof args?.diff === "string" ? args.diff : "";
    const counts = rawCounts(diff);
    if (counts) return counts;
    if (tool.toolName === "write" && typeof args?.content === "string") {
      const added = countLines(args.content);
      return added > 0 ? { added, removed: 0 } : null;
    }
    return null;
  }
  const entries = Array.isArray(details?.perFileResults) ? details.perFileResults : [];
  if (entries.length !== touched.length) return null;
  const parts = entries.map((entry) =>
    entry !== null && typeof entry === "object" && typeof entry.diff === "string"
      ? rawCounts(entry.diff) : null);
  if (parts.some((part) => part === null)) return null;
  return parts.reduce<{ added: number; removed: number }>(
    (sum, part) => ({ added: sum.added + part!.added, removed: sum.removed + part!.removed }),
    { added: 0, removed: 0 },
  );
}

/** Display changed lines; synthesizes an added-lines diff for whole-file writes. */
export function activityDiff(tool: ToolSnapshot): ActivityDiff {
  const details = parseObject(tool.details);
  const args = parseObject(tool.args);
  const touched = touchedFiles(tool);
  const perFile = Array.isArray(details?.perFileResults)
    ? details.perFileResults.filter((value): value is Record<string, unknown> => value !== null && typeof value === "object")
    : [];
  const joined = typeof details?.diff === "string" ? details.diff
    : typeof args?.diff === "string" ? args.diff : null;
  const files = touched.map(({ path }): FileDiff => {
    const fromResult = perFile.find((value) => value.path === path);
    const diff = typeof fromResult?.diff === "string" ? fromResult.diff
      : touched.length === 1 ? joined : null;
    let pieces = diff ? orderedReviewPieces([diff]) : [];
    if (pieces.length === 0 && tool.toolName === "write" && typeof args?.content === "string") {
      const whole = wholeFileDiffPiece(args.content);
      if (whole) pieces = [whole];
    }
    const total = counts(pieces);
    return { path, pieces, added: total?.added ?? null, removed: total?.removed ?? null };
  });

  const needsBatch = touched.length > 1 && files.some((file) => file.pieces.length === 0);
  const batch = needsBatch && joined ? batchReviewPiece(joined) : null;
  const allKnown = files.length > 0 && files.every((file) => file.added !== null);
  return {
    files,
    batch,
    added: allKnown && !needsBatch ? files.reduce((sum, file) => sum + file.added!, 0) : null,
    removed: allKnown && !needsBatch ? files.reduce((sum, file) => sum + file.removed!, 0) : null,
  };
}
