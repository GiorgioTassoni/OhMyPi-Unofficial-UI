//! Typed decoding of the engine's streamed session events.
//!
//! The contract is `AgentSessionEvent` — 28 variants at `omp` v18.2.6, defined
//! by `packages/coding-agent/src/session/agent-session-events.ts` as
//! `pi-agent-core`'s `AgentEvent` (11 variants) extended with 17 session-level
//! additions. [`MessageUpdate`] additionally carries an `AssistantMessageEvent`,
//! the 13 token-delta variants from `pi-ai`.
//!
//! # Rules
//!
//! 1. **Decoding never fails.** An unrecognised `type` becomes
//!    [`SessionEvent::Unknown`]; a recognised `type` whose payload does not match
//!    becomes [`SessionEvent::Malformed`]. Both keep the original frame and both
//!    are recoverable. The engine adds event types between releases, and a
//!    desktop app pinned to a digest still has to survive the day someone bumps
//!    that digest — a stream that dies on the first surprise is worse than one
//!    that reports what it did not understand.
//! 2. **Field names are the wire names**, mechanically snake_cased, so a payload
//!    struct can be diffed against the upstream union by eye.
//! 3. **Model what we branch on; pass through what we render.** Control fields
//!    (ids, names, counters, levels, `isTerminal`) are typed. Content — messages,
//!    tool arguments, results, todos, rules — is carried as [`Value`]. Declaring
//!    those internal shapes here would couple this crate to the engine's
//!    internals, which `docs/11-v1-scope.md` §3.2 forbids; the app decides how to
//!    read them.
//! 4. **Open string unions stay strings** — [`Notice::level`], compaction
//!    `reason`/`action`, stop reasons. A new upstream value then renders with a
//!    default presentation instead of failing a decode.
//!
//! Message *shapes* are deliberately absent: the same message object arrives via
//! `message_end`, `get_messages_page` and `get_state`, so the transcript model
//! owns it (`docs/14-build-plan.md` §1 step 5). This module is the control plane
//! only.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// Every session event type this build understands.
///
/// This list does not drive dispatch — the enum does. It exists to *classify* a
/// failed decode, which decides the diagnostic: a kind listed here that fails to
/// decode is a payload mismatch (our model is behind the engine), while a kind
/// absent from it is an event type we have never seen. The test suite asserts
/// that every listed kind decodes to its own variant, so the list cannot rot
/// silently.
pub const KNOWN_EVENT_KINDS: [&str; 28] = [
    "advisor_cost_changed",
    "advisor_yielded",
    "agent_end",
    "agent_start",
    "auto_compaction_end",
    "auto_compaction_start",
    "auto_retry_end",
    "auto_retry_start",
    "config_warnings_changed",
    "goal_updated",
    "irc_message",
    "message_end",
    "message_start",
    "message_update",
    "model_changed",
    "notice",
    "retry_fallback_applied",
    "retry_fallback_succeeded",
    "thinking_level_changed",
    "todo_auto_clear",
    "todo_reminder",
    "tool_execution_end",
    "tool_execution_start",
    "tool_execution_update",
    "tool_stream_update",
    "ttsr_triggered",
    "turn_end",
    "turn_start",
];

