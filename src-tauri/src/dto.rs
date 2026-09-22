//! The frontend's data contract.
//!
//! Pure data: no logic, no wire types, no `serde_json::Value`. Everything the UI
//! renders crosses the boundary as one of these.
//!
//! # Why hand-written instead of `Serialize` on the core types
//!
//! The transport may be re-pinned to a new engine version, and the session model
//! is shaped by the *wire* — but the UI's payloads are shaped by the *interface*.
//! Deriving `Serialize` onto [`omp_session`] and [`omp_transport`] types would
//! fuse the two, so that a field renamed upstream silently becomes a field the
//! frontend must handle. Keeping DTOs here means a wire change is absorbed in one
//! place and the UI only moves when we decide it should.
//!
//! It also keeps the core crates free of serialization concerns: `omp_transport`
//! deliberately does not derive `Serialize` on wire-shaped types, and this module
//! is why it does not have to.

use serde::{Deserialize, Serialize};

/// The `ready` handshake, as the debug pane shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadySnapshot {
    pub protocol_version: u64,
    pub supported_protocol_versions: Vec<u64>,
    pub max_frame_bytes: u64,
    pub max_reassembled_frame_bytes: u64,
    /// Whether the v2 upgrade was completed. Not cosmetic: without it the model
    /// catalogue (1.58 MB at v18.2.6) cannot be delivered at all.
    pub negotiated_v2: bool,
}

/// Model identity, as the composer chip needs it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSnapshot {
    pub provider: Option<String>,
    pub id: Option<String>,
    pub name: Option<String>,
}

/// Context-window usage, for the ring and its popover.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSnapshot {
    pub tokens: u64,
    pub context_window: u64,
    pub percent: f64,
}

/// The control-plane state the chrome renders.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlSnapshot {
    pub session_id: String,
    pub session_name: Option<String>,
    pub model: Option<ModelSnapshot>,
    pub thinking_level: Option<String>,
    pub is_streaming: bool,
    pub is_compacting: bool,
    pub message_count: u64,
    pub queued_message_count: u64,
    pub context: Option<ContextSnapshot>,
    /// The engine's plan, as `get_state.todoPhases` reported it (`docs/12` §8.1).
    ///
    /// The whole list rather than its length: there is no todo *event* in the engine's
    /// stream, so this snapshot is the only source the panel has, and a count would make
    /// it read the plan a second time through a command of its own.
    pub todo_phases: Vec<TodoPhaseSnapshot>,
    /// How many conversation rows the transcript holds, live and restored alike.
    pub transcript_rows: usize,
    /// `get_state.sessionFile`: the session's JSONL on disk, for the affordances that
    /// open or reveal it. Absent when the engine runs without a session store
    /// (`--no-session`), which is why it is optional rather than a `String`.
    pub session_file: Option<String>,
    /// Whether the engine compacts on its own — the toggle `docs/12` §7.5 reads and
    /// [`crate::models::set_auto_compaction`] writes.
    ///
    /// Optional by contract, and a live session always fills it: the decode defaults
    /// a missing `autoCompactionEnabled` to `false`, so an engine that stopped sending
    /// the field would read as *off* rather than as absent. That default already
    /// exists below this layer (`omp_session::SessionControl`) and is not worth a
    /// second representation here.
    pub auto_compaction_enabled: Option<bool>,
}

/// One row of the model catalogue, as the picker renders it (`docs/12` §7.1).
///
/// The subset of an engine catalog row the UI needs, and deliberately no more: a row
/// also carries `api`, `baseUrl`, `compat`, `cost`, `maxTokens` and `tokenizer`, none
/// of which the picker draws — a field the UI cannot show is a field that only breaks
/// when upstream renames it.
///
/// `Deserialize` as well as `Serialize` because the app's catalogue cache is written
/// in exactly this shape: one row shape means what the picker renders and what the
/// last launch saved cannot drift apart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub context_window: Option<u64>,
    pub reasoning: bool,
    /// `thinking.defaultLevel` — the model's own suggestion, not the session's level.
    pub default_effort: Option<String>,
    /// `thinking.efforts` — the levels this model accepts; empty when it does not think.
    pub efforts: Vec<String>,
}

/// The cached catalogue, as the picker reads it.
///
/// `refreshing` is the picker's spinner and `fetched_at` its staleness answer: a fetch
/// costs 1.20 s cold at v18.2.6, and `docs/12` §7.1 asks for both to be visible rather
/// than implied by an empty list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalogue {
    pub options: Vec<ModelOption>,
    /// Unix milliseconds of the last successful fetch; `None` until one lands.
    pub fetched_at: Option<u64>,
    /// A fetch is in flight.
    pub refreshing: bool,
}

