#![allow(unused_imports)]
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use omp_session::{restore, AgentRoster, Message, SessionControl, Subagent, TodoPhase, Transcript};
use omp_transport::palette::{self, AdvertisedCommand};
use omp_transport::protocol::ui::{self, UiResponse};
use omp_transport::protocol::{self, commands, ImageContent};
use omp_transport::{ClientOptions, OmpClient, SessionEvent, SidecarSpec};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use tokio::sync::broadcast::error::RecvError;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;

use crate::dialogs::Dialogs;
use crate::dto::{
    ActivitySnapshot, AgentProgressSnapshot, AgentRetrySnapshot, AgentSnapshot, AttachmentSnapshot,
    ChromeEvent, CommandSnapshot, ContextSnapshot, ControlSnapshot, CounterSnapshot,
    JobDeliverySnapshot, ModelSnapshot, NotificationEvent, ReadySnapshot, RowPatch, RowSnapshot,
    SessionStatus, SubcommandSnapshot, ThreadEvent, ThreadSnapshot, ToolSnapshot,
    UiRequestSnapshot, ACTIVITY_EVENT, AGENTS_EVENT, CHROME_EVENT, COMMANDS_EVENT,
    NOTIFICATIONS_EVENT, ROWS_EVENT, THREADS_EVENT, UI_REQUESTS_EVENT,
};
use crate::notify::{Facts, Notifier};

use super::*;

fn jobs(message: &Message) -> Vec<JobDeliverySnapshot> {
    message
        .jobs
        .iter()
        .map(|job| JobDeliverySnapshot {
            job_id: job.job_id.clone(),
            kind: job.kind.clone(),
            duration_ms: job.duration_ms,
            label: job.label.clone(),
        })
        .collect()
}

/// The frontend's view of one conversation row.
///
/// Public because a second reader needs it: a subagent's transcript is a session of the same
/// kind, and `bridge::agent_messages` renders its rows through the same mapping so an agent's
/// conversation is not a lesser view of the thread that spawned it.
pub fn row_snapshot(row: &omp_session::Row) -> RowSnapshot {
    match row {
        omp_session::Row::Message(message)
            if matches!(message.role(), "toolResult" | "tool_result" | "tool") =>
        {
            let (tool_call_id, tool_name, is_error) = match &message.kind {
                omp_session::MessageKind::ToolResult {
                    tool_call_id,
                    tool_name,
                    is_error,
                } => (tool_call_id.clone(), tool_name.clone(), *is_error),
                _ => (String::new(), String::new(), false),
            };
            RowSnapshot {
                role: "tool".to_string(),
                timestamp: message.timestamp,
                text: String::new(),
                thinking: None,
                streaming: false,
                tool: Some(ToolSnapshot {
                    tool_call_id,
                    tool_name,
                    intent: None,
                    args: String::new(),
                    details: message
                        .raw
                        .get("details")
                        .map(|v| v.to_string())
                        .unwrap_or_default(),
                    output: message.text(),
                    is_error,
                    finished: true,
                }),
                attachments: attachments(message),
                custom_type: message.custom_type.clone(),
                jobs: jobs(message),
                command: None,
                steering: None,
            }
        }
        omp_session::Row::Message(message) => {
            let steering = match &message.kind {
                omp_session::MessageKind::User { steering, .. } if *steering => Some(true),
                _ => None,
            };
            RowSnapshot {
                role: message.role().to_string(),
                timestamp: message.timestamp,
                text: message.text(),
                thinking: None,
                streaming: false,
                tool: None,
                attachments: attachments(message),
                custom_type: message.custom_type.clone(),
                jobs: jobs(message),
                command: None,
                steering,
            }
        }
        omp_session::Row::Assistant { message, streaming } => {
            let mut text = message.text();
            if text.is_empty() {
                if let Some(err) = message.error_message() {
                    text = err.to_string();
                }
            }
            RowSnapshot {
                role: "assistant".to_string(),
                timestamp: message.timestamp,
                text,
                thinking: optional(message.thinking()),
                streaming: *streaming,
                tool: None,
                attachments: attachments(message),
                custom_type: message.custom_type.clone(),
                jobs: jobs(message),
                command: None,
                steering: None,
            }
        }
        omp_session::Row::Tool(card) => RowSnapshot {
            role: "tool".to_string(),
            timestamp: None,
            text: String::new(),
            thinking: None,
            streaming: !card.is_finished(),
            tool: Some(ToolSnapshot {
                tool_call_id: card.tool_call_id.clone(),
                tool_name: card.tool_name.clone(),
                intent: card.intent.clone(),
                args: arguments(&card.args),
                details: {
                    let details = match &card.outcome {
                        omp_session::ToolOutcome::Running => None,
                        omp_session::ToolOutcome::Done(result) => result.details.as_ref(),
                    };
                    details.map_or_else(String::new, ToString::to_string)
                },
                output: match &card.outcome {
                    omp_session::ToolOutcome::Running => String::new(),
                    omp_session::ToolOutcome::Done(result) => result.text(),
                },
                is_error: matches!(
                    &card.outcome,
                    omp_session::ToolOutcome::Done(result) if result.is_error
                ),
                finished: card.is_finished(),
            }),
            attachments: Vec::new(),
            custom_type: None,
            jobs: Vec::new(),
            command: None,
            steering: None,
        },
        omp_session::Row::Notice {
            level,
            text,
            kind,
            source,
            ..
        } => RowSnapshot {
            role: format!("notice:{level}"),
            timestamp: None,
            text: text.clone(),
            thinking: None,
            streaming: false,
            tool: None,
            attachments: Vec::new(),
            custom_type: Some(kind.clone()),
            jobs: Vec::new(),
            command: source.clone(),
            steering: None,
        },
    }
}

