/**
 * Folding the host's conversation patches into the rows on screen.
 *
 * The host never sends deltas: every patch is "the conversation, replaced from `from`
 * onward" (see `dto::RowPatch`). That is one operation for an append, a streaming rewrite,
 * a card settling behind later rows, and a reset — a delta protocol would need both sides
 * to agree about insertions and removals to stay aligned, and this one cannot drift.
 *
 * The one case it cannot express is a gap: a patch that starts *beyond* the rows we hold
 * means a patch was missed (a subscription that arrived late, a dropped event), and no
 * earlier patch will ever fill it in. The caller re-reads the transcript when that
 * happens, which is why this returns a flag rather than guessing.
 */

import type { RowPatch, RowSnapshot } from "../bridge";

export interface PatchOutcome {
  rows: RowSnapshot[];
  /** True when the patch cannot be applied to what we hold: re-read instead. */
  stale: boolean;
}

export function applyRowPatch(rows: RowSnapshot[], patch: RowPatch): PatchOutcome {
  if (patch.from > rows.length) {
    return { rows, stale: true };
  }
  return { rows: [...rows.slice(0, patch.from), ...patch.rows], stale: false };
}
