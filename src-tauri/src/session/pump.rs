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
use super::grants::*;
use super::snapshot::*;

pub(super) async fn refresh_control(
    client: &OmpClient,
    control: &Mutex<SessionControl>,
) -> Result<SessionControl, String> {
    let response = client
        .request(commands::get_state(), None)
        .await
        .map_err(|error| format!("the agent did not answer get_state: {error}"))?;

    let refreshed = SessionControl::decode(&response["data"])
        .ok_or_else(|| "the agent's state payload had no session id".to_string())?;

    if let Ok(mut control) = control.lock() {
        *control = refreshed.clone();
    }

    Ok(refreshed)
}

/// Determine if a plan contains completed or abandoned tasks, and return the filtered phases.
pub(super) fn filter_completed_todos(phases: &[TodoPhase]) -> (bool, Vec<Value>) {
    let has_completed = phases.iter().any(|phase| {
        phase
            .tasks
            .iter()
            .any(|task| task.status == "completed" || task.status == "abandoned")
    });

    if !has_completed {
        return (false, Vec::new());
    }

    let remaining: Vec<Value> = phases
        .iter()
        .filter_map(|phase| {
            let remaining_tasks: Vec<Value> = phase
                .tasks
                .iter()
                .filter(|task| task.status != "completed" && task.status != "abandoned")
                .map(|task| {
                    let mut obj = json!({
                        "content": task.content,
                        "status": task.status,
                    });
                    if let Some(ref blocker) = task.blocker {
                        obj["blocker"] = json!(blocker);
                    }
                    obj
                })
                .collect();

            if remaining_tasks.is_empty() {
                None
            } else {
                Some(json!({
                    "name": phase.name,
                    "tasks": remaining_tasks,
                }))
            }
        })
        .collect();

    (true, remaining)
}

/// Auto-clear completed or abandoned tasks at the end of a turn,
/// updating the in-memory plan in the engine without requiring user action.
async fn auto_clear_completed_todos(client: &OmpClient, control: &Mutex<SessionControl>) {
    let (has_completed, remaining_phases) = {
        let Ok(ctrl) = control.lock() else { return };
        filter_completed_todos(&ctrl.todo_phases)
    };

    if has_completed {
        let response = call(
            client,
            commands::set_todos(Value::Array(remaining_phases)),
            None,
            "auto-clearing completed todos",
        )
        .await;

        match response {
            Ok(response) => {
                let raw = &response["data"]["todoPhases"];
                if raw.is_array() {
                    let stored = omp_session::decode_phases(raw);
                    if let Ok(mut ctrl) = control.lock() {
                        ctrl.todo_phases = stored;
                    }
                } else if let Ok(mut ctrl) = control.lock() {
                    ctrl.todo_phases.retain_mut(|phase| {
                        phase
                            .tasks
                            .retain(|t| t.status != "completed" && t.status != "abandoned");
                        !phase.tasks.is_empty()
                    });
                }
            }
            Err(err) => {
                eprintln!("[omp-desktop] could not auto-clear completed todos: {err}");
                if let Ok(mut ctrl) = control.lock() {
                    ctrl.todo_phases.retain_mut(|phase| {
                        phase
                            .tasks
                            .retain(|t| t.status != "completed" && t.status != "abandoned");
                        !phase.tasks.is_empty()
                    });
                }
            }
        }
    }
}

/// Subscribe to both of the client's streams and reduce them forever.
///
/// Frames that ask the host for something become pending dialogs, which announce
/// themselves through the sink ([`Dialogs`]); the rest are counted. A dialog that
/// arrives while nobody is listening is still in the store, which is what makes the
/// answer command safe regardless of subscription order.
///
/// `reporting` is what makes a published event addressable: the tag on everything
/// thread-scoped, and the roster the sidebar reads.
/// The state a session's pump feeds, in one bundle.
///
/// A bundle rather than six parameters because it is one thing: every handle here is a
/// reducer the pump writes as frames arrive, and the `LiveSession` that reads them keeps the
/// same arcs. Grouping them is also what keeps the pump's signature about its *job* — a sink,
/// a client, and the state they meet in — instead of a list of mutexes a caller has to get in
/// the right order.

