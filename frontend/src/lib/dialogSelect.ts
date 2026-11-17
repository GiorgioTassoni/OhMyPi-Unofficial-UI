let lastDialogInteractionTime = 0;
let lastDialogInteractionThread: string | null = null;

/** Record when the user answers a dialog to suppress redundant chimes in rapid follow-ups. */
export function markDialogInteraction(thread: string): void {
  lastDialogInteractionTime = Date.now();
  lastDialogInteractionThread = thread;
}

/** Check if the user interacted with a dialog recently in this thread. */
export function isRecentDialogInteraction(thread: string, maxAgeMs = 25_000): boolean {
  return thread === lastDialogInteractionThread && Date.now() - lastDialogInteractionTime < maxAgeMs;
}

/**
 * Normalizes and truncates a question title or dialog prompt for collapsed presentation.
 * Replaces newlines/multiple spaces with a single space and caps at `maxChars`.
 */
export function formatCollapsibleTitle(text: string, maxChars = 65): string {
  const single = text.replace(/\s+/g, " ").trim();
  if (single.length <= maxChars) {
    return single;
  }
  return single.slice(0, maxChars).trimEnd() + "…";
}

export interface DialogSummaryParams {
  titleText: string;
  message?: string | null | undefined;
  kind?: string | null | undefined;
  approvalTool?: string | null | undefined;
  command?: string | null | undefined;
}

/**
 * Derives a concise, one-line summary title for a dialog or approval request.
 */
export function dialogSummaryTitle(params: DialogSummaryParams, maxChars = 65): string {
  if (params.approvalTool) {
    if (params.command) {
      return formatCollapsibleTitle(`Run: ${params.command}`, maxChars);
    }
    return formatCollapsibleTitle(`Approval: ${params.approvalTool}`, maxChars);
  }
  const base = params.titleText || params.message || (params.kind === "confirm" ? "Confirmation" : "Question");
  return formatCollapsibleTitle(base, maxChars);
}
