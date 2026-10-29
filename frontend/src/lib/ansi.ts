/**
 * Utilities for cleaning and formatting terminal output / extension chrome strings.
 */

// Matches ANSI escape sequences, including OSC and CSI, even if control chars were partially converted
const ANSI_REGEX =
  /(?:\x1b|\u001b|\ufffd|[\u0000-\u001f]|▯)?\[[0-9;]*[a-zA-Z]|(?:\x1b|\u001b|\ufffd|[\u0000-\u001f]|▯)\][^\x07\x1b]*(?:\x07|\x1b\\)/g;

/**
 * Strips ANSI escape sequences and control markers from terminal-sourced strings.
 */
export function stripAnsi(text: string): string {
  if (!text) return "";
  return text.replace(ANSI_REGEX, "").trim();
}