/// One streamed `AgentSessionEvent`.
///
/// The wire `type` of each variant is the `snake_case` of its name; the test
/// suite pins that mapping by decoding a sample of every kind.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEvent {
    // ---- agent and turn lifecycle ---------------------------------------
    /// A run began.
    AgentStart,
    /// A run ended; inspect [`AgentEnd::is_terminal`] before treating it as final.
    AgentEnd(AgentEnd),
    /// One assistant response plus any tool calls began.
    TurnStart,
    /// That turn finished.
    TurnEnd(TurnEnd),

    // ---- message lifecycle ----------------------------------------------
    /// A message was appended (user, assistant, or tool result).
    MessageStart(MessagePayload),
    /// An assistant message grew by one token delta.
    MessageUpdate(MessageUpdate),
    /// A message was finalised.
    MessageEnd(MessagePayload),

    // ---- tool execution -------------------------------------------------
    /// A tool began.
    ToolExecutionStart(ToolExecutionStart),
    /// An in-flight tool produced a partial result.
    ToolExecutionUpdate(ToolExecutionUpdate),
    /// An in-flight tool emitted a streaming update. Distinct from
    /// [`SessionEvent::ToolExecutionUpdate`]: this is the live output channel
    /// (progress lines, incremental file contents), not a partial *result*.
    ToolStreamUpdate(ToolStreamUpdate),
    /// A tool finished.
    ToolExecutionEnd(ToolExecutionEnd),

    // ---- compaction and retry -------------------------------------------
    /// Automatic compaction began.
    AutoCompactionStart(AutoCompactionStart),
    /// Automatic compaction finished, was aborted, or was skipped.
    AutoCompactionEnd(AutoCompactionEnd),
    /// A failed request is being retried.
    AutoRetryStart(AutoRetryStart),
    /// The retry sequence finished.
    AutoRetryEnd(AutoRetryEnd),
    /// A provider fallback was applied for a role.
    RetryFallbackApplied(RetryFallbackApplied),
    /// A provider fallback produced a usable response.
    RetryFallbackSucceeded(RetryFallbackSucceeded),

    // ---- session state --------------------------------------------------
    /// The active model changed. Carries no payload: re-read `get_state`.
    ModelChanged,
    /// The effective or configured thinking level changed.
    ThinkingLevelChanged(ThinkingLevelChanged),
    /// Configuration warnings changed. Carries no payload: re-read `get_state`.
    ConfigWarningsChanged,
    /// Advisor cost accounting changed. Carries no payload.
    AdvisorCostChanged,
    /// The advisor yielded this turn. Carries no payload.
    AdvisorYielded,
    /// Time-travel-style rules fired; the rules are passed through opaquely.
    TtsrTriggered(TtsrTriggered),
    /// The engine nudged the agent about unfinished todos.
    TodoReminder(TodoReminder),
    /// The engine cleared the todo list itself.
    TodoAutoClear,
    /// A message from another agent arrived over the hub.
    IrcMessage(IrcMessage),
    /// A status message for the user.
    Notice(Notice),
    /// Goal-mode state changed.
    GoalUpdated(GoalUpdated),

    // ---- diagnostic fallbacks -------------------------------------------
    /// An event type this build does not know. Not an error: the frame is kept
    /// verbatim so the app can log it and keep going.
    #[serde(skip)]
    Unknown {
        /// The wire `type` that was not recognised.
        kind: String,
        /// The frame, unmodified.
        raw: Value,
    },
    /// A known event type whose payload did not match this build's model —
    /// either upstream changed the shape or our model is wrong. Surfaced
    /// rather than swallowed; the frame is kept verbatim.
    #[serde(skip)]
    Malformed {
        /// The recognised wire `type` that failed to decode.
        kind: String,
        /// The frame, unmodified.
        raw: Value,
        /// The decoder's account of what did not match.
        reason: String,
    },
}

impl SessionEvent {
    /// Decode one event frame.
    ///
    /// This is the entry point; it cannot fail. Use it rather than the derived
    /// `Deserialize`, which is strict and exists only to be called from here.
    ///
    /// No allocation on the success path: `&Value` is itself a deserializer, so
    /// the frame is read in place. Clone-free decoding matters because every
    /// streamed delta passes through here.
    pub fn decode(raw: &Value) -> Self {
        match Self::deserialize(raw) {
            Ok(event) => event,
            Err(error) => Self::fallback(raw, &error),
        }
    }

    /// The wire `type` of this event.
    ///
    /// For [`SessionEvent::Unknown`] and [`SessionEvent::Malformed`] this is the
    /// `type` that could not be handled, which is what a diagnostic needs.
    pub fn kind(&self) -> &str {
        match self {
            Self::AgentStart => "agent_start",
            Self::AgentEnd(_) => "agent_end",
            Self::TurnStart => "turn_start",
            Self::TurnEnd(_) => "turn_end",
            Self::MessageStart(_) => "message_start",
            Self::MessageUpdate(_) => "message_update",
            Self::MessageEnd(_) => "message_end",
            Self::ToolExecutionStart(_) => "tool_execution_start",
            Self::ToolExecutionUpdate(_) => "tool_execution_update",
            Self::ToolStreamUpdate(_) => "tool_stream_update",
            Self::ToolExecutionEnd(_) => "tool_execution_end",
            Self::AutoCompactionStart(_) => "auto_compaction_start",
            Self::AutoCompactionEnd(_) => "auto_compaction_end",
            Self::AutoRetryStart(_) => "auto_retry_start",
            Self::AutoRetryEnd(_) => "auto_retry_end",
            Self::RetryFallbackApplied(_) => "retry_fallback_applied",
            Self::RetryFallbackSucceeded(_) => "retry_fallback_succeeded",
            Self::ModelChanged => "model_changed",
            Self::ThinkingLevelChanged(_) => "thinking_level_changed",
            Self::ConfigWarningsChanged => "config_warnings_changed",
            Self::AdvisorCostChanged => "advisor_cost_changed",
            Self::AdvisorYielded => "advisor_yielded",
            Self::TtsrTriggered(_) => "ttsr_triggered",
            Self::TodoReminder(_) => "todo_reminder",
            Self::TodoAutoClear => "todo_auto_clear",
            Self::IrcMessage(_) => "irc_message",
            Self::Notice(_) => "notice",
            Self::GoalUpdated(_) => "goal_updated",
            Self::Unknown { kind, .. } | Self::Malformed { kind, .. } => kind,
        }
    }

