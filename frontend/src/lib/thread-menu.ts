/**
 * The thread context menu (`docs/12` §2.3), as a pure function of what the app knows.
 *
 * Worth isolating because every item has a *reason* to be unavailable, and a menu that
 * offers an action the engine will refuse is worse than one that says why:
 *
 * - **Pin** is dispatched as the engine's own `/pin <id>` through a live thread, because the
 *   pin set is written under an engine-owned cross-process lock (measured: a native lock —
 *   abstract Unix sockets on Linux — that the app cannot take safely). With no thread open
 *   there is nobody to dispatch it, so the item says that rather than failing later.
 * - **Fork** and **Export** need a session *file*. A brand-new thread has none: measured
 *   engine behaviour, a session stays memory-only until it holds an assistant message, and
 *   the engine's own `fork()` refuses such a session.
 * - **Delete** is always offered. Its confirmation closes a live sidecar before removing
 *   the session, so the process cannot recreate the file after deletion.
 * - **Handoff** is refused by the engine while a turn streams, and it is not an export —
 *   it compacts in place and writes no file.
 */

export type MenuAction =
  | "rename"
  | "pin"
  | "export"
  | "fork"
  | "handoff"
  | "reveal"
  | "copy-cwd"
  | "stop"
  | "delete";

export interface MenuItem {
  action: MenuAction;
  label: string;
  /** Why it is unavailable, shown as the item's title. Empty when it is available. */
  reason: string;
  disabled: boolean;
  /** A destructive action, which the menu renders apart and the caller confirms. */
  destructive: boolean;
}

export interface ThreadContext {
  /** The catalogue row, or `null` for a thread the engine has not written to disk yet. */
  session: {
    pinned: boolean;
    cwd: string;
    path: string;
  } | null;
  /** A sidecar is running for this thread. */
  live: boolean;
  /** A turn is streaming in it. */
  streaming: boolean;
  /** Some thread has a sidecar, so an RPC command can be dispatched somewhere. */
  anyLive: boolean;
}

export function menuFor(context: ThreadContext): MenuItem[] {
  const hasFile = context.session !== null;
  const cwd = context.session?.cwd ?? "";
  const pinned = context.session?.pinned ?? false;

  const item = (
    action: MenuAction,
    label: string,
    enabled: boolean,
    reason: string,
    destructive = false,
  ): MenuItem => ({ action, label, reason: enabled ? "" : reason, disabled: !enabled, destructive });

  return [
    item("rename", "Rename…", true, ""),
    item(
      "pin",
      pinned ? "Unpin" : "Pin",
      context.anyLive,
      "open a thread first — the engine writes its own pin set",
    ),
    item("export", "Export HTML…", hasFile, "this thread has no session file yet"),
    item("fork", "Fork from here…", hasFile, "this thread has no session file to fork yet"),
    item(
      "handoff",
      "Hand off…",
      context.live && !context.streaming,
      context.live ? "a turn is running" : "this thread has no sidecar",
    ),
    item("reveal", "Show in file manager", cwd !== "", "this session recorded no directory"),
    item("copy-cwd", "Copy working directory", cwd !== "", "this session recorded no directory"),
    item("stop", "Stop sidecar", context.live, "no sidecar is running"),
    item(
      "delete",
      "Delete conversation…",
      true,
      "",
      true,
    ),
  ];
}

/** The actions the menu hides entirely, because offering them would only ever say no. */
export function visibleMenu(context: ThreadContext): MenuItem[] {
  return menuFor(context).filter((entry) => entry.action !== "stop" || context.live);
}