pub fn spawn_pump(
    reporting: Reporting,
    client: Arc<OmpClient>,
    reducers: Reducers,
) -> JoinHandle<()> {
    let Reducers {
        transcript,
        control,
        dialogs,
        granted_tools,
        granted_commands,
        counters,
        commands,
        agents,
        title_generation_in_flight,
        last_slash_command,
    } = reducers;

    tokio::spawn(async move {
        let mut events = client.subscribe_events();
        let mut frames = client.subscribe_frames();
        // The three things only this task can notice, and the only reason a pump needs state
        // of its own: the notification rules ("once per real occurrence", which is a property
        // of a sequence rather than of an event), the dialogs actually announced, and the
        // engine's own death.
        let mut notifier = Notifier::new();
        let mut closed = client.closed();
        let mut kinds: HashSet<String> = HashSet::new();
        let mut sequence = 0u64;
        // The earliest row changed since the last publish, coalescing a turn's
        // worth of deltas into one patch per tick.
        let mut first_dirty: Option<usize> = None;
        // The event tail: a kind it has not shown goes out at once, and repeats
        // coalesce here until the tick, rather than one IPC event per delta.
        let mut activity = ActivityTail::default();
        // Whether the last roster this thread published said it was streaming, so a
        // turn starting or ending is noticed without comparing whole snapshots.
        let mut last_streaming = control
            .lock()
            .map(|control| control.is_streaming)
            .unwrap_or(false);
        let mut ticker = tokio::time::interval(ROWS_PUBLISH_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                // The engine is gone. This is the *only* arm that can fire for it: the two
                // broadcast streams end with the child, but their senders live inside the
                // `Arc<OmpClient>` this task holds, so `Closed` can never arrive on them —
                // which is how a dead sidecar used to stay on screen as a live thread with a
                // streaming dot and an approval nobody could answer (`docs/14` §7 #6).
                changed = closed.changed() => {
                    // Either a new value (the reader stopped) or a dropped sender (the client
                    // is gone): both mean there is nothing left to reduce.
                    let _ = changed;
                    break;
                }
                received = events.recv() => match received {
                    Ok(event) => {
                        // The notifier's own completion rule, read from the transport: a
                        // second spelling here would be a second answer to "has the turn
                        // finished".
                        let turn_finished = event.ends_run();
                        // Named once: the counters, the tail and the diagnostics all read
                        // it, and a delta stream should not match three times.
                        let kind = event.kind();
                        sequence += 1;
                        counters.events_seen.fetch_add(1, Ordering::Relaxed);

                        match &event {
                            SessionEvent::Unknown { .. } => {
                                counters.unknown_events.fetch_add(1, Ordering::Relaxed);
                            }
                            SessionEvent::Malformed { .. } => {
                                counters.malformed_events.fetch_add(1, Ordering::Relaxed);
                            }
                            _ => {}
                        }

                        // Asked before inserting: the set is nearly always hit, and a
                        // `String` per delta is an allocation per token.
                        if !kinds.contains(kind) {
                            kinds.insert(kind.to_string());
                            counters
                                .distinct_kinds
                                .fetch_add(1, Ordering::Relaxed);
                        }

                        // The control reducer reports when an event left fields
                        // stale; M0 has no poll loop yet, so it is only logged.
                        let stale = control
                            .lock()
                            .map(|mut control| control.apply(&event))
                            .unwrap_or(false);
                        if stale && cfg!(debug_assertions) {
                            println!("[omp-desktop] {kind} invalidated cached state");
                        }

                        if let Ok(mut transcript) = transcript.lock() {
                            transcript.apply(&event);
                            if let Some(index) = transcript.take_dirty() {
                                first_dirty = Some(first_dirty.map_or(index, |earliest| {
                                    earliest.min(index)
                                }));
                            }
                        }

                        if cfg!(debug_assertions) {
                            println!("[omp-desktop] event #{sequence} {kind}");
                        }

                        // A kind the tail has not shown goes out at once; a repeat waits
                        // for the tick, where `take` publishes the newest of them.
                        if let Some(latest) = activity.record(kind, sequence) {
                            reporting.activity(latest);
                        }

                        let is_auto_compaction_end = matches!(
                            event,
                            omp_transport::events::SessionEvent::AutoCompactionEnd(_)
                        );
                        if stale || turn_finished {
                            let client_bg = Arc::clone(&client);
                            let control_bg = Arc::clone(&control);
                            let title_bg = Arc::clone(&title_generation_in_flight);
                            let reporting_bg = reporting.clone();
                            tokio::spawn(async move {
                                if let Err(error) = refresh_control(&client_bg, &control_bg).await {
                                    eprintln!("[omp-desktop] could not refresh session state: {error}");
                                }
                                if is_auto_compaction_end {
                                    if let Ok(mut ctrl) = control_bg.lock() {
                                        ctrl.is_compacting = false;
                                    }
                                }
                                if turn_finished {
                                    auto_clear_completed_todos(&client_bg, &control_bg).await;
                                    spawn_title_generation(
                                        client_bg,
                                        control_bg,
                                        title_bg,
                                        reporting_bg,
                                    );
                                }
                            });
                        }

                        if earns_notification(&event) {
                            let facts = facts(&reporting.thread.current(), &control, &transcript);
                            if let Some(notification) = notifier.event(&event, &facts) {
                                reporting.sink.notifications(notification);
                            }
                        }
                    }
                    Err(RecvError::Lagged(missed)) => {
                        counters.lagged_events.fetch_add(1, Ordering::Relaxed);
                        eprintln!("[omp-desktop] event stream lagged by {missed}");
                    }
                    Err(RecvError::Closed) => break,
                },
                // A tick with nothing dirty publishes nothing: the marker is the
                // only thing that decides, so a paused conversation costs nothing.
                _ = ticker.tick() => {
                    // The newest repeat of the kind the tail already shows rides the
                    // tick with the rows: the window redraws the tail at its own pace.
                    if let Some(latest) = activity.take() {
                        reporting.activity(latest);
                    }
                    // Rows and the roster share the tick: a streaming turn emits deltas
                    // far faster than either a conversation view or a sidebar redraws.
                    let changed = publish_rows(&reporting, &transcript, &mut first_dirty);
                    publish_roster(&reporting, &control, &mut last_streaming, changed);
                }
                received = frames.recv() => match received {
                    Ok(frame) => {
                        counters.frames_seen.fetch_add(1, Ordering::Relaxed);

                        // The slash-command side channels (`docs/rpc.md` §11) have
                        // consumers now, and this is where they arrive: a builtin's
                        // output becomes a notice row, and a metadata change refreshes
                        // the palette's list for whichever window is looking at it.
                        match protocol::classify(&frame) {
                            protocol::FrameClass::CommandOutput => {
                                if let Some(text) = palette::command_output(&frame) {
                                    // `/rename` is an implementation detail of automatic
                                    // naming. Its receipt (including failure) must not become
                                    // an "info" message in the conversation.
                                    if is_generated_title_receipt(text)
                                        && title_generation_in_flight.swap(false, Ordering::AcqRel)
                                    {
                                        continue;
                                    }
                                    let cmd = last_slash_command.lock().ok().and_then(|mut slot| slot.take());
                                    if let Ok(mut transcript) = transcript.lock() {
                                        transcript.apply_command_output_with_command(text, cmd.as_deref());
                                        if let Some(index) = transcript.take_dirty() {
                                            first_dirty = Some(first_dirty.map_or(index, |earliest| {
                                                earliest.min(index)
                                            }));
                                        }
                                    }
                                }
                            }
                            protocol::FrameClass::Subagent => {
                                // Folded in as they arrive, and published the moment the
                                // roster actually changes: the engine deletes a settled
                                // subagent from its registry at once, so a later poll
                                // cannot bring the row back (`docs/12` §9) — these frames
                                // are the only place a finished agent is still a row.
                                let changed = agents
                                    .lock()
                                    .map(|mut roster| roster.apply_frame(&frame, now_ms()))
                                    .unwrap_or(false);
                                if changed {
                                    reporting.agents(&agents);
                                }
                            }
                            protocol::FrameClass::AvailableCommandsUpdate => {
                                let list = crate::palette::with_desktop_builtins(palette::commands_from(&frame));
                                // An update with an empty list is dropped rather than
                                // stored: it would empty a palette that works.
                                if !list.is_empty() {
                                    let snapshots = command_snapshots(&list);
                                    if let Ok(mut cached) = commands.lock() {
                                        *cached = list;
                                    }
                                    reporting
                                        .sink
                                        .commands(&reporting.thread.current(), snapshots);
                                }
                            }
                            protocol::FrameClass::SessionInfoUpdate => {
                                let client_bg = Arc::clone(&client);
                                let control_bg = Arc::clone(&control);
                                let reporting_bg = reporting.clone();
                                tokio::spawn(async move {
                                    if let Err(error) =
                                        refresh_control(&client_bg, &control_bg).await
                                    {
                                        eprintln!(
                                            "[omp-desktop] could not refresh generated title: {error}"
                                        );
                                    } else {
                                        reporting_bg.sink.threads(reporting_bg.roster.snapshots());
                                    }
                                });
                            }
                            _ => {}
                        }

                        // Every `extension_ui_request` is counted; only the
                        // blocking four become state. The classification comes
                        // from the transport, so a new fire-and-forget method
                        // upstream cannot make the app report a stall.
                        if frame.get("type").and_then(|kind| kind.as_str())
                            == Some("extension_ui_request")
                        {
                            counters.ui_requests.fetch_add(1, Ordering::Relaxed);

                            let method = frame
                                .get("method")
                                .and_then(|method| method.as_str())
                                .unwrap_or_default();
                            if ui::UiRequestKind::from_method(method).is_some() {
                                counters
                                    .blocking_ui_requests
                                    .fetch_add(1, Ordering::Relaxed);
                            }
                        }

                        let Some(incoming) = ui::decode(&frame) else {
                            // The half the host does not answer: chrome the window draws
                            // (`docs/12` §13). Emitted with its thread, because two live
                            // sessions share one window and a widget from one is not a
                            // widget for the other.
                            if let Some(op) = ui::chrome(&frame) {
                                reporting.sink.chrome(ChromeEvent {
                                    thread: reporting.thread.current(),
                                    op,
                                });
                            } else if let Some(method) = unusable_dialog(&frame) {
                                // A blocking dialog the decoder could not turn into a
                                // request: the engine is waiting on it and will refuse the
                                // tool at its own deadline, so a silent `continue` here is
                                // the user seeing a tool fail for no stated reason.
                                if let Ok(mut transcript) = transcript.lock() {
                                    transcript.apply_error(&method, "extension_ui_request");
                                    if let Some(index) = transcript.take_dirty() {
                                        first_dirty = Some(first_dirty.map_or(index, |earliest| {
                                            earliest.min(index)
                                        }));
                                    }
                                }
                            }
                            continue;
                        };

                        if let Some(frame) = auto_approval_frame(
                            &incoming,
                            &granted_tools,
                            &granted_commands,
                        ) {
                            match client.send(&frame).await {
                                Ok(()) => continue,
                                Err(error) => eprintln!(
                                    "[omp-desktop] could not answer granted approval: {error}"
                                ),
                            }
                        }

                        if let ui::Incoming::Request(request) = &incoming {
                            let facts =
                                facts(&reporting.thread.current(), &control, &transcript);
                            if let Some(notification) = notifier.dialog(request, &facts) {
                                reporting.sink.notifications(notification);
                            }
                        }

                        dialogs.apply(incoming);
                    }
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                },
            }
        }

        // The tail is diagnostics, and the last thing the engine did is still worth
        // showing: flush it rather than lose whatever arrived after the final tick.
        if let Some(latest) = activity.take() {
            reporting.activity(latest);
        }

        // The engine is gone, so nothing in the set can be answered any more and
        // an approval dialog must not stay on screen behind a dead agent.
        let reason = client
            .closed_reason()
            .unwrap_or_else(|| "the agent's stream ended".to_string());

        dialogs.clear();

        // And the thread itself is not live any more. Two things would otherwise lie about
        // it: the row's streaming dot, which no event will ever clear, and the registry,
        // which would keep offering a thread whose process is gone. The reason is published
        // where a failed turn is (`last_failure` reads the rows), so the sidebar's red dot
        // and the transcript both say it.
        if let Ok(mut control) = control.lock() {
            control.is_streaming = false;
        }
        if let Ok(mut transcript) = transcript.lock() {
            transcript.apply_error(&reason, "sidecar");
            if let Some(index) = transcript.take_dirty() {
                first_dirty = Some(first_dirty.map_or(index, |earliest| earliest.min(index)));
            }
        }
        publish_rows(&reporting, &transcript, &mut first_dirty);

        let thread = reporting.thread.current();
        let facts = facts(&thread, &control, &transcript);
        reporting
            .sink
            .notifications(notifier.host_failure(&facts, &reason));

        // Then the child itself. Two states end here, and both want the same call: an engine
        // that was killed is a zombie until somebody waits for it — and the session is out of
        // the registry now, so nobody else ever will — while an engine whose stdout stopped
        // being readable is still running and must not be left writing into a pipe nobody
        // drains. `wait_or_kill` covers both: it returns at once for a dead child.
        let _ = client.shutdown(SHUTDOWN_GRACE).await;

        reporting.roster.retire(&thread);
        reporting.sink.threads(reporting.roster.snapshots());

        if cfg!(debug_assertions) {
            println!("[omp-desktop] the engine is gone: {reason}");
        }
    })
}