    /// Whether this event ends the run for good.
    ///
    /// Only an `agent_end` can end a run, and only when it is terminal: the
    /// engine reports `isTerminal: false` when an async delivery has scheduled
    /// more work, so the conversation is still live.
    pub fn ends_run(&self) -> bool {
        matches!(self, Self::AgentEnd(end) if end.is_terminal())
    }

    /// Classify a failed decode. See [`KNOWN_EVENT_KINDS`].
    fn fallback(raw: &Value, error: &serde_json::Error) -> Self {
        let (kind, malformed) = classify_failure(raw, &KNOWN_EVENT_KINDS, error);
        match malformed {
            Some(reason) => Self::Malformed {
                kind,
                raw: raw.clone(),
                reason,
            },
            None => Self::Unknown {
                kind,
                raw: raw.clone(),
            },
        }
    }
}

/// Decide whether a failed decode is *our* problem or simply an unfamiliar
/// event type, given the kinds this build knows.
///
/// Returns the wire `type` observed and, when the failure is a payload mismatch,
/// the reason to report. Shared by [`SessionEvent`] and [`MessageDelta`] so the
/// two fallbacks cannot disagree about what counts as unknown.
fn classify_failure(
    raw: &Value,
    known: &[&str],
    error: &serde_json::Error,
) -> (String, Option<String>) {
    let kind = raw.get("type").and_then(Value::as_str).unwrap_or_default();

    // A frame with no usable `type` is malformed, not unfamiliar: there is no
    // type for us to fail to recognise.
    if kind.is_empty() {
        return (
            String::new(),
            Some("frame has no string `type`".to_string()),
        );
    }

    if known.contains(&kind) {
        return (kind.to_string(), Some(error.to_string()));
    }

    (kind.to_string(), None)
}

/// Payload of [`SessionEvent::AgentEnd`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEnd {
    /// `false` when an async delivery will resume the session before its true
    /// final settle. Absent means terminal, for compatibility with older
    /// runtimes — which is why this is the one boolean that stays optional
    /// instead of collapsing to a default.
    pub is_terminal: Option<bool>,
    /// The messages the run produced, passed through opaquely.
    pub messages: Vec<Value>,
    /// Run summary, present only when telemetry was configured.
    pub telemetry: Option<Value>,
    /// Prompt-coverage summary, present only when telemetry was configured.
    pub coverage: Option<Value>,
}

impl AgentEnd {
    /// The documented completion rule: an absent `isTerminal` is terminal.
    pub fn is_terminal(&self) -> bool {
        self.is_terminal.unwrap_or(true)
    }
}

/// Payload of [`SessionEvent::TurnEnd`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnEnd {
    /// The assistant message that closed the turn.
    pub message: Value,
    /// Tool results produced during the turn.
    pub tool_results: Vec<Value>,
}

/// Payload of [`SessionEvent::MessageStart`] and [`SessionEvent::MessageEnd`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePayload {
    /// The message, passed through opaquely.
    pub message: Value,
}

/// Payload of [`SessionEvent::MessageUpdate`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageUpdate {
    /// The message as it stands after this delta.
    ///
    /// Carried so a client that missed a delta can resynchronise instead of
    /// silently rendering truncated text. This is why the individual deltas need
    /// not repeat it.
    pub message: Value,
    /// The delta just applied.
    #[serde(deserialize_with = "de_message_delta")]
    pub assistant_message_event: MessageDelta,
}

/// Payload of [`SessionEvent::ToolExecutionStart`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionStart {
    /// Correlates every later update and the end event for this call.
    pub tool_call_id: String,
    /// The tool's name, as advertised in `get_state`'s `dumpTools`.
    pub tool_name: String,
    /// The arguments, passed through opaquely.
    pub args: Value,
    /// The model's stated intent, when it gave one.
    pub intent: Option<String>,
}

/// Payload of [`SessionEvent::ToolExecutionUpdate`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionUpdate {
    /// Correlates with the call's start and end events.
    pub tool_call_id: String,
    pub tool_name: String,
    /// The arguments, repeated on every update.
    pub args: Value,
    /// The partial result, passed through opaquely.
    pub partial_result: Value,
}

