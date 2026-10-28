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

/**
 * Parses the dialog title to extract an optional `N selected` badge and clean title text.
 */
export function titleParts(title: string): { selectedBadge: string | null; text: string } {
  const match = title.match(/^\((\d+)\s+(selected|selezionat[oi])\)\s*(.*)$/i);
  if (match) {
    const count = match[1];
    const isIt = match[2]?.toLowerCase().startsWith("selezionat");
    return {
      selectedBadge: isIt ? `${count} selezionati` : `${count} selected`,
      text: match[3] ?? "",
    };
  }
  return {
    selectedBadge: null,
    text: title,
  };
}

/**
 * Stable key across round-trips of the same multi-choice question.
 */
export function questionKey(thread: string, title: string): string {
  const base = title
    .replace(/^\((\d+)\s+(selected|selezionat[oi])\)\s*/i, "")
    .trim();
  return `${thread}:${base}`;
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