/// The failure a transcript most recently recorded, if any.
///
/// Two shapes mean "a turn failed", and only one of them is a notice: a failed assistant
/// turn is an assistant message the engine marked (`stopReason: "error"` with the reason
/// on the message — measured, `429 rate limited`), which the transcript keeps as an
/// ordinary row. The notice at error level is the other path: a retry that gave up, or an
/// event this build could not read.
pub(super) fn last_failure(transcript: &Transcript) -> Option<String> {
    for row in transcript.rows().iter().rev() {
        match row {
            omp_session::Row::Assistant { message, .. } => match &message.kind {
                omp_session::MessageKind::Assistant {
                    stop_reason,
                    error_message,
                    ..
                } if stop_reason.as_deref() == Some("error") => {
                    return Some(
                        error_message
                            .clone()
                            .unwrap_or_else(|| "the turn failed".to_string()),
                    );
                }
                _ => return None,
            },
            omp_session::Row::Notice { level, text, .. } if level == "error" => {
                return Some(text.clone());
            }
            omp_session::Row::Message(_) => return None,
            _ => {}
        }
    }
    None
}

/// The images a message carries, in order (`docs/12` §3.1).
fn attachments(message: &omp_session::Message) -> Vec<AttachmentSnapshot> {
    message
        .images()
        .map(|(mime_type, data)| AttachmentSnapshot {
            mime_type: mime_type.to_string(),
            data: data.to_string(),
        })
        .collect()
}

/// A block of text that is present, or absent because it is blank.
pub(super) fn optional(text: String) -> Option<String> {
    (!text.trim().is_empty()).then_some(text)
}

/// The engine's arguments as JSON text, or empty when the call carried none.
fn arguments(args: &serde_json::Value) -> String {
    if args.is_null() {
        String::new()
    } else {
        args.to_string()
    }
}

/// The thread's event tail, published without a tick of its own.
///
/// The tail is diagnostics: a streamed delta repeats the kind of the event before it, and
/// the window redraws the tail at its own pace. So a kind the tail has not shown yet is
/// published at once — the pane must still see a turn start and end even when the next
/// event lands in the same tick — while a repeat waits for the rows tick, which is one IPC
/// event per hundred milliseconds instead of one per token.
#[derive(Debug, Default)]
pub(super) struct ActivityTail {
    /// The newest repeat, waiting for the tick.
    latest: Option<ActivitySnapshot>,
    /// The kind the sink was last given.
    shown: String,
}

impl ActivityTail {
    /// Fold one event in, answering the snapshot to publish now when its kind is new to
    /// the tail.
    pub(super) fn record(&mut self, kind: &str, sequence: u64) -> Option<ActivitySnapshot> {
        if self.shown != kind {
            self.shown.clear();
            self.shown.push_str(kind);
            self.latest = None;

            return Some(ActivitySnapshot {
                kind: kind.to_string(),
                sequence,
            });
        }

        // A repeat is buffered in place, so the deltas after the first allocate nothing.
        let latest = self.latest.get_or_insert_with(|| ActivitySnapshot {
            kind: kind.to_string(),
            sequence,
        });
        latest.sequence = sequence;

        None
    }

    /// The newest repeat since the last call, if any.
    pub(super) fn take(&mut self) -> Option<ActivitySnapshot> {
        self.latest.take()
    }
}