/// Payload of [`SessionEvent::ToolStreamUpdate`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStreamUpdate {
    /// Correlates with the call's start and end events.
    pub tool_call_id: String,
    pub tool_name: String,
    /// The streaming update, passed through opaquely.
    pub update: Value,
}

/// Payload of [`SessionEvent::ToolExecutionEnd`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionEnd {
    /// Correlates with the call's start event.
    pub tool_call_id: String,
    pub tool_name: String,
    /// The result, passed through opaquely.
    pub result: Value,
    /// Whether the tool failed. Optional on the wire and read as falsy when
    /// absent — the same way the engine's own consumers read it.
    #[serde(default)]
    pub is_error: bool,
}

/// Payload of [`SessionEvent::AutoCompactionStart`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoCompactionStart {
    /// Why compaction ran. An open string union: `threshold`, `overflow`,
    /// `idle`, `incomplete`.
    pub reason: String,
    /// Which compaction strategy ran. An open string union: `context-full`,
    /// `remote`, `handoff`, `shake`, `snapcompact`.
    pub action: String,
}

/// Payload of [`SessionEvent::AutoCompactionEnd`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoCompactionEnd {
    /// Which compaction strategy ran; see [`AutoCompactionStart::action`].
    pub action: String,
    /// The compaction outcome, passed through opaquely. `undefined` on the wire
    /// for strategies that report nothing, hence optional here.
    pub result: Option<Value>,
    /// Whether the compaction was aborted before finishing.
    #[serde(default)]
    pub aborted: bool,
    /// Whether the run will retry now that context has room.
    #[serde(default)]
    pub will_retry: bool,
    /// Why compaction failed, when it did.
    pub error_message: Option<String>,
    /// Whether compaction was skipped for a benign reason.
    #[serde(default)]
    pub skipped: bool,
}

/// Payload of [`SessionEvent::AutoRetryStart`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoRetryStart {
    /// Which attempt is about to run, counting from 1.
    pub attempt: u64,
    pub max_attempts: u64,
    /// How long the engine will wait before retrying.
    pub delay_ms: u64,
    /// The provider error that triggered the retry.
    pub error_message: String,
    /// An id that can be looked up in the engine's error records, if given.
    pub error_id: Option<u64>,
}

/// Payload of [`SessionEvent::AutoRetryEnd`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoRetryEnd {
    /// Whether the retry sequence recovered.
    pub success: bool,
    /// The attempt this outcome belongs to.
    pub attempt: u64,
    /// The error that ended the sequence, when it did not recover.
    pub final_error: Option<String>,
    /// Per-attempt error detail, passed through opaquely. Absent means empty.
    #[serde(default)]
    pub retry_errors: Vec<Value>,
}

/// Payload of [`SessionEvent::RetryFallbackApplied`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryFallbackApplied {
    /// The model that failed.
    pub from: String,
    /// The model being tried instead.
    pub to: String,
    /// The role the fallback applies to.
    pub role: String,
}

/// Payload of [`SessionEvent::RetryFallbackSucceeded`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryFallbackSucceeded {
    /// The fallback model that answered.
    pub model: String,
    /// The role the fallback applied to.
    pub role: String,
}

/// Payload of [`SessionEvent::ThinkingLevelChanged`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingLevelChanged {
    /// The level now in effect.
    pub thinking_level: Option<String>,
    /// The user-configured selector, when it differs from the effective level.
    pub configured: Option<String>,
    /// The level `auto` resolved to this turn, once classified.
    pub resolved: Option<String>,
}

/// Payload of [`SessionEvent::TtsrTriggered`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsrTriggered {
    /// The rules that fired, passed through opaquely.
    pub rules: Vec<Value>,
}

/// Payload of [`SessionEvent::TodoReminder`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoReminder {
    /// The outstanding todos, passed through opaquely.
    pub todos: Vec<Value>,
    /// Which reminder this is, counting from 1.
    pub attempt: u64,
    pub max_attempts: u64,
}

/// Payload of [`SessionEvent::IrcMessage`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IrcMessage {
    /// The message, passed through opaquely.
    pub message: Value,
}

/// Payload of [`SessionEvent::Notice`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    /// Severity. An open string union: `info`, `warning`, `error`.
    pub level: String,
    /// The message to show.
    pub message: String,
    /// What produced the notice, when identified.
    pub source: Option<String>,
}

/// Payload of [`SessionEvent::GoalUpdated`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalUpdated {
    /// The goal, or `null` when goal mode was cleared.
    pub goal: Option<Value>,
    /// Goal-mode state, passed through opaquely.
    pub state: Option<Value>,
}