/// Stream health. The cheapest honest proof the event pipeline is flowing.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterSnapshot {
    pub events_seen: u64,
    /// The number of frames the *escape-hatch* stream carried.
    ///
    /// Not every inbound frame: correlated responses go to their caller and session
    /// events to the event stream, so this counts what neither claimed — the
    /// extension-UI requests, host-tool/URI round trips, and uncorrelatable frames
    /// kept for diagnostics.
    pub frames_seen: u64,
    /// Distinct event kinds observed, so a stream of one repeated kind is visible
    /// as such.
    pub event_kinds: u64,
    /// Event kinds this build does not know — drift, surfaced rather than hidden.
    pub unknown_events: u64,
    /// Known kinds whose payload did not decode: our model is behind.
    pub malformed_events: u64,
    /// Times the event channel dropped messages because the consumer fell behind.
    pub lagged_events: u64,
    /// Host UI requests seen, including fire-and-forget ones like `setWidget`.
    pub ui_requests: u64,
    /// The subset that **blocked the run** until the host answered, counted
    /// cumulatively over the session.
    ///
    /// A diagnostic total, not a live state: what is pending *now* is the dialog
    /// set, and that is what the UI warns on. Reading this as "a turn is stuck"
    /// would leave the warning up after every approval.
    pub blocking_ui_requests: u64,
}

/// Everything the debug pane shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    /// The resolved sidecar path, reported because "which binary did it launch?"
    /// is the first question a packaging failure raises.
    pub binary: String,
    pub workspace: String,
    pub sidecar_pid: Option<u32>,
    pub ready: ReadySnapshot,
    pub control: ControlSnapshot,
    pub counters: CounterSnapshot,
    /// The approval mode the host launched this sidecar with (`--approval-mode`).
    ///
    /// Reported by the host rather than read back from the engine: there is no command
    /// that answers it, and the mode is the host's own launch decision (`docs/12` §7.2).
    pub approval_mode: Option<String>,
}

/// One streamed event, for the pane's live tail.
///
/// Deliberately just the kind: the conversation view (step 6) will need coalesced
/// row patches, and inventing a richer payload now would only be thrown away.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySnapshot {
    pub kind: String,
    pub sequence: u64,
}

/// One command the palette offers (`docs/12` §7.3).
///
/// Flattened from the wire: the engine nests what follows the name under `input`
/// (`{ hint }`), and the palette only ever wants the hint — so the nesting does not
/// cross into the frontend.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandSnapshot {
    pub name: String,
    /// `builtin`, `custom`, `extension` or `file`. Open by contract, so a source this
    /// build has not seen renders under its own heading.
    pub source: String,
    /// Alternative names, for **matching only** — never inserted into a message.
    pub aliases: Vec<String>,
    pub description: Option<String>,
    /// What the command wants after its name; the palette's signal not to dispatch it on
    /// a single keypress.
    pub hint: Option<String>,
    pub subcommands: Vec<SubcommandSnapshot>,
}

/// A second level under a command that has one.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubcommandSnapshot {
    pub name: String,
    pub description: Option<String>,
}

/// A dialog the agent is waiting on, flattened for the frontend.
///
/// `kind` is the wire method (`select`, `confirm`, `input`, `editor`) as a plain
/// string so the UI can switch on it without a second vocabulary.
///
/// The approval dialog's content requirements (`docs/12` §10) — tool name, the
/// rendered argument summary, the engine's reason — all arrive inside `title`;
/// the engine formats them, and splitting them would mean re-parsing text the
/// engine already laid out.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiRequestSnapshot {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub message: String,
    /// `select` labels, in the engine's presentation order.
    pub options: Vec<String>,
    pub prefill: Option<String>,
    /// The hint shown inside an `input`'s empty field.
    pub placeholder: Option<String>,
    /// The engine's deadline in milliseconds, when it set one. The dialog shows
    /// it so an auto-resolution is never a surprise (`docs/12` §10).
    pub timeout_ms: Option<u64>,
}

/// How the frontend answers a dialog.
///
/// Mirrors the wire response's shape: exactly one of `value`, `confirmed` or
/// `cancelled` is supplied. Resolving it into a response is the adapter's job
/// (`bridge.rs`), not this module's.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiAnswer {
    pub value: Option<String>,
    pub confirmed: Option<bool>,
    pub cancelled: Option<bool>,
    pub timed_out: Option<bool>,
}

/// An image the composer is sending with a message (`docs/12` §5.1, the bytes route).
///
/// Carries what the webview already prepared and nothing else. Fitting it under the
/// session's advertised frame limit is arithmetic on the window side (`lib/attachments.ts`)
/// because that is where the bytes are and where a canvas can re-encode them; a host that
/// did it again would be a second copy of one policy. The transport is the backstop, and
/// it refuses an oversized frame naming both sizes ([`ImageContent`](omp_transport::protocol::ImageContent)).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageIn {
    /// Base64 bytes, without a `data:` prefix.
    pub data: String,
    /// What the bytes are: `image/png`, `image/jpeg`, `image/gif` or `image/webp`.
    ///
    /// Not checked here. The engine accepts exactly those four and refuses the rest
    /// with its own message, which is a better error than one this layer could invent.
    pub mime_type: String,
}

