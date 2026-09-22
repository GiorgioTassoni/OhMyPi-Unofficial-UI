//! The conversation, reduced from the event stream or rebuilt from history.
//!
//! Two paths produce the same rows, which is the point: a thread opened fresh
//! streams events, and a thread resumed starts from `get_messages_page`. If they
//! disagreed, resuming would show a different transcript than watching it live.
//!
//! ```text
//!   events (live)  ─┐
//!                   ├─►  Transcript  ─►  rows
//!   messages (restore) ─┘
//! ```
//!
//! # Authority
//!
//! Streaming is for liveness, never for truth. Deltas accumulate so text can
//! animate, but `message_end` replaces the row with the engine's own finished
//! message — so a dropped or mis-ordered delta cannot leave a wrong transcript
//! behind. The same reasoning makes the restore path trustworthy: it renders the
//! engine's stored messages, not a reconstruction of them.
//!
//! # Identity
//!
//! A tool card is keyed by `toolCallId` and filled from whichever source arrives
//! first: the live `tool_execution_*` triple, or the `toolResult` message that
//! survives in history. Both carry the engine's `content`/`details`, so a card
//! renders the same either way.

use std::collections::{BTreeMap, HashMap};

use omp_transport::events::{MessageUpdate, SessionEvent};
use omp_transport::MessageDelta;
use serde_json::Value;

use crate::messages::{ContentBlock, Message, MessageKind, ToolResultContent};

/// How a tool call ended.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolOutcome {
    /// No end event has arrived yet.
    Running,
    /// The tool finished, successfully or not.
    Done(ToolResultContent),
}

/// One tool call, as a card.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCard {
    /// Correlates the start, updates, and end of one call.
    pub tool_call_id: String,
    pub tool_name: String,
    /// The arguments the model passed, passed through opaquely.
    pub args: Value,
    /// The model's stated intent, when it gave one.
    pub intent: Option<String>,
    /// Live output from `tool_stream_update`, in arrival order.
    ///
    /// These are deltas — a per-tool renderer reduces them (a terminal-style
    /// transcript for `bash`, an incremental diff for `write`). Only the tools
    /// that stream emit them; the rest stay empty and the card fills in at
    /// `tool_execution_end`.
    pub streamed: Vec<Value>,
    /// The engine's latest coalesced partial result.
    ///
    /// A snapshot rather than a delta, so only the newest matters.
    pub partial: Option<Value>,
    pub outcome: ToolOutcome,
}

impl ToolCard {
    fn new(tool_call_id: &str, tool_name: &str) -> Self {
        Self {
            tool_call_id: tool_call_id.to_string(),
            tool_name: tool_name.to_string(),
            args: Value::Null,
            intent: None,
            streamed: Vec::new(),
            partial: None,
            outcome: ToolOutcome::Running,
        }
    }

    /// Whether the call has finished.
    pub fn is_finished(&self) -> bool {
        matches!(self.outcome, ToolOutcome::Done(_))
    }
}

/// One row of the conversation view (`docs/12-v1-ia-and-screens.md` §3).
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    /// A message that is not an assistant turn: user, developer, or a role this
    /// build does not model. The role lives on the message.
    Message(Message),
    /// An assistant turn. `streaming` is true between `message_start` and
    /// `message_end`, which is what drives the in-place animation.
    Assistant { message: Message, streaming: bool },
    /// One tool call.
    Tool(ToolCard),
    /// An inline chip for maintenance activity: compaction, retries, mode
    /// changes, reminders. `kind` is the wire event kind that produced it;
    /// `source` is the engine's own attribution when it supplied one.
    Notice {
        /// Open string union, as sent: `info`, `warning`, `error`.
        level: String,
        text: String,
        kind: String,
        source: Option<String>,
    },
}

/// The conversation.
#[derive(Debug, Default)]
pub struct Transcript {
    rows: Vec<Row>,
    /// Rows awaiting their `message_end`, keyed by the message's timestamp — the
    /// one identifier both the start and the end payload carry.
    pending: HashMap<u64, usize>,
    /// The row from the most recent `message_start`, used when an `message_end`
    /// arrives without a timestamp to match on.
    last_started: Option<usize>,
    /// The assistant row currently receiving deltas.
    streaming: Option<usize>,
    /// The earliest row whose content changed since the last [`Self::take_dirty`].
    ///
    /// The conversation is append-only apart from its tail being rewritten while it
    /// streams and a tool card mutating where it sits, so "everything from here on"
    /// is the exact and minimal thing a view has to re-render. Tracked here rather
    /// than diffed by the caller, because a diff would have to clone every row to
    /// compare it — and the rows can be megabytes of transcript.
    dirty: Option<usize>,
    /// Blocks for the in-flight assistant message, keyed by `contentIndex`.
    ///
    /// Indexed rather than appended because the engine interleaves block kinds
    /// (thinking, then text, then tool calls) within one message.
    blocks: BTreeMap<u32, ContentBlock>,
}

