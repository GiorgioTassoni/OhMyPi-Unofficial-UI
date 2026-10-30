/**
 * The sidebar's model (`docs/12` §2), as a pure function of what the host reported.
 *
 * Two sources meet here and they are deliberately different things:
 *
 * - **The catalogue** — every session the engine has on disk, read by the host from the
 *   store. It knows titles, lifecycles and when each file last changed, and nothing about
 *   whether anything is running.
 * - **The live set** — the threads with a sidecar right now. It knows streaming, pending
 *   approvals and failures, and nothing about sessions that are not open.
 *
 * The sidebar is their union, because neither alone is the truth: a session with no
 * process must still be listed (that is what resuming is for), and a *brand-new* thread
 * has no file at all yet — measured engine behaviour: a session stays memory-only until it
 * contains an assistant message, so a window that only listed disk sessions would show an
 * empty sidebar right after the user opened one.
 *
 * Everything here is app-owned. The engine has no unread concept, no notion of which
 * project the user is looking at, and no opinion about nesting beyond `parentSession` in a
 * forked session's header.
 */

import type { SessionSummary, ThreadSnapshot } from "../bridge";

/** What the row's single status dot means (`docs/12` §2.2). */
export type ThreadDot = "cold" | "idle" | "streaming" | "attention" | "error";

/** WorkBuddy-style minimal LED kind for session-active statuses. */
export type ThreadLedKind = "streaming" | "attention" | "error";

export function ledFor(dot: ThreadDot, live: boolean): ThreadLedKind | null {
  if (dot === "streaming") return "streaming";
  if (dot === "attention") return "attention";
  if (live && dot === "error") return "error";
  return null;
}

export interface ThreadRow {
  id: string;
  /** The name to show: the engine's title, else the first message, else a placeholder. */
  title: string;
  /** Initial prompt text or excerpt when available. */
  firstMessage?: string;
  /** Lifecycle status reported by the session store, if any. */
  status?: string;
  /** Whether a sidecar is running for this thread. */
  live: boolean;
  dot: ThreadDot;
  /** WorkBuddy-style minimal LED status (green bounce for streaming, orange for attention, red for in-session error, else null). */
  led: ThreadLedKind | null;
  /** App-owned: a turn finished while the user was looking elsewhere. */
  unread: boolean;
  /**
   * A cold session's unfinished state, as a word to show beside the row, or `null`.
   *
   * Separate from the dot because they answer different questions: the dot is "what is
   * happening now" (and a cold thread has nothing happening), while this is "what happened
   * last time" — an interrupted or failed session is worth a word even when it is closed.
   */
  note: string | null;
  /**
   * The host released this thread's sidecar (`docs/11` §3.1), so opening it resumes rather
   * than selects.
   *
   * The host's answer, not ours: the id set it keeps is the only thing that knows the process
   * was released *by the idle policy* rather than never started. A thread that is live is
   * never suspended whatever that set says — a process is the fact the label would deny.
   */
  suspended: boolean;
  /** The parent session id, when this thread was forked from another. */
  parentId: string | null;
  /** 0 for a top-level thread, 1 for one indented under its parent (`docs/12` §6.3). */
  depth: number;
  /** Epoch milliseconds of the last activity; used for ordering. */
  modifiedAt: number;
  messageCount: number;
  pinned: boolean;
  /** The project this row is grouped under; empty when the session recorded no cwd. */
  project: string;
}

export interface ProjectGroup {
  /** The working directory, or empty for sessions that recorded none. */
  path: string;
  /** The last path segment — what `docs/12` §1 shows in the sidebar. */
  name: string;
  threads: ThreadRow[];
}

export interface SidebarInput {
  sessions: SessionSummary[];
  live: ThreadSnapshot[];
  unread: string[];
  /** Projects the engine's registry marks hidden (`projects.json`). */
  hidden: string[];
  /** Custom user project ordering, preserved regardless of last modified timestamps. */
  projectOrder?: string[];
  /** Epoch milliseconds, injected rather than read: a synthesised row needs a time and a
   * pure function cannot ask the clock. */
  now: number;
}

/** The engine's own display-name chain (`sessionDisplayName`), so a row's label matches
 * the resume picker's. */
export function displayName(
  title: string | null,
  firstMessage: string,
  now: number,
  created: string | null,
): string {
  const trimmed = (value: string | null | undefined): string | null => {
    if (!value) return null;
    const line = value.split(/\r?\n/, 1)[0]?.trim() ?? "";
    return line.length > 0 ? line : null;
  };

  const named = trimmed(title);
  if (named) return named;

  const first = trimmed(firstMessage);
  if (first && first !== "(no messages)") return first;

  const at = created ? Date.parse(created) : Number.NaN;
  const time = new Date(Number.isFinite(at) ? at : now);
  const clock = time.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  return `Untitled · ${clock}`;
}

/** The dot for one row, from the live set only: a thread with no process has no state to
 * report, which is exactly what a hollow dot says. */