/// The blocking dialog a frame asks for but cannot be decoded into, or `None`.
///
/// `ui::decode` answers `None` for a chrome frame *and* for a blocking one whose frame is
/// unusable — a `select` with no string `id`, say. Only the second is a problem: the engine
/// has stopped and is waiting, and the sentence names the method and the id it sent, because
/// the request the host could not read is also the request it cannot answer.
///
/// The readability check is repeated here rather than left to the caller, so this cannot be
/// misused into reporting a dialog that is on screen and answerable.
pub(super) fn unusable_dialog(frame: &serde_json::Value) -> Option<String> {
    if ui::decode(frame).is_some() {
        return None;
    }

    if frame.get("type").and_then(serde_json::Value::as_str) != Some("extension_ui_request") {
        return None;
    }

    let method = frame
        .get("method")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let kind = ui::UiRequestKind::from_method(method)?;

    // `decode` returned `None` for it, so something it requires is missing; the id is the one
    // a reader needs to know which request is stuck, and `get("id")` is rendered as it
    // arrived so a non-string id is visible rather than reported as absent.
    Some(format!(
        "the engine is waiting on a `{}` dialog the host could not read (id: {})",
        kind.as_str(),
        frame
            .get("id")
            .map(ToString::to_string)
            .unwrap_or_else(|| "none".to_string())
    ))
}

