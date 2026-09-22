//! The control-plane state: what the chrome around the conversation shows.
//!
//! Model and thinking level in the composer, the streaming/compacting flags,
//! queue modes, session identity, tokens per second, the context ring, and the
//! todo list.
//!
//! # Two sources, one cache
//!
//! `get_state` is authoritative but polled: it is 68 KB and reflects a snapshot.
//! Session events are live but partial: they carry *changes*, not the whole
//! state. So this type holds a [`get_state`] snapshot and applies the events that
//! make part of it stale sooner than the next poll would:
//!
//! * `thinking_level_changed` carries the new level, so it is applied directly;
//! * `agent_start` / `agent_end` flip `is_streaming` without waiting for a poll;
//! * `auto_compaction_*` flips `is_compacting`;
//! * the `todo` tool's end is the one event that says the plan moved;
//! * `model_changed` and `config_warnings_changed` carry **no payload**, so they
//!   cannot be applied — [`SessionControl::apply`] reports that a refresh is due
//!   and the caller re-reads `get_state`.
//!
//! [`get_state`]: omp_transport::protocol::commands::get_state

use omp_transport::events::SessionEvent;
use serde_json::Value;

/// Which model is selected, as the composer chip needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    pub provider: Option<String>,
    pub id: Option<String>,
    /// The display name, when the catalogue supplied one.
    pub name: Option<String>,
}

/// Context-window usage, for the ring and its popover.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContextUsage {
    pub tokens: u64,
    pub context_window: u64,
    /// Percentage of the window used, as the engine computed it.
    pub percent: f64,
}

/// One todo, as the Todos panel renders it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoTask {
    pub content: String,
    /// Open string union: `pending`, `in_progress`, `completed`, `abandoned`,
    /// `blocked`.
    pub status: String,
    /// What the task is waiting for, when blocked.
    pub blocker: Option<String>,
}

/// A named group of todos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoPhase {
    pub name: String,
    pub tasks: Vec<TodoTask>,
}

/// The engine's session state, decoded.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionControl {
    pub session_id: String,
    pub session_name: Option<String>,
    pub session_file: Option<String>,
    pub model: Option<ModelRef>,
    /// The level in effect; `None` means no level is selected.
    pub thinking_level: Option<String>,
    pub is_streaming: bool,
    pub is_compacting: bool,
    pub auto_compaction_enabled: bool,
    pub fast_mode_enabled: bool,
    pub fast_mode_active: bool,
    /// Open string union: `all`, `one-at-a-time`. How queued messages drain.
    pub steering_mode: String,
    pub follow_up_mode: String,
    /// Open string union: `immediate`, `wait`. Whether a steer aborts the
    /// remaining tool calls in the current batch.
    pub interrupt_mode: String,
    /// Measured output rate, `None` until the first tokens.
    pub tokens_per_second: Option<f64>,
    /// The engine's own message count — the number paging cursors are validated
    /// against. Not the transcript's row count.
    pub message_count: u64,
    /// How many messages are queued behind the running turn.
    pub queued_message_count: u64,
    pub context_usage: Option<ContextUsage>,
    pub todo_phases: Vec<TodoPhase>,
}

impl SessionControl {
    /// Decode a `get_state` response's `data`.
    ///
    /// Returns `None` when the payload has no `sessionId`: a state snapshot
    /// without a session identity is not one we can act on, and guessing would
    /// put a wrong session name in the UI.
    pub fn decode(raw: &Value) -> Option<Self> {
        let session_id = raw.get("sessionId").and_then(Value::as_str)?;

        Some(Self {
            session_id: session_id.to_string(),
            session_name: string(raw, "sessionName"),
            session_file: string(raw, "sessionFile"),
            model: raw.get("model").and_then(decode_model),
            thinking_level: string(raw, "thinkingLevel"),
            is_streaming: flag(raw, "isStreaming"),
            is_compacting: flag(raw, "isCompacting"),
            auto_compaction_enabled: flag(raw, "autoCompactionEnabled"),
            fast_mode_enabled: flag(raw, "fastModeEnabled"),
            fast_mode_active: flag(raw, "fastModeActive"),
            steering_mode: string(raw, "steeringMode").unwrap_or_default(),
            follow_up_mode: string(raw, "followUpMode").unwrap_or_default(),
            interrupt_mode: string(raw, "interruptMode").unwrap_or_default(),
            tokens_per_second: raw.get("tokensPerSecond").and_then(Value::as_f64),
            message_count: number(raw, "messageCount"),
            queued_message_count: number(raw, "queuedMessageCount"),
            context_usage: raw.get("contextUsage").and_then(decode_context_usage),
            todo_phases: raw.get("todoPhases").map(decode_phases).unwrap_or_default(),
        })
    }

