/**
 * The typed bridge to the Rust host.
 *
 * Everything the UI knows about the backend goes through this module: command
 * names, payload shapes, and the event subscription. Components import functions
 * from here rather than calling `invoke` directly, so a rename lands in one file
 * and the payload types stay honest against `src-tauri/src/dto.rs`.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Mirrors `dto::ReadySnapshot`. */
export interface ReadySnapshot {
  protocolVersion: number;
  supportedProtocolVersions: number[];
  maxFrameBytes: number;
  maxReassembledFrameBytes: number;
  negotiatedV2: boolean;
}

/** Mirrors `dto::ModelSnapshot`. */
export interface ModelSnapshot {
  provider: string | null;
  id: string | null;
  name: string | null;
}

/** Mirrors `dto::ContextSnapshot`. */
export interface ContextSnapshot {
  tokens: number;
  contextWindow: number;
  percent: number;
}

/** Mirrors `dto::ControlSnapshot`. */
export interface ControlSnapshot {
  sessionId: string;
  sessionName: string | null;
  model: ModelSnapshot | null;
  thinkingLevel: string | null;
  isStreaming: boolean;
  isCompacting: boolean;
  messageCount: number;
  queuedMessageCount: number;
  context: ContextSnapshot | null;
  /** The engine's todo list, as `get_state.todoPhases` reported it. */
  todoPhases: TodoPhaseSnapshot[];
  transcriptRows: number;
  /** `get_state.autoCompactionEnabled`; null when the engine did not say. */
  autoCompactionEnabled: boolean | null;
}

/** Mirrors `dto::CounterSnapshot`. */
export interface CounterSnapshot {
  eventsSeen: number;
  framesSeen: number;
  eventKinds: number;
  unknownEvents: number;
  malformedEvents: number;
  laggedEvents: number;
  uiRequests: number;
  blockingUiRequests: number;
}

/** Mirrors `dto::SessionStatus`. */
export interface SessionStatus {
  binary: string;
  workspace: string;
  sidecarPid: number | null;
  ready: ReadySnapshot;
  control: ControlSnapshot;
  counters: CounterSnapshot;
  /** The approval mode the host launched this sidecar with (`--approval-mode`). */
  approvalMode: string | null;
}

/** Mirrors `dto::ToolSnapshot`. */
export interface ToolSnapshot {
  toolCallId: string;
  toolName: string;
  intent: string | null;
  /** The engine's arguments as JSON text, empty when there were none. */
  args: string;
  /**
   * The engine's structured payload as JSON text, empty when there was none.
   *
   * Where a card finds what `output` does not say: `edit` carries its unified
   * diff here, and a truncated result carries the artifact record.
   */
  details: string;
  /** The result as displayable text; empty until the call ends. */
  output: string;
  isError: boolean;
  finished: boolean;
}

/** Mirrors `dto::AttachmentSnapshot`. */
export interface AttachmentSnapshot {
  /** `image/png`, `image/jpeg`, … as the engine named it. */
  mimeType: string;
  /** Base64 bytes, without a `data:` prefix. */
  data: string;
}

/** Mirrors `dto::RowSnapshot`. */
export interface RowSnapshot {
  /** `user`, `assistant`, `tool`, or `notice:<level>`. */
  role: string;
  text: string;
  thinking: string | null;
  streaming: boolean;
  tool: ToolSnapshot | null;
  /** Images attached to the message; empty for every row that is not one. */
  attachments: AttachmentSnapshot[];
  /**
   * The engine's own type for a message the app did not write: `async-result` for a
   * background job's delivered result (`docs/12` §9). `null` for ordinary messages.
   */
  customType: string | null;
  /** The jobs this row accounts for — a delivery's, and empty for everything else. */
  jobs: JobDeliverySnapshot[];
}

/**
 * Mirrors `dto::RowPatch`: the conversation replaced from `from` onward.
 *
 * One operation for every change — append, streaming rewrite, a card settling, a
 * reset — because a delta protocol would need this side to track insertions and
 * removals to stay aligned.
 */
export interface RowPatch {
  from: number;
  rows: RowSnapshot[];
}

/** Mirrors `dto::ActivitySnapshot`. */
export interface ActivitySnapshot {
  kind: string;
  sequence: number;
}

/** Mirrors `dto::UiRequestSnapshot`. */
export interface UiRequestSnapshot {
  id: string;
  /** The wire method: `select`, `confirm`, `input` or `editor`. */
  kind: string;
  title: string;
  message: string;
  options: string[];
  prefill: string | null;
  placeholder: string | null;
  timeoutMs: number | null;
}

/**
 * Mirrors `PolicyOutcome` (`src-tauri/src/policy.rs`): what an allow-write changed.
 *
 * The outcome exists so the dialog can report the truth instead of a promise: the
 * write is not live (`docs/12` §10), so the UI needs to know it happened, for which
 * tool, and what was in force before.
 */
export interface PolicyOutcome {
  tool: string;
  /** Always `"allow"` today; the record's value, read back. */
  policy: string;
  /** The policy that was in force before, when there was one. */
  previous: string | null;
}

/**
 * Mirrors `dto::UiAnswer`: exactly one of `value`, `confirmed` or `cancelled`.
 *
 * The host refuses an answer carrying none of them, so this is not a place to
 * guess — a `select` answered with an empty choice would leave the agent waiting
 * for a decision that never arrives.
 */
export interface UiAnswer {
  value?: string;
  confirmed?: boolean;
  cancelled?: boolean;
  timedOut?: boolean;
}