export function dotFor(thread: ThreadSnapshot | undefined): ThreadDot {
  if (!thread) return "cold";
  if (thread.pendingApprovals > 0) return "attention";
  if (thread.error) return "error";
  if (thread.streaming) return "streaming";
  return "idle";
}

/** The word beside a cold row, from the engine's lifecycle. `complete` and `unknown` say
 * nothing worth a label. */
function noteFor(status: string, live: boolean): string | null {
  if (live) return null;
  switch (status) {
    case "interrupted":
      return "interrupted";
    case "aborted":
      return "aborted";
    case "error":
      return "failed";
    case "pending":
      return "unanswered";
    default:
      return null;
  }
}

/** The group a project path belongs to. */
export function projectName(path: string): string {
  if (!path) return "No project";
  const segments = path.replace(/[/\\]+$/, "").split(/[/\\]/);
  return segments[segments.length - 1] || path;
}

/**
 * Build the sidebar: projects, each with its threads, parents followed by their children.
 *
 * Sessions are grouped by the **cwd they recorded**, not by the directory the engine filed
 * them in: the engine's bucket names encode a path lossily (`-tmp-omp-8d-app` could be
 * `/tmp/omp-8d-app` or `~/tmp/omp/8d/app`), and every header carries the real path anyway.
 */
export function sidebarModel(input: SidebarInput): ProjectGroup[] {
  const unread = new Set(input.unread);
  const liveById = new Map(input.live.map((thread) => [thread.id, thread]));

  const rows: ThreadRow[] = input.sessions.map((session) => {
    const thread = liveById.get(session.id);
    const dot = dotFor(thread);
    return {
      id: session.id,
      // A live RPC session can announce a generated title before the next disk catalogue
      // scan. Prefer that current engine value; cold sessions still read the persisted slot.
      title: displayName(
        thread?.title ?? session.title,
        session.firstMessage,
        input.now,
        session.createdAt,
      ),
      live: thread !== undefined,
      dot,
      led: ledFor(dot, thread !== undefined),
      unread: unread.has(session.id),
      note: noteFor(session.status, thread !== undefined),
      suspended: session.suspended && thread === undefined,
      parentId: session.parentId,
      depth: 0,
      modifiedAt: session.modifiedAt,
      messageCount: session.messageCount,
      pinned: session.pinned,
      project: session.cwd,
      firstMessage: session.firstMessage,
      status: session.status,
    };
  });

  // A thread that exists only in memory has nothing on disk to be listed from. Its id is
  // the row's identity, so it is also the key the rest of the UI uses to talk to it.
  const known = new Set(rows.map((row) => row.id));
  for (const thread of input.live) {
    if (known.has(thread.id)) continue;
    const dot = dotFor(thread);
    rows.push({
      id: thread.id,
      title: displayName(thread.title, "", input.now, null),
      live: true,
      dot,
      led: ledFor(dot, true),
      unread: unread.has(thread.id),
      note: null,
      // A session the engine has not written to disk yet has never been released by anything.
      suspended: false,
      parentId: null,
      depth: 0,
      modifiedAt: input.now,
      messageCount: 0,
      pinned: false,
      project: thread.workspace,
      firstMessage: "",
      status: thread.error ? "error" : "live",
    });
  }

  const groups = new Map<string, ThreadRow[]>();
  for (const row of rows) {
    const key = row.project;
    const bucket = groups.get(key);
    if (bucket) bucket.push(row);
    else groups.set(key, [row]);
  }

  const hidden = new Set(input.hidden);
  const unhidden = [...groups.entries()]
    .filter(([path]) => !hidden.has(path))
    .map(([path, threads]) => ({ path, name: projectName(path), threads: nest(threads) }));

  if (input.projectOrder && input.projectOrder.length > 0) {
    const orderMap = new Map(input.projectOrder.map((path, idx) => [path, idx]));
    return unhidden.sort((a, b) => {
      const idxA = orderMap.get(a.path);
      const idxB = orderMap.get(b.path);
      if (idxA !== undefined && idxB !== undefined) return idxA - idxB;
      if (idxA !== undefined) return -1;
      if (idxB !== undefined) return 1;
      return a.name.localeCompare(b.name);
    });
  }

  return unhidden.sort((a, b) => a.name.localeCompare(b.name));
}

/**
 * Pure reordering helper for drag-and-drop lists.
 * Given a list and source/target positions, returns a new list with the element relocated.
 */
export function reorderItems<T>(
  list: readonly T[],
  fromIndex: number,
  toIndex: number,
  position: "above" | "below",
): T[] {
  if (
    fromIndex < 0 ||
    fromIndex >= list.length ||
    toIndex < 0 ||
    toIndex >= list.length ||
    fromIndex === toIndex
  ) {
    return [...list];
  }

  const result = [...list];
  const [moved] = result.splice(fromIndex, 1);
  let insertAt = toIndex;
  if (fromIndex < toIndex) {
    insertAt = position === "below" ? toIndex : toIndex - 1;
  } else {
    insertAt = position === "below" ? toIndex + 1 : toIndex;
  }
  insertAt = Math.max(0, Math.min(insertAt, result.length));
  result.splice(insertAt, 0, moved);
  return result;
}