    /// Apply a streamed event, and report whether it left fields stale.
    ///
    /// `true` means an event invalidated something this snapshot does not carry
    /// — currently `model_changed` and `config_warnings_changed`, which are
    /// pure notifications, and the end of a `todo` tool call, whose result is the
    /// plan. The caller should re-read `get_state`; ignoring the return value is the
    /// one way to end up showing a stale model chip or a plan the engine has moved on
    /// from.
    pub fn apply(&mut self, event: &SessionEvent) -> bool {
        match event {
            SessionEvent::ThinkingLevelChanged(change) => {
                self.thinking_level = change.thinking_level.clone();
            }
            SessionEvent::AgentStart => self.is_streaming = true,
            SessionEvent::AgentEnd(end) => {
                if end.is_terminal() {
                    self.is_streaming = false;
                }
            }
            SessionEvent::AutoCompactionStart(_) => self.is_compacting = true,
            SessionEvent::AutoCompactionEnd(_) => self.is_compacting = false,
            // There is no todo *event* — the engine's stream carries `todo_reminder`
            // and `todo_auto_clear` and nothing else — so the end of a tool call is the
            // only thing on the wire that says the plan moved. That result does carry
            // the plan (`details.phases`, which is how a cold thread still shows one),
            // but a re-read is authoritative and already plumbed, so this arm only
            // reports that one is due.
            SessionEvent::ToolExecutionEnd(end) if end.tool_name == "todo" => return true,
            SessionEvent::ModelChanged | SessionEvent::ConfigWarningsChanged => return true,
            _ => {}
        }

        false
    }
}

fn decode_model(raw: &Value) -> Option<ModelRef> {
    // `model` is a full catalogue row; only identity is surfaced here.
    Some(ModelRef {
        provider: string(raw, "provider"),
        id: string(raw, "id"),
        name: string(raw, "name"),
    })
}

fn decode_context_usage(raw: &Value) -> Option<ContextUsage> {
    Some(ContextUsage {
        tokens: number(raw, "tokens"),
        context_window: number(raw, "contextWindow"),
        percent: raw.get("percent").and_then(Value::as_f64)?,
    })
}

/// Decode a phase array.
///
/// Shared by the two places the plan arrives in the same shape: `get_state`'s
/// `todoPhases`, and the `todoPhases` a `set_todos` answer echoes back
/// (`rpc-mode.ts`'s handler stores the request, then answers with
/// `session.getTodoPhases()`). One decoder means a plan cannot look different
/// depending on which of the two the panel happened to read.
///
/// Tolerant of anything that is not an array: a payload without a plan is an empty
/// plan, which is what a session that has never written a todo has too.
pub fn decode_phases(raw: &Value) -> Vec<TodoPhase> {
    raw.as_array()
        .map(|phases| phases.iter().map(decode_phase).collect())
        .unwrap_or_default()
}

fn decode_phase(raw: &Value) -> TodoPhase {
    TodoPhase {
        name: string(raw, "name").unwrap_or_default(),
        tasks: raw
            .get("tasks")
            .and_then(Value::as_array)
            .map(|tasks| {
                tasks
                    .iter()
                    .map(|task| TodoTask {
                        content: string(task, "content").unwrap_or_default(),
                        status: string(task, "status").unwrap_or_default(),
                        blocker: string(task, "blocker"),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn string(raw: &Value, key: &str) -> Option<String> {
    raw.get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn flag(raw: &Value, key: &str) -> bool {
    raw.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn number(raw: &Value, key: &str) -> u64 {
    raw.get(key).and_then(Value::as_u64).unwrap_or_default()
}