/// One row of the conversation, flattened for the frontend.
///
/// Shaped by `docs/12` §3.1: the row kinds are `user`, `assistant`, `tool` and
/// `notice:<level>`, and thinking is carried separately because §3.1 renders it
/// collapsed and tinted rather than inline.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowSnapshot {
    pub role: String,
    /// What was said. Markdown for an assistant turn (`docs/12` §3.1).
    pub text: String,
    /// The reasoning behind an assistant turn, absent when there was none.
    pub thinking: Option<String>,
    /// True for an assistant row still receiving deltas, or a tool still running.
    pub streaming: bool,
    pub tool: Option<ToolSnapshot>,
    /// Images attached to the message, shown as thumbnails (`docs/12` §3.1).
    ///
    /// Empty for every row that is not a message with attachments.
    pub attachments: Vec<AttachmentSnapshot>,
    /// The engine's own type for a message the app did not write: `async-result` for a
    /// background job's delivered result (`docs/12` §9). `None` for ordinary messages.
    pub custom_type: Option<String>,
    /// The jobs this row accounts for — a delivery's, and empty for everything else.
    pub jobs: Vec<JobDeliverySnapshot>,
}

/// One job a delivery message reports on.
///
/// `jobId` is the same string the card that started the job carries in its
/// `details.async.jobId`, so a job row closes itself when this arrives rather than by
/// guessing from the text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobDeliverySnapshot {
    pub job_id: String,
    /// `bash` | `eval` | `task`.
    pub kind: String,
    pub duration_ms: u64,
    pub label: Option<String>,
}

/// An image attached to a message.
///
/// Carries the base64 payload itself: the engine puts the bytes on the block, so
/// there is nothing for the host to resolve or cache.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentSnapshot {
    /// `image/png`, `image/jpeg`, … as the engine named it.
    pub mime_type: String,
    /// Base64 bytes, without a `data:` prefix — the frontend builds the URL.
    pub data: String,
}

/// A tool card: identity, arguments, and outcome.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSnapshot {
    pub tool_call_id: String,
    pub tool_name: String,
    /// The model's stated intent for the call, when it gave one.
    pub intent: Option<String>,
    /// The engine's arguments as JSON text, empty when there were none.
    ///
    /// Text rather than a decoded tree because each tool's card reads different
    /// fields — `bash` a command, `read` a path — so the per-tool renderers
    /// (`docs/12` §3.2) parse what they understand and this contract stays free of
    /// wire types.
    pub args: String,
    /// The engine's structured payload as JSON text, empty when it attached none.
    ///
    /// This is where a card finds what `content` does not say: `edit` carries its
    /// unified diff here, and every tool that truncates carries the artifact
    /// record (`meta.truncation`) here. Same reasoning as `args` — text, not a
    /// decoded tree, because each card reads different fields of it.
    pub details: String,
    /// The result as displayable text; empty until the call ends.
    pub output: String,
    pub is_error: bool,
    pub finished: bool,
}

/// The conversation, replaced from `from` onward.
///
/// One operation covers every case: an append (`from` = the old length), a
/// streaming rewrite (`from` = the trailing row), a card settling behind later rows
/// (`from` = that card), and a reset (`from` = 0). A delta protocol would need the
/// frontend to track insertions and removals to stay aligned; this cannot drift.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowPatch {
    /// The first row that changed. A consumer truncates here and appends [`rows`].
    ///
    /// If it is beyond the consumer's own length, that consumer has missed a patch
    /// and must re-read the conversation — a gap it cannot fill from the patch
    /// alone.
    ///
    /// [`rows`]: Self::rows
    pub from: usize,
    pub rows: Vec<RowSnapshot>,
}

/// What the app was launched with.
///
/// A workspace argument (`omp-desktop /path/to/project`) opens that project
/// immediately — the same affordance as `code .`. Without it the window opens
/// idle, which is the right default for a GUI launched from a dock.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchContext {
    pub workspace: Option<String>,
    /// Who this window is running as, for the sidebar's footer (`docs/12` §2.2).
    ///
    /// The OS user rather than an account: the app has none, and the reference's footer row —
    /// a name over a plan — is the one place in its design that is about identity at all. What
    /// we can honestly put there is the local one, and `None` when the platform will not say.
    pub user: Option<String>,
    /// The app's own version, so the footer's second line is a fact rather than a decoration.
    pub version: String,
}