/// Every `assistantMessageEvent` type this build understands. See
/// [`KNOWN_EVENT_KINDS`] for why this list exists.
pub const KNOWN_DELTA_KINDS: [&str; 13] = [
    "done",
    "error",
    "image_end",
    "start",
    "text_delta",
    "text_end",
    "text_start",
    "thinking_delta",
    "thinking_end",
    "thinking_start",
    "toolcall_delta",
    "toolcall_end",
    "toolcall_start",
];

/// One `AssistantMessageEvent`: the token-level delta carried by
/// [`SessionEvent::MessageUpdate`].
///
/// Only the fields the app accumulates are modelled. The wire's `partial` field
/// — the whole message so far, repeated on every delta — is intentionally
/// dropped: [`MessageUpdate::message`] already carries it once per event, and
/// modelling it here would duplicate a payload that is mostly identical between
/// consecutive deltas.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum MessageDelta {
    /// An assistant message began.
    Start,
    /// A text block began at `content_index`.
    TextStart { content_index: u32 },
    /// Text arrived. Append `delta` to the block at `content_index`.
    TextDelta { content_index: u32, delta: String },
    /// The text block at `content_index` is complete.
    TextEnd { content_index: u32, content: String },
    /// A thinking block began at `content_index`.
    ThinkingStart { content_index: u32 },
    /// Thinking text arrived. Append `delta`.
    ThinkingDelta { content_index: u32, delta: String },
    /// The thinking block at `content_index` is complete.
    ThinkingEnd { content_index: u32, content: String },
    /// An image block at `content_index` is complete; the image is opaque.
    ImageEnd { content_index: u32, content: Value },
    /// A tool call began at `content_index`.
    #[serde(rename = "toolcall_start")]
    ToolCallStart { content_index: u32 },
    /// Tool-call arguments streamed in; `delta` is a fragment of the JSON.
    #[serde(rename = "toolcall_delta")]
    ToolCallDelta { content_index: u32, delta: String },
    /// The tool call at `content_index` is complete; the call is opaque.
    #[serde(rename = "toolcall_end")]
    ToolCallEnd {
        content_index: u32,
        tool_call: Value,
    },
    /// The message finished; `reason` is the stop reason.
    Done {
        /// An open string union, e.g. `stop`, `length`, `toolUse`.
        reason: String,
        /// The finished message, passed through opaquely.
        message: Value,
    },
    /// The message failed; `reason` is `aborted` or `error`.
    Error {
        reason: String,
        /// The failed message, passed through opaquely.
        error: Value,
    },
    /// A delta type this build does not know.
    #[serde(skip)]
    Unknown { kind: String, raw: Value },
    /// A known delta type whose payload did not match this build's model.
    #[serde(skip)]
    Malformed {
        kind: String,
        raw: Value,
        reason: String,
    },
}

impl MessageDelta {
    /// Decode one `assistantMessageEvent`. As with [`SessionEvent::decode`], this
    /// cannot fail: the delta stream must survive an upstream addition.
    pub fn decode(raw: &Value) -> Self {
        match Self::deserialize(raw) {
            Ok(delta) => delta,
            Err(error) => {
                let (kind, malformed) = classify_failure(raw, &KNOWN_DELTA_KINDS, &error);
                match malformed {
                    Some(reason) => Self::Malformed {
                        kind,
                        raw: raw.clone(),
                        reason,
                    },
                    None => Self::Unknown {
                        kind,
                        raw: raw.clone(),
                    },
                }
            }
        }
    }

    /// The wire `type` of this delta.
    pub fn kind(&self) -> &str {
        match self {
            Self::Start => "start",
            Self::TextStart { .. } => "text_start",
            Self::TextDelta { .. } => "text_delta",
            Self::TextEnd { .. } => "text_end",
            Self::ThinkingStart { .. } => "thinking_start",
            Self::ThinkingDelta { .. } => "thinking_delta",
            Self::ThinkingEnd { .. } => "thinking_end",
            Self::ImageEnd { .. } => "image_end",
            Self::ToolCallStart { .. } => "toolcall_start",
            Self::ToolCallDelta { .. } => "toolcall_delta",
            Self::ToolCallEnd { .. } => "toolcall_end",
            Self::Done { .. } => "done",
            Self::Error { .. } => "error",
            Self::Unknown { kind, .. } | Self::Malformed { kind, .. } => kind,
        }
    }
}

/// Deserialize a nested delta through [`MessageDelta::decode`], which is
/// infallible — a `message_update` is only malformed at the outer level.
fn de_message_delta<'de, D>(deserializer: D) -> Result<MessageDelta, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(MessageDelta::decode(&Value::deserialize(deserializer)?))
}