/** Mirrors `dto::ModelOption`: one catalogue row, the subset a picker renders. */
export interface ModelOption {
  provider: string;
  id: string;
  name: string;
  contextWindow: number | null;
  reasoning: boolean;
  /** `thinking.defaultLevel` — the model's own suggestion, not the session's level. */
  defaultEffort: string | null;
  /** `thinking.efforts` — the levels this model accepts; empty when it does not think. */
  efforts: string[];
}

/**
 * Mirrors `dto::ModelCatalogue`: the host's cache, never a fetch.
 *
 * `options` can be empty on a first ever run, which is why `refreshing` exists — the
 * picker shows that it is fetching rather than an empty catalogue (`docs/12` §7.1).
 */
export interface ModelCatalogue {
  options: ModelOption[];
  /** Unix milliseconds of the last successful fetch; null until one lands. */
  fetchedAt: number | null;
  refreshing: boolean;
}

/** Mirrors `dto::LaunchContext`. */
export interface LaunchContext {
  workspace: string | null;
  /** The OS user, for the sidebar's identity row. `null` when the platform will not say. */
  user: string | null;
  /** The app's own version. */
  version: string;
}

/**
 * A thread's live state, as the sidebar's dot needs it (`docs/12` §2.2).
 *
 * A thread's id **is** the engine's session id: the sidebar lists on-disk sessions by that
 * id, and a live thread is one that resumed one of them (or a brand-new session the engine
 * has not written to disk yet).
 */
export interface ThreadSnapshot {
  id: string;
  workspace: string;
  title: string | null;
  /** A turn is streaming. */
  streaming: boolean;
  /** Dialogs the agent is waiting on — the app's safety boundary, so it outranks the rest. */
  pendingApprovals: number;
  /** The last turn failed. */
  error: string | null;
}

/** Mirrors `dto::SessionSummaryDto`: one session the engine has on disk. */
export interface SessionSummary {
  id: string;
  path: string;
  /** The engine's directory name for this session. A hint, not a path. */
  bucket: string;
  /** The working directory the session recorded. Empty for old sessions. */
  cwd: string;
  title: string | null;
  /** `auto` or `user`: the engine never overwrites a user title with an automatic one. */
  titleSource: string | null;
  /** The parent session id, when this session was forked. */
  parentId: string | null;
  /** The engine's ISO-8601 creation stamp, as written. */
  createdAt: string | null;
  /** File mtime in epoch milliseconds — what the sidebar sorts by. */
  modifiedAt: number;
  messageCount: number;
  size: number;
  firstMessage: string;
  /** `complete`, `interrupted`, `aborted`, `error`, `pending` or `unknown`. */
  status: string;
  pinned: boolean;
  /**
   * The host released this session's sidecar to save its ~200 MB and will start a new one on
   * demand (`docs/11` §3.1). App-owned, and the only honest source for it: "no process right
   * now" is not something a session file can say.
   */
  suspended: boolean;
}

/** Mirrors `dto::ProjectSnapshot`: a working directory with sessions in it. */
export interface ProjectSnapshot {
  path: string;
  name: string;
  sessionCount: number;
  hidden: boolean;
}

/**
 * One session-scoped event.
 *
 * Every event about a session carries the thread it belongs to, because the app holds
 * several live at once (D5) and one channel per event would need a listener per thread.
 * The model catalogue is the exception: it is a property of the engine, not of a session.
 */
export interface ThreadEvent<T> {
  thread: string;
  payload: T;
}

/**
 * Open a thread: spawn a sidecar in `workspace`, handshake, and register it.
 *
 * `resume` is a session **id** from the catalogue, not a path and not a prefix: the host
 * resolves it against the store and passes the resolved absolute path to the engine, which
 * would otherwise take a mistyped path verbatim and quietly start an empty session in its
 * place (measured at v18.2.6).
 */
export async function openThread(workspace: string, resume?: string): Promise<ThreadSnapshot> {
  return invoke<ThreadSnapshot>("open_thread", { workspace, resume: resume ?? null });
}

/**
 * Tell the host which thread is on screen.
 *
 * The idle policy's only guard the host cannot derive for itself (`docs/14` step 14): a thread
 * someone is reading is never released from under them, however long it sits idle. `null` means
 * no thread in particular — a closed column, a window with nothing selected.
 */
export function setFocusedThread(thread: string | null): Promise<void> {
  return invoke<void>("set_focused_thread", { thread });
}

/** Every thread with a sidecar right now. */
export async function threads(): Promise<ThreadSnapshot[]> {
  return invoke<ThreadSnapshot[]>("threads");
}

/** Close one thread's sidecar, letting the engine exit cleanly. Other threads stay up. */
export async function closeThread(thread: string): Promise<void> {
  return invoke<void>("close_thread", { thread });
}

/** Every session the engine has on disk, newest first. Works with no thread open. */
export async function sessions(): Promise<SessionSummary[]> {
  return invoke<SessionSummary[]>("sessions");
}

/** The working directories the store knows, with their session counts. */
export async function projects(): Promise<ProjectSnapshot[]> {
  return invoke<ProjectSnapshot[]>("projects");
}

/** What the app was launched with. */
export async function launchContext(): Promise<LaunchContext> {
  return invoke<LaunchContext>("launch_context");
}

/** Re-read one thread's status. */
export async function threadStatus(thread: string): Promise<SessionStatus> {
  return invoke<SessionStatus>("thread_status", { thread });
}

/** One thread's conversation so far. */
export async function transcript(thread: string): Promise<RowSnapshot[]> {
  return invoke<RowSnapshot[]>("transcript", { thread });
}

/** Mirrors `dto::ImageIn`: an image the composer is sending (`docs/12` §5.1). */
export interface ImageIn {
  /** Base64 bytes, without a `data:` prefix. */
  data: string;
  /** `image/png`, `image/jpeg`, `image/gif` or `image/webp`. */
  mimeType: string;
}

