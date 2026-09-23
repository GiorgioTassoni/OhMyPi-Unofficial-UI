/**
 * The end-of-turn review shows only changed code lines. Tool cards still render the complete
 * engine diff, including context. A review is edit history, not a synthesized net diff.
 */
export type ReviewPiece =
  | { kind: "diff"; text: string }
  | { kind: "gap"; lines: number };

interface ChangedLine {
  /** Position in the new file, when the engine supplied one. */
  at: number | null;
  text: string;
  source: number;
  order: number;
}

const UNIFIED_HUNK = /^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/;
const NUMBERED_LINE = /^\s*([+-])(\d+)\|/;

/** Sort changed lines by source position, then mark only the gaps between those positions. */
export function orderedReviewPieces(diffs: readonly string[]): ReviewPiece[] {
  const lines = diffs.flatMap((diff, source) => changedLines(diff, source));
  const located = lines
    .filter((line): line is ChangedLine & { at: number } => line.at !== null)
    .sort((a, b) => a.at - b.at || a.source - b.source || a.order - b.order);

  const pieces: ReviewPiece[] = [];
  let current: string[] = [];
  let previous: number | null = null;
  function flush(): void {
    if (current.length > 0) pieces.push({ kind: "diff", text: current.join("\n") });
    current = [];
  }

  for (const line of located) {
    if (previous !== null && line.at > previous + 1) {
      flush();
      pieces.push({ kind: "gap", lines: line.at - previous - 1 });
    }
    current.push(line.text);
    previous = Math.max(previous ?? line.at, line.at);
  }
  flush();

  // A diff without line positions cannot be sorted honestly. Keep each such payload in the
  // engine's arrival order, after the located edits, rather than assigning it a fake position.
  const unlocated = lines.filter((line) => line.at === null);
  for (const source of new Set(unlocated.map((line) => line.source))) {
    pieces.push({
      kind: "diff",
      text: unlocated.filter((line) => line.source === source).map((line) => line.text).join("\n"),
    });
  }
  return pieces;
}

/** For a multi-file batch, keep file labels and changed lines but do not sort between files. */
export function batchReviewPiece(diff: string): ReviewPiece | null {
  const lines = diff.trimEnd().split("\n");
  if (!lines.some(isChange)) return null;
  const visible = lines.filter((line) =>
    line.startsWith("diff ") || line.startsWith("--- ") || line.startsWith("+++ ") || isChange(line),
  );
  return { kind: "diff", text: visible.join("\n") };
}

function changedLines(diff: string, source: number): ChangedLine[] {
  const lines = diff.trimEnd().split("\n");
  if (lines.length === 1 && lines[0] === "") return [];

  if (lines.some((line) => UNIFIED_HUNK.test(line))) {
    const changed: ChangedLine[] = [];
    let newLine = 0;
    let inHunk = false;
    for (const line of lines) {
      const header = UNIFIED_HUNK.exec(line);
      if (header) {
        newLine = Number(header[1]);
        inHunk = true;
        continue;
      }
      if (!inHunk) continue;
      if (line.startsWith("+")) {
        changed.push({ at: newLine, text: line, source, order: changed.length });
        newLine += 1;
      } else if (line.startsWith("-")) {
        changed.push({ at: newLine, text: line, source, order: changed.length });
      } else if (line.startsWith(" ")) {
        newLine += 1;
      }
    }
    return changed;
  }

  if (lines.some((line) => NUMBERED_LINE.test(line))) {
    return lines.flatMap((line, order) => {
      const match = NUMBERED_LINE.exec(line);
      return match ? [{ at: Number(match[2]), text: line, source, order }] : [];
    });
  }

  const changed = lines.filter(isChange);
  return changed.map((text, order) => ({
    at: null, text, source, order,
  }));
}

function isChange(line: string): boolean {
  return (line.startsWith("+") && !line.startsWith("+++ ")) ||
    (line.startsWith("-") && !line.startsWith("--- "));
}
