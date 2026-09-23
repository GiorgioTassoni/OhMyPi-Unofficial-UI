import type { RowSnapshot } from "../bridge";
import { turnFileSummaries, type TurnFileSummary } from "./panel";
import { patchedConversationTurns, type ConversationTurn } from "./turnActivity";

export interface TranscriptProjection {
  turns: ConversationTurn[];
  summaries: Map<number, TurnFileSummary>;
}

/** A host patch replaces a suffix; project only the turn that suffix intersects. */
export function projectTranscript(
  previous: TranscriptProjection, rows: RowSnapshot[], streaming: boolean, from: number,
): TranscriptProjection {
  const { turns, rebuiltFrom } = patchedConversationTurns(previous.turns, rows, streaming, from);
  const summaries = new Map<number, TurnFileSummary>();
  for (const [after, summary] of previous.summaries) {
    if (after < rebuiltFrom) summaries.set(after, summary);
  }
  for (const summary of turnFileSummaries(rows.slice(rebuiltFrom), streaming)) {
    const shifted: TurnFileSummary = {
      ...summary,
      after: summary.after + rebuiltFrom,
      files: summary.files.map((file) => ({ ...file, row: file.row + rebuiltFrom })),
      changes: summary.changes.map((change) => ({ ...change, row: change.row + rebuiltFrom })),
    };
    summaries.set(shifted.after, shifted);
  }
  return { turns, summaries };
}

export function initialProjection(rows: RowSnapshot[], streaming: boolean): TranscriptProjection {
  return projectTranscript({ turns: [], summaries: new Map() }, rows, streaming, 0);
}