impl Transcript {
    pub fn new() -> Self {
        Self::default()
    }

    /// The rows, in conversation order.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Forget everything. Used when switching threads.
    pub fn clear(&mut self) {
        self.rows.clear();
        // From index 0 even when there was nothing to clear: an empty list is a
        // change the view has to see, or it would keep rendering the old thread.
        self.dirty = Some(0);
        self.pending.clear();
        self.last_started = None;
        self.streaming = None;
        self.blocks.clear();
    }

    /// The earliest changed row since this was last called, if any.
    ///
    /// A view sends everything from this index onward, and nothing else.
    pub fn take_dirty(&mut self) -> Option<usize> {
        self.dirty.take()
    }

    /// Append a row, recording it as changed.
    fn push_row(&mut self, row: Row) -> usize {
        let index = self.rows.len();
        self.rows.push(row);
        self.mark_dirty(index);
        index
    }

    /// The row at `index`, recording it as changed.
    ///
    /// Every mutation of a row goes through here or [`Self::push_row`], so the
    /// change marker cannot be forgotten at a call site.
    fn row_mut(&mut self, index: usize) -> &mut Row {
        self.mark_dirty(index);
        &mut self.rows[index]
    }

    fn mark_dirty(&mut self, index: usize) {
        self.dirty = Some(self.dirty.map_or(index, |current| current.min(index)));
    }

    /// Rebuild from a restored history, replacing whatever is there.
    ///
    /// This is the resume path: `crates/omp-session/src/restore.rs` fetches the
    /// messages and this renders them. Assistant tool calls become cards before
    /// the results are seen, so each `toolResult` message attaches to the card
    /// named by its `toolCallId` rather than starting a second one.
    pub fn restore(&mut self, messages: &[Message]) {
        self.clear();

        for message in messages {
            match &message.kind {
                MessageKind::Assistant { .. } => {
                    self.push_row(Row::Assistant {
                        message: message.clone(),
                        streaming: false,
                    });
                    // Cards come after the message that requested them, matching
                    // the reading order of the live path. The calls are named by
                    // the assistant message; the results arrive later.
                    for (id, name, arguments) in message.tool_calls() {
                        let card = self.card(id, name);
                        card.args = arguments.clone();
                    }
                }
                MessageKind::ToolResult {
                    tool_call_id,
                    tool_name,
                    is_error,
                } => {
                    let content = message.raw.get("content").cloned().unwrap_or(Value::Null);
                    let details = message.raw.get("details").cloned();
                    let card = self.card(tool_call_id, tool_name);
                    card.outcome = ToolOutcome::Done(ToolResultContent {
                        content,
                        details,
                        is_error: *is_error,
                    });
                }
                _ => {
                    self.push_row(Row::Message(message.clone()));
                }
            }
        }
    }

