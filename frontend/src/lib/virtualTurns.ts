import type { ConversationTurn } from "./turnActivity";

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
