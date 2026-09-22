/**
 * What the host's notifications mean for the window (`docs/12` §13).
 *
 * The split is the point of this module. The **host** reports facts — a turn ended, a dialog
 * arrived, retries ran out, a job left the broker's list — and says nothing about who should be
 * interrupted. The **window** decides, in one place, so that a rule can be read rather than
 * discovered in a handler: which facts become an unread ring on a row, which become an in-app
 * notice, and which are pushed to the operating system. Nothing here is anything but a
 * function of what the host said and what the window is showing.
 *
 * The same predicate serves both sources, because a notification and an extension's
 * `notify` op are the same *act* — "this thread has something to say" — and a rule that
 * applied to only one of them would be a rule half the window follows.
 */

import type { NotificationEvent } from "../bridge";

/** A notice the window shows in-app, carrying the host's own words. */
export interface Notice {
  /** Stable key: the list is keyed by it and a dismiss addresses it by id. */
  id: string;
  /** The thread its "open" control reveals, or null for an app-level action receipt. */
  thread: string | null;
  title: string;
  body: string;
  /** How loudly it reads. Only the tint; the sentences are never ours. */
  tone: Tone;
}

/** `info` for an ordinary fact, `warn` for something waiting on the user, `error` for a failure. */
export type Tone = "info" | "warn" | "error";

/**
 * Whether this fact should mark its thread unread.
 *
 * Only a finished turn: that is what the reference's grey ring means ("finished while you were
 * elsewhere"), and it is a *completion*, which is why a turn that failed after exhausting its
 * retries marks its row through the failed turn's own `turn-finished` rather than twice here.
 * The thread on screen is never unread — the user is watching it finish.
 *
 * This is the single source of truth for unread marks. The window used to derive them from the
 * live set's `streaming` flag going false (`threads.ts`'s old `newlyUnread`), which needed the
 * previous snapshot and could only guess at *why* a thread went quiet; the host's own
 * completion rule (`agent_end` with `isTerminal !== false`) does not have to guess, so the
 * derivation is gone rather than kept beside this.
 */
export function marksUnread(event: NotificationEvent, activeId: string | null): boolean {
  return event.kind === "turn-finished" && event.thread !== activeId;
}

/** Whether this fact should raise an in-app notice. The thread on screen never does: it just
 * changed in front of the user, who can see it. */
export function showsNotice(thread: string, activeId: string | null): boolean {
  return thread !== activeId;
}

/**
 * Whether the operating system should be asked to interrupt about this thread.
 *
 * Two conditions, and both are deliberate. Unfocused, because a notification over the window
 * the user is already looking at is noise. And not the thread on screen — even unfocused —
 * because the row, the ring and the notice in the window already carry it: the moment a
 * person comes back, the window must be the thing that tells them, not a stack of system
 * banners about the column that was open all along.
 */
export function shouldNotifyOs(
  thread: string,
  activeId: string | null,
  focused: boolean,
): boolean {
  return thread !== activeId && !focused;
}

/** The notice for one reported notification: the host's title and body, verbatim. */
export function noticeFor(event: NotificationEvent, id: string): Notice {
  return {
    id,
    thread: event.thread,
    title: event.title,
    body: event.body,
    tone: event.kind === "failed" ? "error" : event.kind === "needs-you" ? "warn" : "info",
  };
}

/**
 * The notice for an extension's `notify` op.
 *
 * The op carries a message and a level and no title, so the thread's own name — the one the
 * row already shows — titles it. That is the host's naming, not a summary we invented.
 */
export function noticeFromChrome(
  thread: string,
  name: string,
  message: string,
  level: string | null,
  id: string,
): Notice {
  return { id, thread, title: name, body: message, tone: toneForLevel(level) };
}

/** An app action completed; it has nothing to open, but shares the same toast surface. */
export function noticeForAction(message: string, id: string): Notice {
  return { id, thread: null, title: "Done", body: message, tone: "info" };
}

/** The engine's own level word, mapped to the three tones the window tints with. */
export function toneForLevel(level: string | null): Tone {
  switch ((level ?? "").toLowerCase()) {
    case "error":
    case "fatal":
      return "error";
    case "warn":
    case "warning":
      return "warn";
    default:
      return "info";
  }
}

/**
 * Add a notice to the stack, keeping the newest `limit`.
 *
 * Capped because these are unbounded: a background job finishing can raise one long after the
 * window stopped being watched, and a stack that grows without end covers the conversation it
 * is telling you about.
 */
export function addNotice(notices: Notice[], notice: Notice, limit: number): Notice[] {
  return [...notices, notice].slice(-limit);
}

/** Remove one notice by id. An id that is not there leaves the stack alone. */
export function dropNotice(notices: Notice[], id: string): Notice[] {
  return notices.filter((notice) => notice.id !== id);
}
