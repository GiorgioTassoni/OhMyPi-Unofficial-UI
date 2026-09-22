//! Session state for the desktop app: the transcript, the control panel, and
//! history restore.
//!
//! This crate sits between [`omp_transport`] and the Tauri shell, and it is the
//! only layer that interprets what the engine said. The transport answers "what
//! arrived"; this answers "what does the app show". It knows nothing about
//! widgets, and — deliberately — nothing about frames, chunking or `id`
//! correlation (`docs/11-v1-scope.md` §3.2, where this layer is called `state`).
//!
//! ```text
//!   omp-transport   wire: frames, chunks, requests, typed events
//!        │
//!   omp-session     ← this crate: message model, transcript, control, restore
//!        │
//!   Tauri shell     thin adapters, one per command
//! ```
//!
//! # Layout
//!
//! * [`messages`] — the domain view of a message: roles, content blocks, tool
//!   results. Tolerant: unknown parts are preserved, never fatal.
//! * [`transcript`] — the conversation as rows, reduced from the event stream or
//!   rebuilt from restored history, with the same result either way.
//! * [`control`] — the `get_state` snapshot plus the event-driven liveness
//!   updates that make parts of it stale sooner than the next poll.
//! * [`restore`] — paging a session's history, including the engine's two
//!   documented refusals (`session_busy`, `stale_cursor`).
//! * [`ui_requests`] — the dialogs the engine is waiting on, and the one rule
//!   they must obey: exactly one answer each.
//! * [`agents`] — the subagents a session is running, from the frames that push them
//!   and the list the engine answers with.
//!
//! # The rule these all follow
//!
//! Streaming is for liveness; the engine is for truth. Deltas drive animation,
//! but `message_end` and the restored history replace what they accumulated. A
//! client that trusts its own reconstruction over the engine's own message will
//! eventually show a transcript the engine never had.

pub mod agents;
pub mod control;
pub mod messages;
pub mod restore;
pub mod transcript;
pub mod ui_requests;

pub use agents::{
    read_transcript, AgentProgress, AgentRoster, AgentStatus, Subagent, TranscriptPage,
};
pub use control::{decode_phases, ContextUsage, ModelRef, SessionControl, TodoPhase, TodoTask};
pub use messages::{ContentBlock, JobDelivery, Message, MessageKind, ToolResultContent};
pub use restore::{restore_history, PageSource, RestoreError, RestoreOutcome, RestorePolicy};
pub use transcript::{Row, ToolCard, ToolOutcome, Transcript};
pub use ui_requests::{AnswerError, UiRequests};