/// One live thread, as the sidebar's row renders it (`docs/12` §2.2).
///
/// Derived from the session on every read rather than stored: the dot is a question
/// about the sidecar (`is it streaming? is a dialog waiting? did the turn fail?`),
/// and a second copy of those answers is a second thing that can be stale.
///
/// `id` is the **engine's** session id (`get_state.sessionId`), not one the app
/// invents: it is what the on-disk catalogue lists and what a resume resolves back to
/// a session file. An app-owned id would need a mapping table nothing else uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSnapshot {
    pub id: String,
    /// The directory the sidecar was launched in.
    pub workspace: String,
    /// The session's name, when the engine has one.
    pub title: Option<String>,
    /// A turn is in flight.
    pub streaming: bool,
    /// Dialogs the agent is blocked on: the sidebar's amber dot.
    pub pending_approvals: usize,
    /// The last failed turn, as the conversation recorded it, and absent when there
    /// has been none.
    pub error: Option<String>,
}

/// One user message a fork can start from (`docs/12` §6.3).
///
/// Both directions of the same shape, which is why it is one type rather than two: the
/// engine answers `get_branch_messages` with `[{entryId, text}]` and this is what the
/// fork picker receives, unchanged. Hand-writing the pair would let a rename on one
/// side silently empty the picker, and the id is the only thing the follow-up `branch`
/// accepts, so a row whose `entryId` did not survive the trip would fail on click.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchTarget {
    /// The entry id, from `get_branch_messages` — the **only** source of valid ones
    /// (`get_messages` and `get_messages_page` carry none).
    pub entry_id: String,
    /// The message's text, as the picker lists it.
    pub text: String,
}

/// One session on disk, as the sidebar's catalogue lists it.
///
/// The store's own [`omp_store::SessionSummary`] with the parts a window cannot carry:
/// a `PathBuf` becomes a displayable string, and `lifecycle` becomes the engine's own
/// word for it so a badge and the engine's resume picker agree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummaryDto {
    pub id: String,
    /// The session file's absolute path, for the affordances that open or reveal it.
    pub path: String,
    /// The store bucket's name — a **hint**, not a path (see [`omp_store::SessionSummary`]).
    pub bucket: String,
    /// The cwd recorded in the session's own header; empty for old sessions.
    pub cwd: String,
    pub title: Option<String>,
    /// `auto` or `user`: the engine never overwrites a user title.
    pub title_source: Option<String>,
    pub parent_id: Option<String>,
    /// The header's `timestamp`, ISO-8601.
    pub created_at: Option<String>,
    /// The file's mtime in Unix milliseconds: what the list is sorted by.
    pub modified_at: u64,
    pub message_count: u64,
    pub size: u64,
    pub first_message: String,
    /// [`omp_store::Lifecycle::as_str`] — the engine's own status string.
    pub status: String,
    /// Pinned in the engine's own pin file (`session-pins.json`).
    pub pinned: bool,
    /// The app released this thread's sidecar for being idle (`docs/11` D5).
    ///
    /// App-owned, and derived from the live registry rather than stored with the session:
    /// it is a fact about *this process*, not about the file on disk — another window, or a
    /// restart, has suspended nothing. It exists so a released thread's row can never be
    /// drawn as a live one (`docs/12` §16).
    pub suspended: bool,
}

/// One project group in the sidebar (`docs/12` §2.1).
///
/// Derived from the sessions' own `cwd` rather than from the engine's project
/// registry, which holds a single entry on a store with nine buckets: the sessions are
/// where the projects actually are, and the registry only adds the one thing a session
/// cannot say — that the user asked for a project to be hidden.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSnapshot {
    pub path: String,
    /// The path's last segment, or the path itself when it has none.
    pub name: String,
    pub session_count: usize,
    pub hidden: bool,
}

/// One session-scoped event, and the thread it belongs to.
///
/// Every event a session emits is addressed to exactly one thread, and without the tag
/// two live sessions are indistinguishable on the wire: a row patch from one would be
/// folded into the other's conversation. The payload is unchanged from the
/// single-session shape, so the only thing a subscriber learns is which thread it is
/// looking at.
#[derive(Debug, Clone, Serialize)]
pub struct ThreadEvent<T> {
    /// The engine's session id: the same key the sidebar and the catalogue use.
    pub thread: String,
    pub payload: T,
}

/// One indexed record a search returned (`docs/12` §7.4).
///
/// `kind` is the reader's own spelling (`title`, `prompt`, `answer`, `thinking`, `tool`,
/// `result`) and `ordinal` is the position in the thread's message stream, carried so a hit
/// can be ordered and labelled. `text` is the record as the index holds it — capped at
/// 8 KiB by `omp_store::messages` — because the *row* a hit belongs to is found by its
/// text: the app's rows are its own reduction of the stream, and an ordinal would not
/// survive it (`frontend/src/lib/search.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchHit {
    /// The engine's session id, which is what opens the hit's thread.
    pub thread: String,
    pub kind: String,
    pub ordinal: u64,
    pub text: String,
}