/** Send a prompt. */
export async function prompt(thread: string, message: string, images: ImageIn[]): Promise<void> {
  return invoke<void>("prompt", { thread, message, images });
}

/**
 * Inject a message into the running turn (`docs/12` §5.2: `Enter` while streaming).
 *
 * Safe to call when no turn is running — measured at v18.2.6, the engine starts one
 * — so the composer does not have to be certain which state it is in.
 */
export async function steer(thread: string, message: string, images: ImageIn[]): Promise<void> {
  return invoke<void>("steer", { thread, message, images });
}

/** Queue a message behind the running turn (`⌥Enter`). */
export async function followUp(thread: string, message: string, images: ImageIn[]): Promise<void> {
  return invoke<void>("follow_up", { thread, message, images });
}

/** Stop the turn in flight. */
export async function stopTurn(thread: string): Promise<void> {
  return invoke<void>("stop_turn", { thread });
}

/** Stop the turn in flight and send this message in its place. */
export async function stopTurnAndSend(
  thread: string,
  message: string,
  images: ImageIn[],
): Promise<void> {
  return invoke<void>("stop_turn_and_send", { thread, message, images });
}

/**
 * Record "always allow" for a tool in the engine's own config (`docs/12` §10).
 *
 * Writes `tools.approval.<tool> = allow` so the *next* session runs that tool
 * without asking. It cannot affect the running session — measured, and the reason
 * the dialog says so — so a rejection here means the write failed, never that the
 * setting was ignored.
 */
export async function allowTool(tool: string): Promise<PolicyOutcome> {
  return invoke<PolicyOutcome>("allow_tool", { tool });
}

/** Mirrors `dto::CommandSnapshot`: one row of the command palette (`docs/12` §7.3). */
export interface CommandEntry {
  name: string;
  /** `builtin`, `custom`, `extension` or `file`. */
  source: string;
  /** Alternative names, for matching only. */
  aliases: string[];
  description: string | null;
  /** What the command wants after its name. */
  hint: string | null;
  subcommands: SubcommandEntry[];
}

/** Mirrors `dto::SubcommandSnapshot`. */
export interface SubcommandEntry {
  name: string;
  description: string | null;
}

/**
 * The commands the palette offers.
 *
 * Needs a session — the engine answers it — and the host caches it after the first fetch,
 * because two of the four sources come from the workspace and a later
 * `available_commands_update` refreshes that cache.
 */
export async function availableCommands(thread: string): Promise<CommandEntry[]> {
  return invoke<CommandEntry[]>("available_commands", { thread });
}

/** The host's cached model catalogue. Never fetches — see `refreshModels`. */
export async function models(): Promise<ModelCatalogue> {
  return invoke<ModelCatalogue>("models");
}

/** Fetch a fresh catalogue out of band; the answer arrives as `models-updated`. */
export async function refreshModels(): Promise<void> {
  return invoke<void>("refresh_models");
}

/** Switch the session's model. Rejects with the engine's message. */
export async function setModel(thread: string, provider: string, modelId: string): Promise<void> {
  return invoke<void>("set_model", { thread, provider, modelId });
}

/** Set the session's thinking level (`off`…`max`, `auto`). */
export async function setThinkingLevel(thread: string, level: string): Promise<void> {
  return invoke<void>("set_thinking_level", { thread, level });
}

/** Turn automatic compaction on or off. */
export async function setAutoCompaction(thread: string, enabled: boolean): Promise<void> {
  return invoke<void>("set_auto_compaction", { thread, enabled });
}

/** Compact the conversation now, optionally with instructions for the summary. */
export async function compact(thread: string, instructions: string | null): Promise<void> {
  return invoke<void>("compact", { thread, instructions });
}

/** App-owned favourite model keys, in order. */
export async function favourites(): Promise<string[]> {
  return invoke<string[]>("favourites");
}

/** Store the favourites order, returning what was kept. */
export async function setFavourites(keys: string[]): Promise<string[]> {
  return invoke<string[]>("set_favourites", { keys });
}

/**
 * Change the session's approval mode.
 *
 * The engine has no runtime setter, so the host writes `tools.approvalMode` and
 * restarts the sidecar on the same session file — which is why this returns the new
 * `SessionStatus` rather than nothing: the session the caller was watching is gone,
 * and a turn that was running with it. The *thread id* is unchanged: same session file,
 * so the registry entry is the one that was already there.
 */
export async function setApprovalMode(thread: string, mode: string): Promise<SessionStatus> {
  return invoke<SessionStatus>("set_approval_mode", { thread, mode });
}

/** The dialogs the agent is waiting on. */
export async function uiRequests(thread: string): Promise<UiRequestSnapshot[]> {
  return invoke<UiRequestSnapshot[]>("ui_requests", { thread });
}

/**
 * Answer a pending dialog.
 *
 * Rejects when the engine is no longer waiting — a withdrawn or already-answered
 * request — which the caller should treat as "close the dialog", not "retry".
 */
export async function respondUiRequest(
  thread: string,
  requestId: string,
  answer: UiAnswer,
): Promise<void> {
  return invoke<void>("respond_ui_request", { thread, requestId, answer });
}

/**
 * One indexed record a search returned (`docs/12` §7.4).
 *
 * `kind` is the index's own spelling (`title`, `prompt`, `answer`, `thinking`, `tool`,
 * `result`) and `ordinal` is the position in the thread's message stream, carried so a hit
 * can be ordered and labelled; the *row* a hit belongs to is found by its text, because the
 * app's rows are its own reduction of the stream and an ordinal would not survive it.
 */
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
  return listen<IndexStatus>("index-progress", (event) => handler(event.payload));
}