    /// Reduce one streamed event into the conversation.
    ///
    /// Unknown and malformed events become a visible chip rather than being
    /// dropped: a transcript that quietly omits what the engine said is worse
    /// than one that admits it could not read something.
    pub fn apply(&mut self, event: &SessionEvent) {
        match event {
            SessionEvent::MessageStart(payload) => self.begin_message(&payload.message),
            SessionEvent::MessageUpdate(update) => self.apply_delta(update),
            SessionEvent::MessageEnd(payload) => self.end_message(&payload.message),

            SessionEvent::ToolExecutionStart(start) => {
                let card = self.card(&start.tool_call_id, &start.tool_name);
                card.args = start.args.clone();
                card.intent = start.intent.clone();
            }
            SessionEvent::ToolExecutionUpdate(update) => {
                let partial = update.partial_result.clone();
                self.card(&update.tool_call_id, &update.tool_name).partial = Some(partial);
            }
            SessionEvent::ToolStreamUpdate(update) => {
                let chunk = update.update.clone();
                self.card(&update.tool_call_id, &update.tool_name)
                    .streamed
                    .push(chunk);
            }
            SessionEvent::ToolExecutionEnd(end) => {
                let is_error = end.is_error;
                let content = end
                    .result
                    .get("content")
                    .cloned()
                    .unwrap_or_else(|| end.result.clone());
                let details = end.result.get("details").cloned();
                let card = self.card(&end.tool_call_id, &end.tool_name);
                card.outcome = ToolOutcome::Done(ToolResultContent {
                    content,
                    details,
                    is_error,
                });
            }

            SessionEvent::Notice(notice) => self.notice(
                &notice.level,
                notice.message.clone(),
                "notice",
                notice.source.clone(),
            ),
            SessionEvent::AutoCompactionStart(start) => {
                let text = format!("Compacting context ({})", start.action);
                self.notice("info", text, "auto_compaction_start", None);
            }
            SessionEvent::AutoCompactionEnd(end) => {
                let (level, text) = if end.aborted {
                    ("warning", "Compaction aborted".to_string())
                } else if end.skipped {
                    ("info", "Compaction skipped".to_string())
                } else if end.will_retry {
                    ("info", "Context compacted; retrying".to_string())
                } else {
                    ("info", "Context compacted".to_string())
                };
                self.notice(level, text, "auto_compaction_end", None);
            }
            SessionEvent::AutoRetryStart(start) => {
                let text = format!(
                    "Retrying after an error (attempt {}/{})",
                    start.attempt, start.max_attempts
                );
                self.notice("warning", text, "auto_retry_start", None);
            }
            SessionEvent::AutoRetryEnd(end) => {
                let (level, text) = if end.success {
                    ("info", "Retry succeeded".to_string())
                } else {
                    let text = end
                        .final_error
                        .clone()
                        .unwrap_or_else(|| "Retry failed".to_string());
                    ("error", text)
                };
                self.notice(level, text, "auto_retry_end", None);
            }
            SessionEvent::RetryFallbackApplied(applied) => {
                let text = format!("Falling back from {} to {}", applied.from, applied.to);
                self.notice("warning", text, "retry_fallback_applied", None);
            }
            SessionEvent::RetryFallbackSucceeded(succeeded) => {
                let text = format!("Fallback to {} answered", succeeded.model);
                self.notice("info", text, "retry_fallback_succeeded", None);
            }
            SessionEvent::ModelChanged => {
                self.notice("info", "Model changed".to_string(), "model_changed", None);
            }
            SessionEvent::ThinkingLevelChanged(change) => {
                let level = change.thinking_level.as_deref().unwrap_or("none");
                let text = format!("Thinking level: {level}");
                self.notice("info", text, "thinking_level_changed", None);
            }
            SessionEvent::TtsrTriggered(triggered) => {
                let text = format!("{} rule(s) fired", triggered.rules.len());
                self.notice("info", text, "ttsr_triggered", None);
            }
            SessionEvent::TodoReminder(reminder) => {
                let text = format!(
                    "Todo reminder {}/{}",
                    reminder.attempt, reminder.max_attempts
                );
                self.notice("info", text, "todo_reminder", None);
            }
            SessionEvent::GoalUpdated(_) => {
                self.notice("info", "Goal updated".to_string(), "goal_updated", None);
            }

            SessionEvent::Unknown { kind, .. } => {
                let text = format!("Unrecognised engine event: {kind}");
                self.notice("warning", text, "unknown_event", None);
            }
            SessionEvent::Malformed { kind, reason, .. } => {
                let text = format!("Could not read `{kind}`: {reason}");
                self.notice("error", text, "malformed_event", None);
            }

            // Lifecycle and side channels that are not transcript rows.
            SessionEvent::AgentStart
            | SessionEvent::AgentEnd(_)
            | SessionEvent::TurnStart
            | SessionEvent::TurnEnd(_)
            | SessionEvent::ConfigWarningsChanged
            | SessionEvent::AdvisorCostChanged
            | SessionEvent::AdvisorYielded
            | SessionEvent::TodoAutoClear
            | SessionEvent::IrcMessage(_) => {}
        }
    }

    /// A builtin slash command's output, as a notice row.
    ///
    /// `command_output` is a *frame*, not a session event (`docs/rpc.md` §11), so it
    /// reaches the transcript through here rather than through [`Self::apply`] — the same
    /// way an `extension_ui_request` reaches the dialog store. `docs/12` §7.3: a
    /// local-only command produces output without a turn, and the transcript is where the
    /// user reads it.
    pub fn apply_command_output(&mut self, text: &str) {
        self.notice("info", text.to_string(), "command_output", None);
    }

    /// A failure the host itself saw, as an error notice row.
    ///
    /// Two callers need it, and both are facts the *engine* cannot report: a sidecar that
    /// died mid-turn (the stream simply ends, so nothing else would ever say so — the row
    /// would keep its streaming dot and the sidebar would keep presenting a dead thread as
    /// live), and a blocking dialog whose frame the host could not read (the engine waits on
    /// it, so the only visible thing would be the tool failing later for no stated reason).
    ///
    /// An error notice is where a failure is visible: `session::last_failure` reads the rows
    /// to derive the thread's `error` and the sidebar's red dot (`docs/12` §2.2), so this is
    /// the same place — and the same shape — a failed turn lands in.
    pub fn apply_error(&mut self, text: &str, source: &str) {
        self.notice("error", text.to_string(), source, None);
    }