/// How far the app's own search index has got (`docs/12` §7.4).
///
/// Two of these are the overlay's footer (`indexing 12/34`) and one is its spinner. The
/// index is the app's own — the engine has no cross-thread search at v18.2.6 — so this is
/// also the only place that says whether it exists yet at all: an index that cannot be
/// read answers `indexed: 0` rather than failing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    /// Sessions in the index.
    pub indexed: u32,
    /// Sessions in the store: the denominator of "indexing 12/34".
    pub total: u32,
    /// A scan is in flight.
    pub running: bool,
    /// Unix milliseconds of the last scan that reached the end, `None` before the first.
    pub completed_at: Option<u64>,
    /// Why the last pass did not reach the end, in the engine's own sentence.
    ///
    /// `None` means the index is as current as the last pass made it. `Some` means the footer's
    /// count is stale *and this is why* — without it, a corrupt index reads exactly like a
    /// store with nothing in it.
    pub error: Option<String>,
}

/// One todo, as the Todos tab renders it (`docs/12` §8.1).
///
/// `status` is an open string union rather than an enum: measured, the engine stores
/// whatever it is given and its own reference UI switches on `pending` / `in_progress` /
/// `completed` / `abandoned` / `blocked` without rejecting the rest. A closed enum here
/// would turn a status this build has not heard of into a task the panel cannot draw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoTaskSnapshot {
    pub content: String,
    pub status: String,
    /// What the task waits for, when it is blocked.
    ///
    /// Always on the wire — `null` when there is none, because the frontend's interface
    /// declares it as `string | null` — and never a blank string: the engine's own
    /// projection drops the field it has no value for.
    pub blocker: Option<String>,
}

/// A named group of todos: the unit the panel collapses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoPhaseSnapshot {
    pub name: String,
    pub tasks: Vec<TodoTaskSnapshot>,
}

/// One task on the way *back* to the engine, for the one write this panel offers.
///
/// A separate type from [`TodoTaskSnapshot`] on purpose: this is what the window sends,
/// and the engine answers with its own projection of it. One type for both directions
/// would invite rendering the request as though it were the answer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoTaskInput {
    pub content: String,
    pub status: String,
    #[serde(default)]
    pub blocker: Option<String>,
}

/// A phase on the way back to the engine.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoPhaseInput {
    pub name: String,
    pub tasks: Vec<TodoTaskInput>,
}

/// One entry of one level of the workspace tree (`docs/12` §8.2's Tree view).
///
/// Deliberately unopinionated: every entry the filesystem has is listed, including
/// `.git` and `node_modules`. Hiding either is a display decision, and a host that made
/// it would be answering a question the panel did not ask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEntry {
    /// The entry's own name, without the directory in front of it.
    pub name: String,
    /// Absolute, and inside the thread's workspace: the panel passes it straight back to
    /// list the next level.
    pub path: String,
    pub is_dir: bool,
    /// Size in bytes; 0 for a directory, whose size is not a thing the tree draws.
    pub size: u64,
    /// Unix milliseconds, 0 when the platform will not say.
    pub modified_at: u64,
}

/// A tool result the engine spilled to `artifact://<id>`, read back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSnapshot {
    /// The id the row named, echoed back so a panel holding several reads can pair them.
    pub id: String,
    /// The file actually read, for a "reveal in file manager" affordance.
    pub path: String,
    /// The file's real size — which is the size of the *untruncated* result, so this is
    /// how a user learns what the card left out.
    pub bytes: u64,
    /// Decoded lossily: an artifact is a log, and a log can hold bytes that are not UTF-8.
    pub text: String,
    /// True when [`crate::panel`]'s cap, not the file, decided where `text` stops.
    pub truncated: bool,
}

/// The event name the frontend subscribes to.
pub const ACTIVITY_EVENT: &str = "session-activity";

/// One subagent, as the agents panel lists it (`docs/12` §9).
///
/// A flattened view of `omp_session::Subagent`: the panel needs one shape to render, and
/// the reducer's nesting (`progress` inside a row, retries inside that) is a decoder's
/// convenience rather than a renderer's. Fields keep the engine's own names and units —
/// microseconds are not milliseconds and the panel does not convert.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSnapshot {
    pub id: String,
    /// Dispatch order within a `task` call, which is the batch's own order.
    pub index: u64,
    pub agent: String,
    /// `bundled` | `user` | `project`, as an open string.
    pub agent_source: Option<String>,
    /// The engine's status vocabulary: `pending` | `running` | `completed` | `failed` |
    /// `aborted`. `None` when no frame and no list carried one, which is a state the panel
    /// must render as "unknown" rather than pick a value for.
    pub status: Option<String>,
    pub description: Option<String>,
    pub task: Option<String>,
    pub assignment: Option<String>,
    /// The subagent's own session file: what its transcript is read from.
    pub session_file: Option<String>,
    /// The `task` tool call that spawned it, for the jump back into the transcript.
    pub parent_tool_call_id: Option<String>,
    /// Whether it runs detached: the parent turn kept working while it did.
    pub detached: bool,
    /// When *this host* last heard about the row (Unix milliseconds).
    pub last_update_ms: u64,
    /// Whether the engine's last `get_subagents` answer contained it.
    ///
    /// False on a running status means the engine stopped listing it without reporting how
    /// it ended — the panel says exactly that instead of inventing a status.
    pub listed: bool,
    pub progress: Option<AgentProgressSnapshot>,
}

