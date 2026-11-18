import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export { invoke, type UnlistenFn };

/** Subscribe to a typed Tauri event and forward its payload directly. */
export async function listenPayload<T>(
  event: string,
  handler: (payload: T) => void,
): Promise<UnlistenFn> {
  return listen<T>(event, (e) => handler(e.payload));
}

/** Linux native fallback, used only for an explicit image paste. */
export async function clipboardImage(): Promise<Blob | null> {
  const data = await invoke<string | null>("clipboard_image");
  if (data === null) return null;
  const bytes = Uint8Array.from(atob(data), (character) => character.charCodeAt(0));
  return new Blob([bytes], { type: "image/png" });
}

export interface SearchHit {
  thread: string;
  kind: string;
  ordinal: number;
  text: string;
}

/** How far the app's own search index has got (`docs/12` §7.4). */
export interface IndexStatus {
  /** Sessions in the index. */
  indexed: number;
  /** Sessions in the store: the denominator of "indexing 12/34". */
  total: number;
  /** A scan is in flight. */
  running: boolean;
  /** Epoch milliseconds of the last completed scan, or null if there has never been one. */
  completedAt: number | null;
  /**
   * Why the last pass did not get to the end, in the engine's own sentence.
   *
   * The overlay's footer is where "search found nothing" is read, and a corrupt index is a
   * different problem with a different answer than an empty store.
   */
  error: string | null;
}

/**
 * Search every session the app has indexed.
 *
 * The app's own index, not the engine's: at v18.2.6 the engine has no cross-thread search at
 * all (`history.db` indexes prompts, not message text or tool activity), and a cold session
 * has no sidecar to ask — so the host reads the store with `omp-store` and keeps a SQLite FTS
 * index of its own (decision D7).
 */
export async function search(query: string, limit?: number): Promise<SearchHit[]> {
  return invoke<SearchHit[]>("search", { query, limit: limit ?? null });
}

/** The index's progress, for the overlay's footer. */
export async function indexStatus(): Promise<IndexStatus> {
  return invoke<IndexStatus>("index_status");
}

/** Ask for a scan now, rather than waiting for the next one. */
export async function reindex(): Promise<void> {
  return invoke<void>("reindex");
}

/** Subscribe to index progress, which arrives while a scan runs and when it finishes. */
export async function onIndexProgress(
  handler: (status: IndexStatus) => void,
): Promise<UnlistenFn> {
  return listenPayload<IndexStatus>("index-progress", handler);
}

/** One user message a fork can start from (`get_branch_messages`). */

export async function pickDirectory(title?: string, defaultPath?: string): Promise<string | null> {
  return invoke<string | null>("pick_directory", {
    title: title ?? null,
    defaultPath: defaultPath ?? null,
  });
}

/** Open a file or directory in the user's default application, under the host's allowlist. */
export async function openPath(path: string): Promise<void> {
  return invoke<void>("open_path", { path });
}

export async function toggleDevtools(): Promise<void> {
  return invoke<void>("toggle_devtools");
}

/**
 * Delete a session: its file and its artifacts directory.

/**
 * Open a URL in the user's default application.
 *
 * The host decides whether it will: the allowlist for what a model-authored link
 * may launch lives in Rust (`external.rs`), and a refusal comes back here as a
 * message worth showing rather than as a silently dead click.
 */
export async function openExternal(url: string): Promise<void> {
  await invoke("open_external", { url });
}

/**
 * Ask the host to raise an operating-system notification.
 *
 * The host's command and not the plugin's JavaScript, whose `sendNotification` is a bare
 * `new window.Notification(...)`: nothing in that plugin installs the web global it needs, so a
 * webview without one fails silently — and the fact is still delivered in-app, so the failure
 * has nowhere to show up. The command is a real API on every platform.
 */
export async function notifyOs(title: string, body: string): Promise<void> {
  return invoke<void>("notify_os", { title, body });
}

/** Ask the host to play a system chime for notifications. */
export async function playSystemSound(kind: string): Promise<void> {
  return invoke<void>("play_notification_sound", { kind });
}
