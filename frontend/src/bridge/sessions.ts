import { invoke, listenPayload, type UnlistenFn } from "./core";
import type { JobDeliverySnapshot } from "./agents";

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
  /** Engine-stamped Unix milliseconds when this row is a message. */
  timestamp?: number | null;
  text: string;
  thinking: string | null;
  streaming: boolean;
  tool: ToolSnapshot | null;
  /** Images attached to the message; empty for every row that is not one. */
  attachments: AttachmentSnapshot[];
  /**
   * The engine's own type for a message the app did not write: `async-result` for a
   * background job's delivered result (`docs/12` §9). For maintenance notices,
   * this is the lifecycle kind, such as `manual_compaction_end`. `null` for ordinary messages.
   */
  customType: string | null;
  /** The jobs this row accounts for — a delivery's, and empty for everything else. */
  jobs: JobDeliverySnapshot[];
  /** The slash command that produced this output, if known. */
  command?: string | null;
  /** True when this user message was injected as a mid-run steer. */
  steering?: boolean;
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

/** Mirrors `dto::UiOptionDetail`. */
export interface UiOptionDetail {
  description?: string | null;
}

/** Mirrors `dto::AskOption`. */
export interface AskOption {
  label: string;
  description?: string | null;
  preview?: string | null;
}

/** Mirrors `dto::AskQuestion`. */
export interface AskQuestion {
  id: string;
  question: string;
  header?: string | null;
  options: AskOption[];
  multi: boolean;
  recommended?: number | null;
}

/** Mirrors `dto::AskAnswer`. */
export interface AskAnswer {
  id: string;
  selectedOptions: string[];
  customInput?: string | null;
}

/** Mirrors `dto::UiRequestSnapshot`. */
export interface UiRequestSnapshot {
  id: string;
  /** The wire method: `select`, `confirm`, `input`, `editor` or `ask`. */
  kind: string;
  title: string;
  message: string;
  options: string[];
  optionDetails?: UiOptionDetail[];
  questions: AskQuestion[];
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
 * Mirrors `dto::UiAnswer`: exactly one of `value`, `answers`, `confirmed` or `cancelled`.
 *
 * The host refuses an answer carrying none of them, so this is not a place to
 * guess — a `select` answered with an empty choice would leave the agent waiting
 * for a decision that never arrives.
 */
export interface UiAnswer {
  value?: string;
  answers?: AskAnswer[];
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

/** The sidebar's sessions and project groups from the same store scan. */
export interface SidebarSnapshot {
  sessions: SessionSummary[];
  projects: ProjectSnapshot[];
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

/** Every saved session and its project group, from one store scan. */
export async function sidebarSnapshot(): Promise<SidebarSnapshot> {
  return invoke<SidebarSnapshot>("sidebar_snapshot");
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

export interface ArchivedHistoryInfo {
  totalTurns: number;
  archivedTurns: number;
  remainingArchivedTurns: number;
}

export interface LoadEarlierTurnsResult {
  rows: RowSnapshot[];
  remainingArchivedTurns: number;
}

/** Statistics on archived turns for a thread. */
export async function getArchivedHistoryInfo(thread: string): Promise<ArchivedHistoryInfo> {
  return invoke<ArchivedHistoryInfo>("get_archived_history_info", { thread });
}

/** Load a batch of earlier archived turns on demand. */
export async function loadEarlierTurns(thread: string, batchSize: number): Promise<LoadEarlierTurnsResult> {
  return invoke<LoadEarlierTurnsResult>("load_earlier_turns", { thread, batchSize });
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
 * Persists `tools.approval.<tool> = allow` for future sessions and enables the
 * same grant in the running host session.
 */
export async function allowTool(thread: string, tool: string): Promise<PolicyOutcome> {
  return invoke<PolicyOutcome>("allow_tool", { thread, tool });
}

/** Grant the next unapproved executable in a supported command chain. */
export async function allowCommand(thread: string, title: string): Promise<string> {
  return invoke<string>("allow_command", { thread, title });
}

/** The exact executable an "always allow" click would grant next, if safe to parse. */
export async function nextCommandGrant(thread: string, title: string): Promise<string | null> {
  return invoke<string | null>("next_command_grant", { thread, title });
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
 * waits for the current turn and queued follow-ups, then restarts the sidecar on
 * the same session file. The returned `SessionStatus` describes the replacement
 * process; the thread id and conversation are unchanged.
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

/**
 * Delete a session.
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

/** A workspace file match for `@` mention autocompletion. */
export interface WorkspaceFileMatch {
  name: string;
  path: string;
  isDir: boolean;
}

/** Search files across the thread's workspace matching a query, for `@` mentions. */
export async function searchWorkspaceFiles(
  thread: string,
  query: string,
  limit?: number,
): Promise<WorkspaceFileMatch[]> {
  return invoke<WorkspaceFileMatch[]>("search_workspace_files", {
    thread,
    query,
    limit: limit ?? 20,
  });
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

/** Read the full contents of a file on disk for the read-only file viewer. */
export async function readFileContent(workspace: string | null, path: string): Promise<string> {
  return invoke<string>("read_file_content", { workspace, path });
}

/** Read the full binary contents of a file on disk as base64 for image viewers. */
export async function readFileBase64(workspace: string | null, path: string): Promise<string> {
  return invoke<string>("read_file_base64", { workspace, path });
}

/** Subscribe to the streamed event tail. */
export async function onActivity(
  handler: (event: ThreadEvent<ActivitySnapshot>) => void,
): Promise<UnlistenFn> {
  return listenPayload<ThreadEvent<ActivitySnapshot>>("session-activity", handler);
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
  return listenPayload<ThreadEvent<UiRequestSnapshot[]>>("session-ui-requests", handler);
}

/** Subscribe to the palette's list, after the engine changed its metadata. */
export async function onCommandsUpdated(
  handler: (event: ThreadEvent<CommandEntry[]>) => void,
): Promise<UnlistenFn> {
  return listenPayload<ThreadEvent<CommandEntry[]>>("commands-updated", handler);
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
  return listenPayload<ThreadSnapshot[]>("threads-updated", handler);
}

export async function onModelsUpdated(
  handler: (catalogue: ModelCatalogue) => void,
): Promise<UnlistenFn> {
  return listenPayload<ModelCatalogue>("models-updated", handler);
}

/**
 * Subscribe to conversation patches.
 *
 * Already coalesced by the host, so a streaming turn arrives at a screen's rate.
 */
export async function onRows(
  handler: (event: ThreadEvent<RowPatch>) => void,
): Promise<UnlistenFn> {
  return listenPayload<ThreadEvent<RowPatch>>("session-rows", handler);
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
  return listenPayload<NotificationEvent>("session-notifications", handler);
}



/** Subscribe to the fire-and-forget half of the extension-UI vocabulary. */
export async function onChrome(
  handler: (event: ChromeEvent) => void,
): Promise<UnlistenFn> {
  return listenPayload<ChromeEvent>("session-chrome", handler);
}