/// What the executor last knew about a running subagent (`AgentProgress`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProgressSnapshot {
    pub last_intent: Option<String>,
    pub current_tool: Option<String>,
    pub current_tool_args: Option<String>,
    pub tool_count: u64,
    /// Assistant requests across the run: the engine's soft-budget counter.
    pub requests: u64,
    /// Lifetime tokens, excluding cache re-reads.
    pub tokens: u64,
    /// The latest turn's context size, and the window it is compared against.
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    /// Cumulative billing cost in USD.
    pub cost: f64,
    pub duration_ms: u64,
    /// `<provider>/<id>`, with a `:<level>` suffix when the level was explicit.
    pub resolved_model: Option<String>,
    pub resolved_thinking_level: Option<String>,
    /// Whether a live advisor was attached to this run.
    pub advisor: bool,
    pub retry: Option<AgentRetrySnapshot>,
    pub retry_failure: Option<AgentRetrySnapshot>,
}

/// An auto-retry in flight, or the one that ended the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRetrySnapshot {
    pub attempt: u64,
    /// Zero on a give-up, which carries no cap (`retryFailure`).
    pub max_attempts: u64,
    pub delay_ms: u64,
    pub error_message: String,
    pub started_at_ms: u64,
}

/// One subagent transcript found on disk (`docs/12` §9's parked rows).
///
/// Everything here is a fact about a file, not about a run: the engine stops answering for
/// a settled subagent, and what it left behind is the transcript it wrote. What the agent
/// was *asked* to do is not in here — the parent's own transcript has that, on the `task`
/// card carrying the same id, and the panel joins the two rather than guessing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParkedAgent {
    /// The agent id: the transcript's file stem, which is also the id in the parent's card
    /// and in the frames that reported the run.
    pub id: String,
    /// The transcript file itself, for "reveal in file manager" and for the read.
    pub path: String,
    pub bytes: u64,
    /// The file's modification time (Unix milliseconds) — when the agent last wrote, which
    /// is the closest thing to "when this ran" that needs no date parsing.
    pub modified_ms: u64,
    /// Where the subagent ran, from its own session header. Empty when the header has none.
    pub cwd: String,
    /// The `parentSession` its header recorded, verbatim: a session **file path** for a
    /// nested child, and `None` for a subagent this session spawned directly.
    pub parent: Option<String>,
    /// True for an advisor transcript, which is a second opinion rather than a peer agent.
    pub advisor: bool,
    /// The advisor's slug, for the named form (`__advisor.<slug>.jsonl`).
    pub advisor_slug: Option<String>,
}

/// One page of a subagent's transcript (`docs/12` §9).
///
/// Rows are the app's own [`RowSnapshot`], deliberately: a subagent's transcript is a
/// session of the same kind, so it renders through the same conversation rows — the same
/// tool cards, diffs and markdown — rather than through a second, lesser viewer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTranscript {
    /// The agent this page belongs to, echoed so a pane holding two reads can pair them.
    pub agent: String,
    /// The file the page came from.
    pub path: String,
    /// Where the next page starts. Pass it back for the delta.
    pub next_byte: u64,
    /// The file shrank under the cursor, so this page restarts at zero and **replaces**
    /// what the pane was showing.
    pub reset: bool,
    /// Where the page came from: the engine, or the file the engine would not read.
    pub source: String,
    pub rows: Vec<RowSnapshot>,
}

/// One broker scope from `omp ps --json`.
///
/// A scope is a project (or the shared `global` one) the engine's broker supervises daemons
/// under. The shape is the engine's serializer verbatim, measured against a real run
/// (`cli/ps-cli.ts`): the scope's own fields, and `daemons` inside it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerScope {
    /// `project` | `global`, as the engine labels the scope.
    pub kind: String,
    /// The directory the scope belongs to. Empty for a global scope.
    pub project_dir: String,
    pub runtime_dir: String,
    /// The supervising broker's pid. `None` when the engine wrote no pid file, which is
    /// what a scope with no live broker looks like.
    pub broker_pid: Option<u64>,
    pub daemons: Vec<BrokerDaemon>,
}

