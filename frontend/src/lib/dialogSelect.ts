import type { UiRequestSnapshot } from "../bridge";

const STORAGE_KEY = "ohmypi:dialog_selected_options";

/**
 * Returns true if the option label represents the confirmation action to finish selecting
 * in a multi-choice flow (e.g. "✓ Done selecting", "Done selecting").
 */
export function isDoneOption(option: string): boolean {
  const clean = option
    .replace(/^[\s✓✔\u2713\u2714\u2611\u25A0\u25CF\*\-\+\[\]xX]+/, "")
    .trim()
    .toLowerCase();
  return (
    clean === "done selecting" ||
    clean === "done" ||
    clean === "fine selezione" ||
    clean === "fine" ||
    clean.startsWith("done selecting")
  );
}

/**
 * Returns true if the option is a custom input row (e.g. "Other (type your own)").
 */
export function isCustomInputOption(option: string): boolean {
  const clean = option.trim().toLowerCase();
  return (
    clean === "other" ||
    clean.startsWith("other (") ||
    clean === "altro" ||
    clean.startsWith("altro (")
  );
}

/**
 * Strips leading marker bullets, checks or icons from an option string for presentation.
 */
export function cleanOptionLabel(option: string): string {
  if (isDoneOption(option)) {
    return "Done selecting";
  }
  return option.replace(/^(\[[ xX✓✔☑]\]|[✓✔☑•\*\-\+])\s*/i, "").trim();
}

export interface ParsedDialogTitle {
  selectedBadge: string | null;
  stepBadge: string | null;
  isCustomResponse: boolean;
  text: string;
}

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
 * Parses the dialog title to extract an optional `N selected` badge, `X of Y` progress,
 * detects custom response follow-ups, and strips clutter from the question text.
 */
export function titleParts(title: string): ParsedDialogTitle {
  let text = title;
  let selectedBadge: string | null = null;
  let stepBadge: string | null = null;
  let isCustomResponse = false;

  // 1. Detect and strip dumped CLI choices block (e.g. "\n\n○ Option 1...\n🔘 Other (type your own)")
  const cliOptionsMatch = text.match(/\n\n\s*(?:[○🔘\u25CB\u25CF\u25C9]|\([ xXoO•\*\-]?\)|\d+\.)[\s\S]*$/);
  if (cliOptionsMatch) {
    const dumped = cliOptionsMatch[0];
    if (dumped.includes("Other") || dumped.includes("Altro") || dumped.includes("🔘") || dumped.includes("(o)")) {
      isCustomResponse = true;
    }
    text = text.slice(0, cliOptionsMatch.index).trim();
  }

  // 2. Multi-choice selected badge: "(1 selected) ..."
  const selectedMatch = text.match(/^\((\d+)\s+(selected|selezionat[oi])\)\s*(.*)$/i);
  if (selectedMatch) {
    const count = selectedMatch[1];
    const isIt = selectedMatch[2]?.toLowerCase().startsWith("selezionat");
    selectedBadge = isIt ? `${count} selezionati` : `${count} selected`;
    text = selectedMatch[3] ?? "";
  }

  // 3. Question progress badge: e.g. "(1/2)", "(2/2)", "[1/3]", "Question 1 of 2:"
  const leadingStep = text.match(/^\s*(?:question\s+)?[\(\[](\d+)\s*(?:\/|di|of)\s*(\d+)[\)\]]\s*[:\-\.]?\s*/i);
  if (leadingStep) {
    stepBadge = `${leadingStep[1]} of ${leadingStep[2]}`;
    text = text.slice(leadingStep[0].length).trim();
  } else {
    const trailingStep = text.match(/\s*[\(\[](\d+)\s*(?:\/|di|of)\s*(\d+)[\)\]]\s*$/i);
    if (trailingStep) {
      stepBadge = `${trailingStep[1]} of ${trailingStep[2]}`;
      text = text.slice(0, trailingStep.index).trim();
    }
  }

  return {
    selectedBadge,
    stepBadge,
    isCustomResponse,
    text,
  };
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

/**
 * Stable key across round-trips of the same multi-choice question.
 */
export function questionKey(thread: string, title: string): string {
  const parts = titleParts(title);
  return `${thread}:${parts.text.trim()}`;
}

/**
 * Returns true if the dialog represents a multi-selection choice rather than single selection.
 */
export function isMultiSelectDialog(dialog: UiRequestSnapshot): boolean {
  if (dialog.optionDetails?.some((d) => d.selected !== undefined && d.selected !== null)) {
    return true;
  }
  if (dialog.options.some((opt) => isDoneOption(opt))) {
    return true;
  }
  if (/^\(\d+\s+(selected|selezionat[oi])\)/i.test(dialog.title)) {
    return true;
  }
  if (/\b(scegli|seleziona|choose|select|pick)\s+\d+|multi[-\s]?(choice|select)|multiple\b/i.test(dialog.title)) {
    return true;
  }
  return false;
}

/**
 * Returns true if this option is currently selected.
 */
export function isOptionSelected(
  dialog: UiRequestSnapshot,
  option: string,
  index: number,
  selectedSet?: Set<string>,
): boolean {
  // If the engine provided an explicit selected field in option details
  if (dialog.optionDetails?.[index]?.selected === true) {
    return true;
  }
  // If the option label itself was framed with a check prefix
  if (/^(\[[xX✓✔☑]\]|[✓✔☑])\s+/i.test(option)) {
    return true;
  }
  const clean = cleanOptionLabel(option);
  if (selectedSet && (selectedSet.has(clean) || selectedSet.has(option))) {
    return true;
  }
  return false;
}

/**
 * Load persisted selections from sessionStorage.
 */
export function loadSelectedOptions(): Record<string, string[]> {
  try {
    if (typeof sessionStorage === "undefined") return {};
    const raw = sessionStorage.getItem(STORAGE_KEY);
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

/**
 * Save current selections to sessionStorage.
 */
export function saveSelectedOptions(data: Record<string, Set<string>>): void {
  try {
    if (typeof sessionStorage === "undefined") return;
    const serializable: Record<string, string[]> = {};
    for (const [k, v] of Object.entries(data)) {
      if (v && v.size > 0) {
        serializable[k] = Array.from(v);
      }
    }
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify(serializable));
  } catch {
    // Ignore storage quota or disabled errors
  }
}
