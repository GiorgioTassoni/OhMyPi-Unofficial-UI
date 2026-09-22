/**
 * The extension chrome an engine pushes, as per-thread state (`protocol::ui::chrome`).
 *
 * Six ops arrive the same way and land in six different places: a notice, the thin line by the
 * composer, the monospace block above it, the thread's title, the composer's draft, and a URL.
 * Two of them are *state* — a status line and a widget stay up until the engine clears them —
 * and those two are what this module models, keyed by the thread that pushed them, so an
 * extension running in one session can never draw in another's column.
 *
 * The other four are acts the shell performs once. They are not folded in here, and returning
 * the same record for them is what keeps them off the render path: an extension that pushes a
 * title on every turn must not re-render every row.
 */

import type { ChromeOp } from "../bridge";

/** What one thread's chrome surfaces currently show. */
export interface ThreadChrome {
  /** The thin line near the composer; `null` when the extension cleared it. */
  status: string | null;
  /** The small monospace block above the composer; `null` when the extension cleared it. */
  widget: string[] | null;
}

/** What a thread shows before anything pushes to it. */
export const NO_CHROME: ThreadChrome = { status: null, widget: null };

/** The chrome one thread currently has. */
export function chromeFor(
  chrome: Record<string, ThreadChrome>,
  thread: string,
): ThreadChrome {
  return chrome[thread] ?? NO_CHROME;
}

/**
 * Fold one push in.
 *
 * A cleared surface is `null` rather than a removal: the thread keeps its slot in the record,
 * which is what lets the shell clear one surface without inventing the other.
 */
export function applyChrome(
  chrome: Record<string, ThreadChrome>,
  thread: string,
  op: ChromeOp,
): Record<string, ThreadChrome> {
  if (op.kind === "status") {
    return { ...chrome, [thread]: { ...chromeFor(chrome, thread), status: op.text } };
  }
  if (op.kind === "widget") {
    return { ...chrome, [thread]: { ...chromeFor(chrome, thread), widget: op.lines } };
  }

  return chrome;
}

/**
 * Whether an `editor-text` push may write the composer.
 *
 * Only the thread on screen. The message is the engine putting text in *the editor someone is
 * looking at*, and this window keeps one composer per open thread — so applying it to the
 * thread it names while another is on screen would silently overwrite a draft nobody can see,
 * which is the one way this op can destroy work. Case: the thread on screen, where the
 * engine's text **replaces** what is in the box; the draft that was there is not merged and
 * not kept, because "set the editor's text" is exactly that.
 */
export function draftTarget(activeId: string | null, thread: string): boolean {
  return activeId !== null && thread === activeId;
}
