/**
 * The todo vocabulary (`docs/12` §8.1).
 *
 * The engine's statuses are an open union — `pending`, `in_progress`, `completed`,
 * `abandoned`, `blocked` today — so a status this build has not seen renders as itself with a
 * neutral mark rather than as "unknown", which would hide a real state behind a placeholder.
 */

/**
 * The word for a status.
 *
 * Only the *word*: how a state is drawn (the icon, its tint) belongs to the panel, which is
 * where the window's visual language lives — and a glyph string here would be a second drawing
 * of the same state that no stylesheet knows about. A status this build has not seen is its own
 * word rather than "unknown", because an unseen state is still a state.
 */
const WORDS: Record<string, string> = {
  pending: "pending",
  in_progress: "in progress",
  completed: "done",
  abandoned: "abandoned",
  blocked: "blocked",
};

export function todoLabel(status: string): string {
  return WORDS[status] ?? status;
}

/** How far along a phase is, for the count beside its name. */
export function phaseProgress(tasks: readonly { status: string }[]): {
  done: number;
  total: number;
} {
  return {
    done: tasks.filter((task) => task.status === "completed").length,
    total: tasks.length,
  };
}

/**
 * Whether a phase is worth opening by default.
 *
 * A phase with work left is the one a reader came for; a finished phase is a receipt. The
 * panel still lets either be opened — this only decides where the eyes start.
 */
export function phaseOpen(tasks: readonly { status: string }[]): boolean {
  return tasks.some((task) => task.status !== "completed" && task.status !== "abandoned");
}