/// One supervised process.
///
/// Every field is one the engine wrote in its own snapshot (`ps-cli.ts`'s serializer
/// spreads the daemon's persisted record), and the two the collector adds: `command`,
/// `cwd` and `supervised`. Fields the engine keeps but nothing here renders — the daemon's
/// `id`, the `readyMatch` pattern, the retained log's path — are deliberately not carried:
/// a decoder that forwards what no view shows is how a DTO becomes a second schema to
/// maintain against upstream.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerDaemon {
    /// The name `omp ps stop <name>` takes.
    pub name: String,
    /// `running` | `exited` | …, verbatim: the engine's own lifecycle word.
    pub state: String,
    /// The session that asked for it, when one did — a session id, so the panel can say
    /// which thread a helper belongs to rather than attributing it to the project.
    pub owner: Option<String>,
    pub command: String,
    pub cwd: String,
    /// Whether the broker restarts it when it dies.
    pub supervised: bool,
    /// Whether it outlives the session that started it.
    pub persist: bool,
    /// Whether it outlives every omp process (`detached`).
    pub detached: bool,
    pub restart_count: u64,
    /// The exit code, when it has exited. `None` while it runs.
    pub exit_code: Option<i64>,
    /// Bytes of output the engine has kept for it.
    pub output_bytes: u64,
    /// When it started, and when it exited (Unix milliseconds; 0 / `None` when unset).
    pub started_at_ms: u64,
    pub exited_at_ms: Option<u64>,
}

/// Every agent the app can currently account for, one entry per thread.
///
/// Thread-tagged inside the payload rather than by event name: the panel is **global**
/// (`docs/12` §9 — never project-scoped), so one answer describes several threads, and a
/// per-thread event name would need one listener per thread and still not carry the set.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadAgents {
    /// The engine's session id, which is also the sidebar row this joins against.
    pub thread: String,
    pub agents: Vec<AgentSnapshot>,
    /// Why this thread's roster cannot be read, when the engine refused the subscription.
    ///
    /// An empty roster and a roster that cannot be read look identical otherwise, and one
    /// of them is a broken promise while the other is the truth.
    pub error: Option<String>,
}

/// Carries the full pending set whenever it changes.
///
/// The whole set rather than a delta: it is at most a handful of dialogs, and a
/// delta would let the UI's idea of what is pending drift from the engine's.
pub const UI_REQUESTS_EVENT: &str = "session-ui-requests";

/// Carries a [`RowPatch`] whenever the conversation changes.
///
/// Coalesced by the publisher, so a streaming turn arrives at a screen's rate
/// rather than a token's.
pub const ROWS_EVENT: &str = "session-rows";

/// Carries a [`ModelCatalogue`] whenever a catalogue fetch lands.
///
/// Fired on a failed fetch too: the picker's refreshing state has to clear either way,
/// and the payload's `refreshing` plus the rows it still holds say what happened.
pub const MODELS_EVENT: &str = "models-updated";

/// The palette's list, after the engine said it changed.
///
/// `docs/12` §7.3: the engine pushes the array when command *metadata* changes, which is
/// how a command from a file or an extension appears without a restart.
pub const COMMANDS_EVENT: &str = "commands-updated";

/// The live-thread roster, whenever the set or any thread's state changes.
///
/// The whole roster rather than a delta, and **not** thread-tagged: the sidebar draws
/// one row per open thread, so what it renders is the set, which no single thread can
/// describe on its own.
pub const THREADS_EVENT: &str = "threads-updated";

/// One thread's agent roster, whenever it changes (`docs/12` §9).
///
/// Tagged with the thread, because the panel shows every thread's agents side by side and
/// a thread's roster is exactly what changes when a subagent starts, reports or settles.
pub const AGENTS_EVENT: &str = "session-agents";

/// Carries an [`IndexStatus`] while the app's own search index is being built.
///
/// Not thread-tagged either, and for the same reason: one index covers every session, so
/// a scan's progress describes the app rather than any thread in it. Emitted when a scan
/// starts, at most a few times a second while it runs, and once when it finishes.
pub const INDEX_EVENT: &str = "index-progress";

/// One app-owned terminal, as the panel's tabs list it (`docs/12` §11, decision D6).
///
/// A fact about a *process*, not about a thread: the terminal is the user's shell, started
/// in a workspace, and it outlives every session switch. Nothing here describes the agent —
/// the engine's `bash` runs under `PI_NO_PTY=1` and can neither see nor drive this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSnapshot {
    /// Minted by the host, and the key every other terminal command takes.
    pub id: String,
    /// The directory the shell was started in — the tab's default label.
    pub cwd: String,
    /// Whether the shell is still running.
    ///
    /// A tab outlives its shell (the scrollback is the point of keeping it), so this is what
    /// separates a live terminal from one showing how it ended.
    pub running: bool,
    /// How the shell ended, once it has.
    pub exit: Option<TerminalExit>,
    /// The shell's process id, while it is meaningful.
    pub pid: Option<u32>,
}

/// How a terminal's shell ended.
///
/// Exactly one of the two, because that is what the kernel reports: a process either returns
/// a code or dies of a signal, and portable-pty's own status carries a placeholder code for a
/// signal death that this deliberately does not pass on as a real exit code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalExit {
    pub code: Option<i32>,
    /// The signal's name, as the engine's platform spells it (`SIGHUP`).
    pub signal: Option<String>,
}