    fn notice(&mut self, level: &str, text: String, kind: &str, source: Option<String>) {
        self.push_row(Row::Notice {
            level: level.to_string(),
            text,
            kind: kind.to_string(),
            source,
        });
    }

    /// Borrow the card for a tool call, creating it on first mention.
    fn card(&mut self, tool_call_id: &str, tool_name: &str) -> &mut ToolCard {
        let index = match self
            .rows
            .iter()
            .position(|row| matches!(row, Row::Tool(card) if card.tool_call_id == tool_call_id))
        {
            Some(index) => index,
            None => self.push_row(Row::Tool(ToolCard::new(tool_call_id, tool_name))),
        };

        match self.row_mut(index) {
            Row::Tool(card) => card,
            // The index came from a `Row::Tool` match and nothing was removed
            // in between, so this is unreachable by construction.
            _ => unreachable!("tool card index points at a tool row"),
        }
    }

    fn begin_message(&mut self, raw: &Value) {
        let message = Message::decode(raw);

        match &message.kind {
            MessageKind::Assistant { .. } => {
                self.blocks.clear();
                for (index, block) in message.blocks.iter().enumerate() {
                    self.blocks.insert(index as u32, block.clone());
                }
                let index = self.push_row(Row::Assistant {
                    message,
                    streaming: true,
                });
                if let Some(stamp) = self.rows[index].message_stamp() {
                    self.pending.insert(stamp, index);
                }
                self.last_started = Some(index);
                // Independent of the timestamp: deltas must land on this row
                // even if the engine stamped nothing.
                self.streaming = Some(index);
            }
            MessageKind::ToolResult {
                tool_call_id,
                tool_name,
                is_error,
            } => {
                let content = raw.get("content").cloned().unwrap_or(Value::Null);
                let details = raw.get("details").cloned();
                let is_error = *is_error;
                let card = self.card(tool_call_id, tool_name);
                card.outcome = ToolOutcome::Done(ToolResultContent {
                    content,
                    details,
                    is_error,
                });
            }
            _ => {
                let stamp = message.timestamp;
                let index = self.push_row(Row::Message(message));
                if let Some(stamp) = stamp {
                    self.pending.insert(stamp, index);
                }
                self.last_started = Some(index);
            }
        }
    }

    fn end_message(&mut self, raw: &Value) {
        let message = Message::decode(raw);
        // Consumed for every role, so a stale row can never pair with a later
        // end that has no timestamp of its own.
        let settled = self.settle(message.timestamp);

        match &message.kind {
            MessageKind::Assistant { .. } => {
                // Decide the target before moving: exactly one of the two paths
                // below consumes the message.
                let target = match settled {
                    Some(index) if matches!(self.rows.get(index), Some(Row::Assistant { .. })) => {
                        Some(index)
                    }
                    _ => None,
                };

                match target {
                    // The engine's finished message wins over the deltas.
                    Some(index) => {
                        // No `Some(..)`: `row_mut` cannot fail, because rows are
                        // never removed — the type guard is the only condition.
                        if let Row::Assistant {
                            message: row,
                            streaming,
                        } = self.row_mut(index)
                        {
                            *row = message;
                            *streaming = false;
                        }
                    }
                    None => {
                        self.push_row(Row::Assistant {
                            message,
                            streaming: false,
                        });
                    }
                }

                self.blocks.clear();
                self.streaming = None;
            }
            MessageKind::ToolResult {
                tool_call_id,
                tool_name,
                is_error,
            } => {
                let content = raw.get("content").cloned().unwrap_or(Value::Null);
                let details = raw.get("details").cloned();
                let is_error = *is_error;
                let card = self.card(tool_call_id, tool_name);
                card.outcome = ToolOutcome::Done(ToolResultContent {
                    content,
                    details,
                    is_error,
                });
            }
            _ => {
                let target = match settled {
                    Some(index) if matches!(self.rows.get(index), Some(Row::Message(_))) => {
                        Some(index)
                    }
                    _ => None,
                };

                match target {
                    Some(index) => {
                        if let Row::Message(row) = self.row_mut(index) {
                            *row = message;
                        }
                    }
                    None => {
                        self.push_row(Row::Message(message));
                    }
                }
            }
        }
    }