export const PROJECT_ORDER_STORAGE_KEY = "omp:projects-order";

export function loadProjectOrder(
  storage: Storage | undefined = typeof localStorage !== "undefined" ? localStorage : undefined,
): string[] {
  if (!storage) return [];
  try {
    const raw = storage.getItem(PROJECT_ORDER_STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed)
      ? parsed.filter((item): item is string => typeof item === "string")
      : [];
  } catch {
    return [];
  }
}

export function saveProjectOrder(
  paths: readonly string[],
  storage: Storage | undefined = typeof localStorage !== "undefined" ? localStorage : undefined,
): void {
  if (!storage) return;
  try {
    storage.setItem(PROJECT_ORDER_STORAGE_KEY, JSON.stringify(paths));
  } catch {
    // Ignore storage quota
  }
}

/** Newest first, pinned first; a fork sits directly under the thread it came from. */
function nest(rows: ThreadRow[]): ThreadRow[] {
  const ordered = [...rows].sort(compare);
  const byId = new Map(ordered.map((row) => [row.id, row]));
  const children = new Map<string, ThreadRow[]>();

  for (const row of ordered) {
    const parent = row.parentId ? byId.get(row.parentId) : undefined;
    // An orphaned fork (its parent was deleted, or lives in another project) stays at the
    // top level rather than disappearing behind a parent that is not on screen.
    if (!parent || parent.id === row.id) continue;
    const bucket = children.get(parent.id);
    if (bucket) bucket.push(row);
    else children.set(parent.id, [row]);
  }

  const nested: ThreadRow[] = [];
  const seen = new Set<string>();
  /**
   * Depth-first, so a fork of a fork stays under its own parent instead of vanishing.
   *
   * `seen` is not decoration: these ids come from files on disk, and a pair of sessions that
   * name each other as parent would otherwise walk forever. A row is emitted once, at the
   * depth it was first reached.
   */
  const emit = (row: ThreadRow, depth: number): void => {
    if (seen.has(row.id)) return;
    seen.add(row.id);
    nested.push(depth === 0 ? row : { ...row, depth });
    for (const child of children.get(row.id) ?? []) emit(child, depth + 1);
  };

  for (const row of ordered) {
    if (row.parentId && byId.has(row.parentId) && row.parentId !== row.id) continue;
    emit(row, 0);
  }
  // Nothing above is emitted when two rows name each other as parent — neither is a
  // top-level row — so anything still unseen is listed flat rather than disappearing.
  for (const row of ordered) emit(row, 0);
  return nested;
}

function compare(a: ThreadRow, b: ThreadRow): number {
  if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
  return b.modifiedAt - a.modifiedAt || a.id.localeCompare(b.id);
}

/**
 * Whether an error indicates that the sidecar process for a thread is not currently running.
 *
 * During thread shutdown, sidecar restart, or after stopping a thread, backend IPC commands
 * that require a live process answer with `no live thread <id>`. This is normal lifecycle state,
 * not an unexpected error that should be surfaced as a user-facing failure.
 */
export function isNoLiveThreadError(cause: unknown): boolean {
  if (typeof cause === "string") {
    return cause.includes("no live thread");
  }
  if (cause instanceof Error) {
    return cause.message.includes("no live thread");
  }
  return String(cause).includes("no live thread");
}

/** Format modified epoch milliseconds as concise relative time (e.g. 'now', '5m', '2h', '3d'). */
export function formatRelativeTime(modifiedAt: number, now = Date.now()): string {
  if (!modifiedAt || modifiedAt <= 0) return "";
  const diffSec = Math.max(0, Math.floor((now - modifiedAt) / 1000));
  if (diffSec < 60) return "now";
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m`;
  const diffHour = Math.floor(diffMin / 60);
  if (diffHour < 24) return `${diffHour}h`;
  const diffDay = Math.floor(diffHour / 24);
  if (diffDay < 30) return `${diffDay}d`;
  return `${Math.floor(diffDay / 30)}mo`;
}

/** Format modified timestamp as friendly descriptive relative time for hover cards. */
export function formatFriendlyTime(modifiedAt: number, now = Date.now()): string {
  if (!modifiedAt || modifiedAt <= 0) return "";
  const diffSec = Math.max(0, Math.floor((now - modifiedAt) / 1000));
  if (diffSec < 45) return "Just now";
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin === 1) return "1 min ago";
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHour = Math.floor(diffMin / 60);
  if (diffHour === 1) return "1 hour ago";
  if (diffHour < 24) return `${diffHour}h ago`;
  const diffDay = Math.floor(diffHour / 24);
  if (diffDay === 1) return "Yesterday";
  if (diffDay < 7) return `${diffDay}d ago`;
  return new Date(modifiedAt).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
}

/** Format timestamp into precise human readable string (e.g. 'Oct 29, 23:45'). */
export function formatFullDate(timestamp: number): string {
  if (!timestamp || timestamp <= 0) return "";
  const date = new Date(timestamp);
  return date.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