/** One user message a fork can start from (`get_branch_messages`). */
export interface BranchTarget {
  entryId: string;
  text: string;
}

/**
 * Rename a session (`docs/12` §2.3).
 *
 * The engine answers a bare ack and emits **nothing** (measured: the RPC path does not call
 * `notifyTitleChanged`), so nothing will arrive to correct a stale name — the caller
 * refreshes the catalogue, which reads the title slot the engine rewrote in place.
 *
 * An empty name is refused by the engine with prose and no `code`; the UI refuses it first
 * so the user never sees that round trip.
 */
export async function renameThread(thread: string, name: string): Promise<void> {
  return invoke<void>("rename_thread", { thread, name });
}

/**
 * The user messages a fork can start from.
 *
 * This is the **only** source of valid `entryId`s: `get_messages` and `get_messages_page`
 * do not carry entry ids (the page cursor hides the leaf id), so a picker built from the
 * transcript would have nothing valid to send.
 */
export async function branchTargets(thread: string): Promise<BranchTarget[]> {
  return invoke<BranchTarget[]>("branch_targets", { thread });
}

/**
 * Fork the session at one of its user messages (`docs/12` §6.3).
 *
 * The engine mints a **new session file in the same sidecar**: the response carries no
 * identity and the thread's old id stops existing, so what comes back is the new thread's
 * id — the caller must switch to it rather than keep talking to the old one.
 */
export async function branchThread(thread: string, entryId: string): Promise<ThreadSnapshot> {
  return invoke<ThreadSnapshot>("branch_thread", { thread, entryId });
}

/**
 * Hand off the session, optionally with instructions (`docs/12` §6.4).
 *
 * Not an export: the document is committed as a compaction entry on the **current** session
 * and no file is written (measured: the RPC path never sets `savedPath`). The caller
 * re-reads the transcript, which is where the maintenance appears.
 */
export async function handoffThread(thread: string, instructions: string | null): Promise<void> {
  return invoke<void>("handoff_thread", { thread, instructions });
}

/**
 * Export the session to HTML, returning the absolute path the host wrote.
 *
 * The path is the host's, not the engine's: `export_html` with no `outputPath` defaults to a
 * *relative* name resolved against the sidecar's working directory, which would land the
 * file inside the user's project.
 */
export async function exportHtml(thread: string): Promise<string> {
  return invoke<string>("export_html", { thread });
}

/** Open the native OS folder picker dialog and return the selected directory path, or null if cancelled. */
export async function pickDirectory(title?: string): Promise<string | null> {
  return invoke<string | null>("pick_directory", { title: title ?? null });
}

/** Open a file or directory in the user's default application, under the host's allowlist. */
export async function openPath(path: string): Promise<void> {
  return invoke<void>("open_path", { path });
}

/**
 * Delete a session: its file and its artifacts directory.
 *
 * App-owned, because the engine advertises no delete command (measured: `delete` is not in
 * the 45 advertised commands) and its own `/delete` is TUI-only. Best-effort by nature —
 * the engine's own docs call deletion "not a guaranteed erasure boundary".
 */
export async function deleteSession(id: string): Promise<void> {
  return invoke<void>("delete_session", { id });
}

/**
 * Pin or unpin a session (`docs/12` §2.3).
 *
 * Dispatched as the engine's `/pin <id>` through a live thread, which is why `thread` is
 * required: the pin set is written by the engine under its own cross-process lock
 * (measured: a native lock — abstract Unix sockets on Linux — that the app cannot take
 * safely), so OMP's `session-pins.json` stays the single source of truth and the app only
 * mirrors it by re-reading.
 */
export async function pinSession(thread: string, id: string): Promise<void> {
  return invoke<void>("pin_session", { thread, id });
}

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

// --------------------------------------------------- the right panel (`docs/12` §8)

/** One todo, as the engine reports it. `status` is an open union. */
export interface TodoTaskSnapshot {
  content: string;
  /** `pending`, `in_progress`, `completed`, `abandoned`, `blocked`. */
  status: string;
  /** What the task is waiting for, when it is blocked. */
  blocker: string | null;
}

/** A named group of todos: the unit the panel collapses. */
export interface TodoPhaseSnapshot {
  name: string;
  tasks: TodoTaskSnapshot[];
}

/**
 * The panel's own todo list, on the way back.
 *
 * Flattened rather than reusing the snapshot type because this is the one place the app
 * *writes*: the engine normalises whatever arrives and answers with its own list, which is
 * what the panel then renders — never an optimistic copy of this.
 */
export interface TodoPhaseInput {
  name: string;
  tasks: { content: string; status: string; blocker?: string | null }[];
}

/**
 * The thread's todos, re-read from the engine.
 *
 * There is no todo *event* (measured: the engine's own event list carries only
 * `todo_reminder` and `todo_auto_clear`), so `get_state` is the only source and the host
 * re-reads it whenever the `todo` tool runs.
 */
export async function threadTodos(thread: string): Promise<TodoPhaseSnapshot[]> {
  return invoke<TodoPhaseSnapshot[]>("thread_todos", { thread });
}

/**
 * Replace the thread's todo list, returning the engine's normalised version.
 *
 * Rendering the answer rather than the request is the whole point: the engine decides what a
 * task without a status becomes, and which phases survive.
 */
export async function setTodos(
  thread: string,
  phases: TodoPhaseInput[],
): Promise<TodoPhaseSnapshot[]> {
  return invoke<TodoPhaseSnapshot[]>("set_todos", { thread, phases });
}

/** One entry of one directory level of a workspace. */
export interface WorkspaceEntry {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  modifiedAt: number;
}