    /// Pair a `message_end` with the row its `message_start` created.
    fn settle(&mut self, stamp: Option<u64>) -> Option<usize> {
        let matched = stamp.and_then(|stamp| self.pending.remove(&stamp));
        let index = matched.or(self.last_started);
        if self.last_started == index {
            self.last_started = None;
        }
        index
    }

    fn apply_delta(&mut self, update: &MessageUpdate) {
        let index = match self.streaming {
            Some(index) => index,
            None => {
                // Subscribed mid-turn: no `message_start` was seen, so adopt the
                // update's own message rather than dropping the content.
                self.begin_message(&update.message);
                match self.streaming {
                    Some(index) => index,
                    // The adopted message had no usable identity, so there is no
                    // row to attach this delta to.
                    None => return,
                }
            }
        };

        apply_delta_blocks(&mut self.blocks, &update.assistant_message_event);

        // Collected before the row borrow: `row_mut` borrows all of `self`, so the
        // block map cannot be read through it.
        let blocks: Vec<ContentBlock> = self.blocks.values().cloned().collect();
        if let Row::Assistant { message, .. } = self.row_mut(index) {
            message.blocks = blocks;
        }
    }
}

/// Reduce one token delta into the message's block map.
fn apply_delta_blocks(blocks: &mut BTreeMap<u32, ContentBlock>, delta: &MessageDelta) {
    match delta {
        MessageDelta::TextStart { content_index } => {
            blocks.entry(*content_index).or_insert(ContentBlock::Text {
                text: String::new(),
            });
        }
        MessageDelta::TextDelta {
            content_index,
            delta,
        } => {
            match blocks.entry(*content_index).or_insert(ContentBlock::Text {
                text: String::new(),
            }) {
                ContentBlock::Text { text } => text.push_str(delta),
                // The engine changed the block kind mid-stream. Trust the later
                // `text_end`, which carries the authoritative content.
                slot => {
                    *slot = ContentBlock::Text {
                        text: delta.clone(),
                    }
                }
            }
        }
        MessageDelta::TextEnd {
            content_index,
            content,
        } => {
            blocks.insert(
                *content_index,
                ContentBlock::Text {
                    text: content.clone(),
                },
            );
        }
        MessageDelta::ThinkingStart { content_index } => {
            blocks
                .entry(*content_index)
                .or_insert(ContentBlock::Thinking {
                    thinking: String::new(),
                });
        }
        MessageDelta::ThinkingDelta {
            content_index,
            delta,
        } => {
            match blocks
                .entry(*content_index)
                .or_insert(ContentBlock::Thinking {
                    thinking: String::new(),
                }) {
                ContentBlock::Thinking { thinking } => thinking.push_str(delta),
                slot => {
                    *slot = ContentBlock::Thinking {
                        thinking: delta.clone(),
                    }
                }
            }
        }
        MessageDelta::ThinkingEnd {
            content_index,
            content,
        } => {
            blocks.insert(
                *content_index,
                ContentBlock::Thinking {
                    thinking: content.clone(),
                },
            );
        }
        MessageDelta::ImageEnd {
            content_index,
            content,
        } => {
            // The delta's `content` is the finished block (`ImageContent`), so it
            // goes through the same decode as a block read out of a message —
            // one rule for what an image block is, on both paths.
            blocks.insert(*content_index, ContentBlock::decode(content));
        }
        MessageDelta::ToolCallStart { content_index } => {
            blocks
                .entry(*content_index)
                .or_insert(ContentBlock::ToolCall {
                    id: String::new(),
                    name: String::new(),
                    arguments: Value::Null,
                    intent: None,
                });
        }
        // The streamed `delta` is a fragment of the call's JSON, not the call.
        // The authoritative call arrives at `toolcall_end`, so accumulating
        // fragments here would only risk showing a half-parsed argument object.
        MessageDelta::ToolCallDelta { .. } => {}
        MessageDelta::ToolCallEnd {
            content_index,
            tool_call,
        } => {
            blocks.insert(*content_index, ContentBlock::decode(tool_call));
        }

        // No block to place: the message-level lifecycle, and the fallbacks.
        MessageDelta::Start
        | MessageDelta::Done { .. }
        | MessageDelta::Error { .. }
        | MessageDelta::Unknown { .. }
        | MessageDelta::Malformed { .. } => {}
    }
}

impl Row {
    /// The timestamp of the message this row carries, when it carries one.
    fn message_stamp(&self) -> Option<u64> {
        match self {
            Row::Message(message) | Row::Assistant { message, .. } => message.timestamp,
            _ => None,
        }
    }
}
