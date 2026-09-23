import type { ConversationTurn } from "./turnActivity";

export interface TurnLayout {
  /** Prefix positions, including the trailing total at `offsets[turns.length]`. */
  offsets: number[];
  total: number;
}

export interface TurnWindow { start: number; end: number; before: number; after: number }

export function estimatedTurnHeight(turn: ConversationTurn, hasFiles: boolean): number {
  const toolRows = turn.items.reduce((count, item) => count + (item.kind === "group" ? item.group.rows.length : 0), 0);
  const otherRows = turn.items.reduce((count, item) => count + (item.kind === "row" ? 1 : 0), 0);
  return 112 + toolRows * 34 + otherRows * 45 + (hasFiles ? 105 : 0);
}

export function turnLayout(
  turns: readonly ConversationTurn[], measured: ReadonlyMap<number, number>, hasFiles: (end: number) => boolean,
): TurnLayout {
  const offsets = [0];
  for (const turn of turns) {
    offsets.push(offsets[offsets.length - 1] + (measured.get(turn.start) ?? estimatedTurnHeight(turn, hasFiles(turn.end))));
  }
  return { offsets, total: offsets[offsets.length - 1] };
}

/** Binary search keeps scroll handling logarithmic even with thousands of turns. */
export function visibleTurns(layout: TurnLayout, top: number, viewportHeight: number, overscan = 700): TurnWindow {
  const count = layout.offsets.length - 1;
  if (count === 0) return { start: 0, end: 0, before: 0, after: 0 };
  const low = Math.max(0, top - overscan);
  const high = Math.max(low, top + viewportHeight + overscan);
  let left = 0;
  let right = count;
  while (left < right) {
    const middle = (left + right) >>> 1;
    if (layout.offsets[middle + 1] <= low) left = middle + 1;
    else right = middle;
  }
  const start = Math.min(left, count - 1);
  left = start + 1;
  right = count;
  while (left < right) {
    const middle = (left + right) >>> 1;
    if (layout.offsets[middle] < high) left = middle + 1;
    else right = middle;
  }
  const end = Math.max(start + 1, left);
  return { start, end, before: layout.offsets[start], after: layout.total - layout.offsets[end] };
}

export function turnContainingRow(turns: readonly ConversationTurn[], row: number): number {
  let left = 0;
  let right = turns.length;
  while (left < right) {
    const middle = (left + right) >>> 1;
    if (turns[middle].end < row) left = middle + 1;
    else right = middle;
  }
  return left < turns.length && turns[left].start <= row ? left : -1;
}