/**
 * One directory level of the thread's workspace, measured rather than guessed.
 *
 * One level at a time, because a workspace can hold anything: the panel unfolds what the
 * user opens, and the host refuses a path outside the thread's own working directory.
 */
export async function workspaceTree(
  thread: string,
  path: string | null,
): Promise<WorkspaceEntry[]> {
  return invoke<WorkspaceEntry[]>("workspace_tree", { thread, path });
}

/** A tool result the engine spilled to `artifact://<id>`, read back. */
export interface ArtifactSnapshot {
  id: string;
  path: string;
  /** The file's real size, which is the size of the *untruncated* result. */
  bytes: number;
  text: string;
  /** True when the host capped what it returned, so the panel can say so. */
  truncated: boolean;
}

/**
 * Read one spilled result.
 *
 * The engine has no command for this — measured: the whole `RpcCommand` union is silent on
 * artifacts — so the host resolves the file the same way the engine does: the session's
 * `.jsonl` path minus that suffix is the artifacts directory, and the id names
 * `<id>.<tool>.log` inside it.
 */
export async function readArtifact(thread: string, id: string): Promise<ArtifactSnapshot> {
  return invoke<ArtifactSnapshot>("read_artifact", { thread, id });
}

/** Subscribe to the streamed event tail. */
export async function onActivity(
  handler: (event: ThreadEvent<ActivitySnapshot>) => void,
): Promise<UnlistenFn> {
  return listen<ThreadEvent<ActivitySnapshot>>("session-activity", (event) =>
    handler(event.payload),
  );
}

/**
 * Subscribe to the pending-dialog set.
 *
 * Carries the whole set on every change rather than a delta, so the UI's idea of
 * what is pending cannot drift from the engine's.
 */
export async function onUiRequests(
  handler: (event: ThreadEvent<UiRequestSnapshot[]>) => void,
): Promise<UnlistenFn> {
  return listen<ThreadEvent<UiRequestSnapshot[]>>("session-ui-requests", (event) =>
    handler(event.payload),
  );
}

/**
 * Subscribe to conversation patches.
 *
 * Already coalesced by the host, so a streaming turn arrives at a screen's rate.
 */
/** Subscribe to the palette's list, after the engine changed its metadata. */
export async function onCommandsUpdated(
  handler: (event: ThreadEvent<CommandEntry[]>) => void,
): Promise<UnlistenFn> {
  return listen<ThreadEvent<CommandEntry[]>>("commands-updated", (event) =>
    handler(event.payload),
  );
}

/**
 * Subscribe to the live thread set.
 *
 * The whole set arrives on every change (open, close, a turn starting or ending, an
 * approval appearing), which is what makes the sidebar's dots a rendering of the host's
 * state rather than a guess rebuilt from per-thread events.
 */
export async function onThreadsUpdated(
  handler: (threads: ThreadSnapshot[]) => void,
): Promise<UnlistenFn> {
  return listen<ThreadSnapshot[]>("threads-updated", (event) => handler(event.payload));
}

export async function onModelsUpdated(
  handler: (catalogue: ModelCatalogue) => void,
): Promise<UnlistenFn> {
  return listen<ModelCatalogue>("models-updated", (event) => handler(event.payload));
}

/**
 * Subscribe to conversation patches.
 *
 * Already coalesced by the host, so a streaming turn arrives at a screen's rate.
 */
export async function onRows(
  handler: (event: ThreadEvent<RowPatch>) => void,
): Promise<UnlistenFn> {
  return listen<ThreadEvent<RowPatch>>("session-rows", (event) => handler(event.payload));
}

// --------------------------------- notifications and extension chrome (`docs/12` §13)

/** The four facts the host reports as worth a person's attention. */
export type NotificationKind = "turn-finished" | "needs-you" | "failed" | "job-finished";

/**
 * One notification, in the engine's own words.
 *
 * The host does not decide whether anybody should be interrupted — no focus rules, no
 * preferences — so this carries the fact and nothing else: `title` and `body` are the session
 * name, the tool name and the error's own sentence, and the window renders them verbatim
 * (`lib/notify.ts` owns the delivery rules).
 */
export interface NotificationEvent {
  thread: string;
  kind: NotificationKind;
  title: string;
  body: string;
  /** The job a `job-finished` is about; `null` for the other three kinds. */
  jobId: string | null;
}

/**
 * The extension chrome the host passed through (`protocol::ui::chrome`).
 *
 * A sibling of the blocking dialog vocabulary rather than part of it: every op here is
 * fire-and-forget, so it names something the window should *show* and never something it must
 * answer. `editor-text` is the engine's own text for the composer, and `open-url` a URL it
 * wants launched — which the host still puts through its allowlist.
 */
export type ChromeOp =
  | { kind: "notify"; message: string; level: string | null }
  | { kind: "status"; text: string | null }
  | { kind: "widget"; lines: string[] | null }
  | { kind: "title"; title: string }
  | { kind: "editor-text"; text: string }
  | { kind: "open-url"; url: string };

/** One chrome push, addressed to the thread that carries it. */
export interface ChromeEvent {
  thread: string;
  op: ChromeOp;
}

/**
 * Subscribe to what the host decided is worth telling the user about.
 *
 * Facts only: the delivery rules (unread, an in-app notice, the OS notification) are the
 * window's, and they live in `lib/notify.ts`.
 */