/// One batch of a terminal's output, base64 so it arrives as the bytes the shell wrote.
///
/// The hot path, and the only event the app sends at a program's own pace: a full-screen
/// repaint arrives as hundreds of small writes, which the host batches (`crate::pty`), and a
/// base64 string costs a third less than the same bytes as a JSON number array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutput {
    pub id: String,
    /// Raw terminal bytes: escape sequences included, and not necessarily UTF-8 — a terminal
    /// decodes them itself, which is why this is not a `String`.
    pub data: String,
}

/// Carries a [`TerminalOutput`] batch (the panel's per-tab stream).
pub const TERMINAL_OUTPUT_EVENT: &str = "terminal-output";

/// Carries the whole tab set whenever a terminal is opened, exits, or is closed.
///
/// The set rather than a delta, like the thread roster: it is a handful of rows, and the
/// panel renders exactly what it was last told.
pub const TERMINALS_EVENT: &str = "terminals-updated";

/// One fact the host is reporting about a thread (`docs/12` §13).
///
/// The host reports and the window decides: nothing here says whether a person should be
/// interrupted — no focus rule, no preference, no unread bookkeeping — because that decision
/// belongs to the surface that knows what is on screen and what the user asked for. `title`
/// and `body` are the engine's own words (the session's name, the dialog's own prompt, the
/// error's sentence), never a summary of them: a host that paraphrased would be inventing
/// the one thing a notification exists to carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationEvent {
    /// The engine's session id the fact belongs to.
    ///
    /// Empty when the fact cannot be attributed to one: a broker process the engine recorded
    /// **no** owner for leaves this blank, and the window says so rather than the host
    /// guessing a thread (`docs/12` §9's `owner` field is the only attribution there is).
    pub thread: String,
    /// `turn-finished`, `needs-you`, `failed` or `job-finished`.
    pub kind: String,
    pub title: String,
    /// The engine's own sentence, possibly cut to one line — see [`crate::notify`].
    pub body: String,
    /// The job a `job-finished` notification is about, when it is about one.
    ///
    /// The broker's own name for the daemon, which is also what `omp ps stop` takes: it is
    /// the only identity the broker list carries, so it is the one a window can act on.
    pub job_id: Option<String>,
}

/// Carries a [`NotificationEvent`] the moment one happens.
pub const NOTIFICATIONS_EVENT: &str = "session-notifications";

/// One fire-and-forget request from the engine, and the thread it came from.
///
/// The other half of the extension-UI vocabulary: `session-ui-requests` carries the dialogs
/// the engine *waits* on, and this carries the chrome it only wants drawn. The thread tag
/// matters for the same reason it does on every other stream — two live sessions share one
/// window, and a widget from one is not a widget for the other.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeEvent {
    pub thread: String,
    pub op: ChromeOp,
}

/// The extension-UI chrome op, as the transport decodes it.
///
/// Re-exported rather than mirrored, unlike everything else in this module: the six shapes
/// are the engine's closed vocabulary for a fire-and-forget request, and the frontend
/// switches on `kind` — a hand-written copy here would be a second place to keep the same
/// six spellings. The event *name* and the envelope are still this module's.
pub use omp_transport::protocol::ui::ChromeOp;

/// Carries a [`ChromeEvent`] whenever the engine pushes chrome.
pub const CHROME_EVENT: &str = "session-chrome";

// ──────────────────────────────────────────────────────── settings (`docs/12` §12, `docs/13`)

// The settings screen's contract is the `omp-settings` crate's own types, re-exported rather
// than mirrored.
//
// That is the opposite of this file's usual rule, and it is deliberate: the screen's shape is
// *derived from generated data* — 505 keys of catalog metadata decide what rows exist, which
// widget each gets and how a value is validated. A hand-written mirror here would be a second
// source of truth for all of it, and the catalog exists precisely to stop that.
//
// What the host adds on top is the wire naming: these are the exact payloads the four
// `settings_*` commands return.
pub use omp_settings::apply::{
    ApplyReport as SettingsApplyReport, ChangeResult as SettingsChangeResult,
    WriteOutcome as SettingsWriteOutcome,
};
pub use omp_settings::hatch::{
    Backup as SettingsBackup, Payload as SettingsHatch, Plan as SettingsHatchPlan,
    Refusal as SettingsRefusal,
};
pub use omp_settings::screen::{
    Drift as SettingsDrift, FileStateSummary as SettingsFile, Origin as SettingsOrigin,
    Row as SettingsRow, Screen as SettingsScreen, ScreenSection as SettingsSection,
    Sources as SettingsSources, Tone as SettingsTone,
};

/// What restarting live sessions did.
///
/// A session that is mid-turn is *not* restarted — killing a run the user is watching to
/// apply a setting they may not have finished reading is not a trade this app makes — so the
/// report says which ones were left, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsRestartReport {
    /// The sessions that were restarted and resumed.
    pub restarted: Vec<String>,
    /// The sessions that were left alone, with the reason.
    pub skipped: Vec<SettingsRestartSkip>,
}

/// One session the restart left alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsRestartSkip {
    pub thread: String,
    pub reason: String,
}