/// Tell the UI to re-render the conversation from the earliest changed row.
///
/// Called on a tick rather than per event: see [`ROWS_PUBLISH_INTERVAL`].
pub(super) fn publish_rows(
    reporting: &Reporting,
    transcript: &Mutex<Transcript>,
    first_dirty: &mut Option<usize>,
) -> bool {
    // One lock for the marker and the rows it names: commands such as manual compaction
    // write maintenance notices without a session event, so the marker is collected here,
    // and the patch is built from the same transcript state the marker covered. The guard
    // is scoped to the block: the emit below is a Tauri event, and a window's event loop
    // must never be inside the pump's critical section.
    let patch = {
        let Ok(mut transcript) = transcript.lock() else {
            eprintln!("[omp-desktop] the transcript lock was poisoned");
            return false;
        };
        if let Some(index) = transcript.take_dirty() {
            *first_dirty = Some(first_dirty.map_or(index, |earliest| earliest.min(index)));
        }
        let Some(from) = first_dirty.take() else {
            return false;
        };

        let rows = transcript.rows();
        // Clamped rather than assumed: a marker left over from a longer transcript
        // would otherwise slice out of bounds, and a panic here takes the window down.
        let from = from.min(rows.len());

        RowPatch {
            from,
            rows: rows[from..].iter().map(row_snapshot).collect(),
        }
    };

    let thread = reporting.thread.current();
    reporting.sink.rows(&thread, patch);
    true
}

/// Republish the sidebar's roster when this thread's own state may have moved.
///
/// Gated on a change rather than sent every tick, for the same reason rows are
/// coalesced: a streaming turn would otherwise broadcast the whole roster ten times a
/// second. Two things about a thread are invisible in the rows — a turn starting and a
/// turn ending, which move `is_streaming` — so they are compared directly. Everything
/// else arrives with rows (a failure becomes a row) or through [`Dialogs`], which
/// republishes the roster itself the moment the pending set changes.
/// The frontend's view of one subagent.
///
/// Flattened on purpose: the reducer nests a run's progress and retries because that is
/// how the frames arrive, while the panel renders one row of fields.
pub fn agent_snapshot(agent: &Subagent) -> AgentSnapshot {
    AgentSnapshot {
        id: agent.id.clone(),
        index: agent.index,
        agent: agent.agent.clone(),
        agent_source: agent.agent_source.clone(),
        status: agent.status.map(|status| status.as_str().to_string()),
        description: agent.description.clone(),
        task: agent.task.clone(),
        assignment: agent.assignment.clone(),
        session_file: agent.session_file.clone(),
        parent_tool_call_id: agent.parent_tool_call_id.clone(),
        detached: agent.detached,
        created_at_ms: agent.created_at_ms,
        last_update_ms: agent.last_update_ms,
        listed: agent.listed,
        progress: agent
            .progress
            .as_ref()
            .map(|progress| AgentProgressSnapshot {
                last_intent: progress.last_intent.clone(),
                current_tool: progress.current_tool.clone(),
                current_tool_args: progress.current_tool_args.clone(),
                tool_count: progress.tool_count,
                requests: progress.requests,
                tokens: progress.tokens,
                context_tokens: progress.context_tokens,
                context_window: progress.context_window,
                cost: progress.cost,
                duration_ms: progress.duration_ms,
                resolved_model: progress.resolved_model.clone(),
                resolved_thinking_level: progress.resolved_thinking_level.clone(),
                advisor: progress.advisor,
                retry: progress.retry.as_ref().map(|retry| AgentRetrySnapshot {
                    attempt: retry.attempt,
                    max_attempts: retry.max_attempts,
                    delay_ms: retry.delay_ms,
                    error_message: retry.error_message.clone(),
                    started_at_ms: retry.started_at_ms,
                }),
                retry_failure: progress
                    .retry_failure
                    .as_ref()
                    .map(|failure| AgentRetrySnapshot {
                        attempt: failure.attempt,
                        max_attempts: 0,
                        delay_ms: 0,
                        error_message: failure.error_message.clone(),
                        started_at_ms: 0,
                    }),
            }),
    }
}

/// Unix milliseconds, from the one clock the app reads wall time from.
///
/// The roster's rows are timed by *this* host rather than by the engine's `lastUpdate`
/// (`omp_session::agents`), so a panel that says "4s ago" is comparing one clock to
/// itself.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or_default()
}

pub(super) fn publish_roster(
    reporting: &Reporting,
    control: &Mutex<SessionControl>,
    last_streaming: &mut bool,
    rows_changed: bool,
) {
    let streaming = control
        .lock()
        .map(|control| control.is_streaming)
        .unwrap_or(*last_streaming);

    if !rows_changed && streaming == *last_streaming {
        return;
    }

    *last_streaming = streaming;
    reporting.sink.threads(reporting.roster.snapshots());
}