/// Whether an event can earn a notification at all.
///
/// [`Notifier::event`] answers exactly two shapes — a terminal end, which reports the turn
/// as finished or failed, and a retry that gave up — so those are the only events whose
/// [`facts`] are worth building. Everything else is a streamed delta or bookkeeping, and
/// the facts carry the engine's *whole* last answer: joining that text once per frame to
/// drop it again is the amplifier, not the emit.
///
/// The notifier's two shapes, restated as the gate the pump reads: the transport's
/// [`SessionEvent::ends_run`] is the completion rule both use, and the retry arm is the one
/// shape the notifier adds on top of it. The unit test beside the pump's helpers pins the
/// gate and the notifier together, so neither can drift without the other failing.
pub(super) fn earns_notification(event: &SessionEvent) -> bool {
    event.ends_run() || matches!(event, SessionEvent::AutoRetryEnd(end) if !end.success)
}

/// What the host knows about a thread when a notification trigger fires.
///
/// Read under the two locks the pump already takes and returned owned: the emit happens
/// outside both, so a window's event loop is never inside the pump's critical section.
pub(super) fn facts(thread: &str, control: &Mutex<SessionControl>, transcript: &Mutex<Transcript>) -> Facts {
    let name = control
        .lock()
        .ok()
        .and_then(|control| control.session_name.clone());
    let (failure, answer) = transcript
        .lock()
        .map(|transcript| (last_failure(&transcript), last_answer(&transcript)))
        .unwrap_or((None, None));

    Facts {
        thread: thread.to_string(),
        name,
        failure,
        answer,
    }
}

/// The engine's most recent assistant text, if it has said anything yet.
pub(super) fn last_answer(transcript: &Transcript) -> Option<String> {
    transcript.rows().iter().rev().find_map(|row| match row {
        omp_session::Row::Assistant { message, .. } => optional(message.text()),
        _ => None,
    })
}