export async function onNotifications(
  handler: (event: NotificationEvent) => void,
): Promise<UnlistenFn> {
  return listen<NotificationEvent>("session-notifications", (event) => handler(event.payload));
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

/** Subscribe to the fire-and-forget half of the extension-UI vocabulary. */
export async function onChrome(
  handler: (event: ChromeEvent) => void,
): Promise<UnlistenFn> {
  return listen<ChromeEvent>("session-chrome", (event) => handler(event.payload));
}

// ---------------------------------------------------------------- agents (`docs/12` §9)

/**
 * One subagent, as the engine last described it.
 *
 * Field names and units are the engine's: `cost` is USD, `durationMs` and `lastUpdateMs` are
 * milliseconds, `tokens` excludes cache reads. Nothing here converts, so a value the panel
 * shows is a value the engine reported.
 */
/** One job a delivery message reports on. */
export interface JobDeliverySnapshot {
  /** The same string the card that started it carries in `details.async.jobId`. */
  jobId: string;
  /** `bash` | `eval` | `task`. */
  kind: string;
  durationMs: number;
  label: string | null;
}

export interface AgentSnapshot {
  /** The engine's id for the spawn — also the file stem of its transcript. */
  id: string;
  /** Dispatch order within one `task` call. */
  index: number;
  agent: string;
  agentSource: string | null;
  /** `pending` | `running` | `completed` | `failed` | `aborted`, or null when nothing said. */
  status: string | null;
  description: string | null;
  task: string | null;
  assignment: string | null;
  sessionFile: string | null;
  /** The `task` tool call that spawned it: the link back into the conversation. */
  parentToolCallId: string | null;
  detached: boolean;
  /** When *this host* last heard about the row. */
  lastUpdateMs: number;
  /**
   * Whether the engine's last `get_subagents` answer contained it.
   *
   * False on a running status means the engine stopped listing it without reporting how it
   * ended. The panel says that rather than picking a status.
   */
  listed: boolean;
  progress: AgentProgressSnapshot | null;
}

export interface AgentProgressSnapshot {
  lastIntent: string | null;
  currentTool: string | null;
  currentToolArgs: string | null;
  toolCount: number;
  requests: number;
  tokens: number;
  contextTokens: number | null;
  contextWindow: number | null;
  cost: number;
  durationMs: number;
  resolvedModel: string | null;
  resolvedThinkingLevel: string | null;
  advisor: boolean;
  /** An auto-retry in flight: why an agent can look stalled without having failed. */
  retry: { attempt: number; maxAttempts: number; delayMs: number; errorMessage: string } | null;
  /** The retry that ended the run, when the engine gave up. */
  retryFailure: { attempt: number; errorMessage: string } | null;
}

/** Every agent the app can account for, for one thread. */
export interface ThreadAgents {
  thread: string;
  agents: AgentSnapshot[];
  /** Why this thread has no roster, when the engine refused the subscription at open. */
  error: string | null;
}

/** A subagent transcript found on disk, after its engine forgot about it. */
export interface ParkedAgent {
  id: string;
  path: string;
  bytes: number;
  modifiedMs: number;
  cwd: string;
  /** The `parentSession` its header recorded: the parent's own file path. */
  parent: string | null;
  advisor: boolean;
  advisorSlug: string | null;
}

/** One page of a subagent's transcript. */
export interface AgentTranscript {
  agent: string;
  path: string;
  nextByte: number;
  /** The file shrank under the cursor, so this page **replaces** what was shown. */
  reset: boolean;
  /** `engine` when the engine served it, `file` when it was read from disk instead. */
  source: string;
  rows: RowSnapshot[];
}

/** One supervised process from `omp ps --json`. */
export interface BrokerDaemon {
  /** The name `omp ps stop <name>` takes. */
  name: string;
  /** `running` | `exited` | …, verbatim. */
  state: string;
  /** The session that asked for it, when one did. */
  owner: string | null;
  command: string;
  cwd: string;
  supervised: boolean;
  persist: boolean;
  detached: boolean;
  restartCount: number;
  exitCode: number | null;
  outputBytes: number;
  startedAtMs: number;
  exitedAtMs: number | null;
}

/** One broker scope: a project (or the shared global one) its daemons run under. */
export interface BrokerScope {
  kind: string;
  projectDir: string;
  runtimeDir: string;
  brokerPid: number | null;
  daemons: BrokerDaemon[];
}

/** Every agent the app can account for, one entry per thread. */
export async function agents(): Promise<ThreadAgents[]> {
  return invoke<ThreadAgents[]>("agents");
}

/**
 * The subagent transcripts this session's file shows on disk.
 *
 * The engine's registry is live-only, so this is where everything that already finished is
 * found — including for a session this window has never opened.
 */
export async function parkedAgents(thread: string): Promise<ParkedAgent[]> {
  return invoke<ParkedAgent[]>("parked_agents", { thread });
}

/**
 * One page of a subagent's transcript, by byte cursor.
 *
 * Pass the previous page's `nextByte` to get the delta. A page with `reset` set is the
 * file's beginning, not a continuation, and must replace what the pane was showing.
 */
export async function agentMessages(
  thread: string,
  agent: string,
  fromByte?: number,
): Promise<AgentTranscript> {
  return invoke<AgentTranscript>("agent_messages", { thread, agent, fromByte });
}

/** Every broker-owned process the engine is supervising. */
export async function brokerProcesses(thread?: string | null): Promise<BrokerScope[]> {
  return invoke<BrokerScope[]>("broker_processes", { thread: thread ?? null });
}

/** Stop one broker-owned process. `omp ps` is the only supported path. */
export async function stopBrokerProcess(name: string, thread?: string | null): Promise<string> {
  return invoke<string>("stop_broker_process", { name, thread: thread ?? null });
}

/**
 * Subscribe to one thread's agent roster.
 *
 * Tagged with the thread, because a roster is exactly what changes when one of its subagents
 * starts, reports or settles — and the panel shows every thread's agents side by side.
 */
export async function onAgents(
  handler: (event: ThreadEvent<AgentSnapshot[]>) => void,
): Promise<UnlistenFn> {
  return listen<ThreadEvent<AgentSnapshot[]>>("session-agents", (event) =>
    handler(event.payload),
  );
}

/**
 * One app-owned terminal (`docs/12` §11, decision D6).
 *
 * A fact about a process rather than about a thread: the shell the user opened, in a
 * workspace, which outlives every session switch. Nothing here describes the agent — its
 * `bash` runs without a terminal (`PI_NO_PTY=1`) and cannot see this one.
 */
export interface TerminalSnapshot {
  /** Minted by the host; every other terminal command takes it. */
  id: string;
  /** The directory the shell was started in — the tab's label. */
  cwd: string;
  /** Whether the shell is still running. A tab outlives its shell so the last screenful stays. */
  running: boolean;
  /** How it ended, once it has. */
  exit: { code: number | null; signal: string | null } | null;
  /** The shell's process id, while that means something. */
  pid: number | null;
}

/** One batch of a terminal's output, base64 because terminal bytes are not text. */
export interface TerminalOutput {
  id: string;
  data: string;
}

/** Every terminal the app has open, in the order its tabs were opened. */
export async function terminals(): Promise<TerminalSnapshot[]> {
  return invoke<TerminalSnapshot[]>("terminals");
}

/**
 * Open a terminal in `cwd`, running the user's own shell.
 *
 * `cols` and `rows` are what the emulator measured: the kernel passes the size to the shell,
 * which is what makes a full-screen program in here draw correctly.
 */
export async function terminalOpen(
  cwd: string,
  cols: number,
  rows: number,
): Promise<TerminalSnapshot> {
  return invoke<TerminalSnapshot>("terminal_open", { cwd, cols, rows });
}

/** Send typed or pasted input to a terminal. */
export async function terminalWrite(id: string, data: string): Promise<void> {
  return invoke<void>("terminal_write", { id, data });
}

/** Tell a terminal's shell its window changed size. */
export async function terminalResize(id: string, cols: number, rows: number): Promise<void> {
  return invoke<void>("terminal_resize", { id, cols, rows });
}

/** Close a tab, ending its shell and everything it started. Returns the remaining tabs. */
export async function terminalClose(id: string): Promise<TerminalSnapshot[]> {
  return invoke<TerminalSnapshot[]>("terminal_close", { id });
}

/** Subscribe to every terminal's output. One channel: batches are tagged with their tab. */
export async function onTerminalOutput(
  handler: (output: TerminalOutput) => void,
): Promise<UnlistenFn> {
  return listen<TerminalOutput>("terminal-output", (event) => handler(event.payload));
}

/** Subscribe to the tab set, whenever a terminal is opened, exits or is closed. */
export async function onTerminals(
  handler: (terminals: TerminalSnapshot[]) => void,
): Promise<UnlistenFn> {
  return listen<TerminalSnapshot[]>("terminals-updated", (event) => handler(event.payload));
}

// ------------------------------------------- settings (`docs/12` §12, `docs/13` mapping)

/**
 * The whole settings screen, as the host composes it.
 *
 * It mirrors `omp_settings::screen::Screen`. The host sends *everything* the screen draws —
 * the catalog is not in the frontend — so opening settings is one round trip and the
 * frontend holds no second copy of 505 keys of metadata to drift from.
 */
export interface SettingsScreen {
  /** The engine release the shipped catalog was generated against. */
  catalogVersion: string;
  sources: SettingsSources;
  drift: SettingsDrift;
  sections: SettingsSection[];
}

/** Every config file the app can see, and why one might be unreadable. */
export interface SettingsSources {
  agentDir: string;
  globalFile: SettingsFile;
  projectFile: SettingsFile;
  /** Read-only overlays (`PI_CONFIG_FILES`): shown, never edited. */
  overlays: SettingsFile[];
}

/** One config file, as the banner describes it. */
export interface SettingsFile {
  path: string;
  exists: boolean;
  /** A syntax error's own words, when the file could not be parsed. */
  error: string | null;
  /** Parts of the file the app will not act on. */
  refusals: SettingsRefusal[];
}

/** Something in a config file the app will not act on, and why. */
export interface SettingsRefusal {
  key: string;
  reason: string;
}

/** The engine knows settings this build does not, or the other way round. */
export interface SettingsDrift {
  unknownKeys: string[];
  missingKeys: string[];
}

/** One section of the settings nav. */
export interface SettingsSection {
  id: string;
  title: string;
  blurb: string;
  /** `app`, `agent` or `system` — the nav column it belongs to. */
  navGroup: string;
  /** A name from `lib/icons.ts`. */
  icon: string;
  groups: SettingsGroup[];
}

/** One sub-group inside a section (the engine's own group name). */
export interface SettingsGroup {
  name: string;
  rows: SettingsRow[];
}

/** A row is one of three things, discriminated by `role`. */
export type SettingsRow = SettingsSettingRow | SettingsStaticRow | SettingsActionRow;

/** A settings key with a control. */
export interface SettingsSettingRow {
  role: "setting";
  key: string;
  label: string;
  description: string;
  /** The schema type: `boolean`, `number`, `enum`, `string`, `array` or `record`. */
  type: string;
  /** The widget: `toggle`, `select`, `number`, `text`, `list`, `record` or `secret`. */
  control: string;
  /** The engine's effective value; `null` when it has none, or when it is redacted. */
  value: unknown;
  /** Whether the engine holds a value — a different question from whether we may show it. */
  present: boolean;
  /** The engine holds a credential it will not print. */
  redacted: boolean;
  /** An enum's options, with the engine's own labels. */
  choices: { value: string; label: string }[];
  /** Where the value is written: `overlay`, `project`, `global`, `default` or `unknown`. */
  origin: string;
  /** Why the row cannot be edited, when a `ui.condition` is unmet. */
  gated: { reason: string; needs: string | null } | null;
  /** What a write costs: `live` (applied through RPC now), `sidecar`, or `app`. */
  restart: string;
  /** Why this key is dangerous, when it is. */
  danger: { level: "confirm" | "warn"; why: string } | null;
}

/** A fact the app reports and nothing here edits. */
export interface SettingsStaticRow {
  role: "static";
  id: string;
  label: string;
  description: string;
  value: string;
}

/** A control that does something rather than storing something. */
export interface SettingsActionRow {
  role: "action";
  id: string;
  label: string;
  description: string;
  /** The screen dispatches on this: `open-mode-menu`, `open-config-file`. */
  action: string;
  tone: "normal" | "danger";
}

/** What one write recorded, and what it will cost. */
export interface SettingsWriteOutcome {
  key: string;
  value: unknown;
  restart: string;
  /** The sentence to show afterwards; it never promises more than the restart allows. */
  message: string;
}

/** The escape hatch: the config file as the app will show it. */
export interface SettingsHatch {
  path: string;
  exists: boolean;
  /** The file's text **after redaction** — a credential is never sent to the frontend. */
  text: string;
  /** A syntax error's own words; when set, `text` is empty on purpose. */
  error: string | null;
  /** The keys the file sets, sorted. */
  keys: string[];
  /** Parts of the file the app will not act on. */
  refusals: SettingsRefusal[];
  /** The app's own backups of this file, newest first. */
  backups: SettingsBackup[];
}

/** One backup of the config file, with its text redacted the same way. */
export interface SettingsBackup {
  path: string;
  /** Seconds since the epoch, when the app named it. */
  at: number | null;
  text: string;
}

/** What a hand-edit would change, before anything is written. */
export interface SettingsHatchPlan {
  changes: SettingsHatchChange[];
  /** Anything the plan will not do. A plan with a refusal applies **nothing**. */
  refusals: SettingsRefusal[];
}

/** One edit a hand-edit would make. */
export interface SettingsHatchChange {
  key: string;
  action: "set" | "reset";
  before: unknown;
  after: unknown;
  restart: string;
}

/** What applying a hand-edit actually did. */
export interface SettingsApplyReport {
  changes: { key: string; action: string; ok: boolean; error: string | null }[];
  refused: SettingsRefusal[];
}

/** What a restart of the live sessions did. */
export interface SettingsRestartReport {
  restarted: string[];
  skipped: { thread: string; reason: string }[];
}

/**
 * The whole settings screen: catalog, engine values, and where each value comes from.
 *
 * `thread` is the session whose workspace the app treats as the one in view: its project
 * config file (`<workspace>/.omp/config.yml`) is the one the screen reports provenance from.
 * Omit it and the screen shows no project file at all, which is the honest answer for a
 * window with nothing open.
 */
export async function settingsScreen(thread?: string | null): Promise<SettingsScreen> {
  return invoke<SettingsScreen>("settings_screen", { thread: thread ?? null });
}

/**
 * Record a value for one settings key.
 *
 * `confirmation` is the key's own name, and it is required for the handful of keys that can
 * widen what the agent may execute or where data goes; the host refuses them without it, so a
 * dialog that forgot to ask cannot get through.
 */
export async function setSetting(
  key: string,
  value: unknown,
  confirmation?: string | null,
): Promise<SettingsWriteOutcome> {
  return invoke<SettingsWriteOutcome>("settings_write", { key, value, confirmation: confirmation ?? null });
}

/** Return a key to the engine's default. */
export async function resetSetting(
  key: string,
  confirmation?: string | null,
): Promise<SettingsWriteOutcome> {
  return invoke<SettingsWriteOutcome>("settings_reset", { key, confirmation: confirmation ?? null });
}

/**
 * Read a config file the way the escape hatch draws it.
 *
 * `scope` picks which file: `global` is `~/.omp/agent/config.yml` (the one the engine writes
 * for every project), `project` is `<thread's workspace>/.omp/config.yml`. Overlays
 * (`PI_CONFIG_FILES`) are shown in the sources banner and are never editable.
 */
export async function settingsHatch(
  thread?: string | null,
  scope?: "global" | "project",
): Promise<SettingsHatch> {
  return invoke<SettingsHatch>("settings_hatch", { thread: thread ?? null, scope: scope ?? "global" });
}

/** What an edited copy of the config file would change — validation and diff, no writing. */
export async function settingsHatchPlan(text: string): Promise<SettingsHatchPlan> {
  return invoke<SettingsHatchPlan>("settings_hatch_plan", { text });
}

/**
 * Apply an edited copy of the config file.
 *
 * `confirmation` must be `"confirm"` when the plan touches a key on the do-not-surface list;
 * the host refuses the whole plan otherwise. A plan with a refusal applies nothing at all.
 */
export async function settingsHatchApply(
  text: string,
  confirmation?: string | null,
): Promise<SettingsApplyReport> {
  return invoke<SettingsApplyReport>("settings_hatch_apply", { text, confirmation: confirmation ?? null });
}

/** Copy the config file aside, and answer with the copy's path. */
export async function settingsBackup(): Promise<string> {
  return invoke<string>("settings_backup");
}

/** Restart every live session so a written setting is in effect now. */
export async function settingsRestartSessions(): Promise<SettingsRestartReport> {
  return invoke<SettingsRestartReport>("settings_restart_sessions");
}
