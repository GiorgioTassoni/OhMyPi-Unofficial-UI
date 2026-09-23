//! The session actor: one live agent, its reducers, and the task that feeds them.
//!
//! This is where the app's two streams meet the reducers from `omp-session`:
//!
//! ```text
//!   OmpClient ──events──▶ Transcript    (the conversation)
//!            └─frames──▶ SessionControl (the chrome) + counters
//!                     └─▶ UiRequests    (the dialogs the engine waits on)
//! ```
//!
//! The two frame-derived reducers are split because they mean different things: a
//! status frame is chrome the app owns, while an `extension_ui_request` is a
//! question the agent will wait forever for. The second one is the app's safety
//! boundary (D3), so it is the one that must never be dropped.
//!
//! Everything above this module (the Tauri commands) is a thin adapter; everything
//! below (the transport) knows nothing about windows.

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use omp_session::{
    restore, AgentRoster, Message, Row, SessionControl, Subagent, TodoPhase, Transcript,
};
use omp_transport::palette::{self, AdvertisedCommand};
use omp_transport::protocol::ui::{self, UiResponse};
use omp_transport::protocol::{self, commands, ImageContent};
use omp_transport::{ClientOptions, OmpClient, SessionEvent, SidecarSpec};
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

/// How often a changed conversation is sent to the UI.
///
/// A streaming turn emits deltas far faster than a screen refreshes, and
/// re-rendering per delta is the jank `docs/12` §3.4 rules out. Nothing is lost by
/// waiting a tick: a patch always starts at the earliest change, so a quiet tick
/// sends nothing at all.
const ROWS_PUBLISH_INTERVAL: Duration = Duration::from_millis(100);

/// Two idle samples avoid mistaking the gap between an accepted RPC prompt and
/// OMP's `agent_start` for a finished turn.
const MODE_SWITCH_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// How long the engine gets to finish cleanly before it is killed.
///
/// Defined here rather than in `bridge` because two layers need it now and they must not
/// disagree: the commands that close a session, and the pump itself — a reader that has
/// stopped means either a dead child to reap (a killed engine stays a zombie until somebody
/// waits for it) or an engine whose stdout can no longer be drained, which must be stopped
/// rather than left writing into a pipe nobody reads.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Title generation is a model call, so it gets a model-call timeout rather than the
/// transport's short command-acceptance timeout.
const TITLE_GENERATION_TIMEOUT: Duration = Duration::from_secs(120);

/// The level asked for at open, so the engine pushes its subagent roster.
///
/// The engine's default is `off`, so nothing about a subagent arrives unless a client
/// asks (`docs/12` §9). `progress` is the level whose frames carry every field the agents
/// panel renders — the snapshot the registry folds them into is the same one
/// `get_subagents` answers with. `events` would additionally forward each subagent's own
/// `AgentSessionEvent` stream; the panel reads a subagent's transcript on demand through
/// `get_subagent_messages`' byte cursor instead, so paying for a live firehose per
/// subagent would buy a second way to see the same thing.
const SUBAGENT_SUBSCRIPTION: &str = "progress";

/// Counters shared between the pump and the status reads.
///
/// Kept as atomics rather than behind a mutex: the pump writes on every event and
/// the UI may read at any moment, so neither should ever wait for the other.
#[derive(Debug, Default)]
pub struct Counters {
    pub events_seen: AtomicU64,
    pub frames_seen: AtomicU64,
    pub distinct_kinds: AtomicU64,
    pub unknown_events: AtomicU64,
    pub malformed_events: AtomicU64,
    pub lagged_events: AtomicU64,
    /// Every host UI request seen, informational ones included.
    pub ui_requests: AtomicU64,
    /// The subset that **block the run** until the host answers. Only these mean
    /// "the agent is stuck"; a `setWidget` from an extension is fire-and-forget
    /// and must not be reported as a stall.
    pub blocking_ui_requests: AtomicU64,
}

impl From<&Counters> for CounterSnapshot {
    fn from(counters: &Counters) -> Self {
        Self {
            events_seen: counters.events_seen.load(Ordering::Relaxed),
            frames_seen: counters.frames_seen.load(Ordering::Relaxed),
            event_kinds: counters.distinct_kinds.load(Ordering::Relaxed),
            unknown_events: counters.unknown_events.load(Ordering::Relaxed),
            malformed_events: counters.malformed_events.load(Ordering::Relaxed),
            lagged_events: counters.lagged_events.load(Ordering::Relaxed),
            ui_requests: counters.ui_requests.load(Ordering::Relaxed),
            blocking_ui_requests: counters.blocking_ui_requests.load(Ordering::Relaxed),
        }
    }
}

/// The engine's session id, as a value that can move.
///
/// A cell rather than a `String` because a thread's identity is not a constant for its
/// sidecar's lifetime: `branch` mints a **new session in the same sidecar** (measured —
/// the response is `{text, cancelled}` and says nothing about identity), after which the
/// old id stops existing. Three things are keyed by that id — the registry the sidebar
/// lists, the tag on every published event, and the catalogue's own row — so they read
/// one cell rather than three copies of it that a fork would leave disagreeing.
///
/// A mutex, not an atomic: the value is a `String`, and the lock only ever covers a
/// clone or a swap of one.
#[derive(Debug, Clone, Default)]
pub struct ThreadId(Arc<Mutex<String>>);

impl ThreadId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(Arc::new(Mutex::new(id.into())))
    }

    /// The id the engine is reporting right now.
    pub fn current(&self) -> String {
        self.lock().clone()
    }

    /// Follow the engine to the session it just moved this thread to.
    ///
    /// Deliberately not called from [`refresh_control`], which is where the new id is
    /// *read*: moving a thread leaves the registry keyed by an id nothing answers to
    /// unless the caller refiles it in the same breath, and a silent move inside a
    /// refresh is exactly how that would happen. `flows::branch` is the one caller.
    pub fn set(&self, id: impl Into<String>) {
        *self.lock() = id.into();
    }

    /// The lock, recovering a poisoned one.
    ///
    /// It only ever covers a `String`, so a panic while holding it cannot have left the
    /// id half-written — whereas refusing to read a thread's own id would make every
    /// event from a live session untaggable.
    fn lock(&self) -> std::sync::MutexGuard<'_, String> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// A live agent session.
///
/// The client lives behind a `Mutex<Option<_>>` so that "closed" is a state the
/// actor can hold: `client()` answers `None` once the session has been shut down,
/// and no caller has to guess whether a handle it still holds is usable.
#[derive(Debug)]
pub struct LiveSession {
    pub client: Mutex<Option<Arc<OmpClient>>>,
    pub ready: omp_transport::ReadyFrame,
    pub binary: String,
    pub workspace: String,
    /// The engine's session id (`get_state.sessionId`), read during the handshake.
    ///
    /// The thread's identity, and deliberately the engine's rather than one the app
    /// mints: the on-disk catalogue lists sessions by this id and a resume resolves it
    /// back to a session file, so an app-owned id would need a mapping table that
    /// nothing else in the ecosystem uses.
    pub thread: ThreadId,
    /// The approval mode this process was launched with, from the spec.
    pub approval_mode: Option<String>,
    /// Serializes user turn submissions with a mode switch becoming pending.
    turn_gate: AsyncMutex<()>,
    mode_switch_pending: AtomicBool,
    pub transcript: Arc<Mutex<Transcript>>,
    pub control: Arc<Mutex<SessionControl>>,
    pub dialogs: Arc<Dialogs>,
    /// Tool grants made while this sidecar is running. OMP reads config only at launch.
    granted_tools: Arc<Mutex<HashSet<String>>>,
    /// Executables the user chose to allow for this conversation only.
    granted_commands: Arc<Mutex<HashSet<String>>>,
    pub counters: Arc<Counters>,
    /// The palette's list, as the engine last advertised it (`docs/12` §7.3).
    ///
    /// Per session rather than app-wide, unlike the model catalogue: two of the four
    /// sources (`file`, `custom`) come from the workspace, so two windows on two projects
    /// legitimately see different commands. The pump refreshes it on
    /// `available_commands_update`; [`LiveSession::commands`] fetches it the first time.
    pub commands: Arc<Mutex<Vec<AdvertisedCommand>>>,
    /// The subagents this session has spawned, as the frames and `get_subagents` describe
    /// them (`docs/12` §9).
    ///
    /// Held rather than fetched on demand because the engine's own registry forgets a
    /// settled agent the moment it settles; what this holds is the only place a finished
    /// subagent is still a row.
    pub agents: Arc<Mutex<AgentRoster>>,
    /// Why this session has no roster to speak of, when the engine refused the
    /// subscription at open ([`SUBAGENT_SUBSCRIPTION`]).
    ///
    /// `None` with an empty roster means "no subagents", which is a different answer from
    /// "the engine will not say" — and the panel shows them differently.
    ///
    /// Write-once, deliberately: [`LiveSession::agents`] polls the live roster later and only
    /// *logs* a miss, so a refusal here is never cleared by a later poll succeeding. A roster
    /// the engine would not serve once must not end up looking healthy afterwards — the panel
    /// would then show an empty roster as "no agents", which is the claim this field exists to
    /// avoid.
    pub agents_error: Option<String>,
    /// Prevent two fast prompts from starting two model calls for the same missing title.
    title_generation_in_flight: Arc<AtomicBool>,
    pub pump: Mutex<Option<JoinHandle<()>>>,
}

/// Releases a pending switch if the request fails or the caller goes away.
pub struct ModeSwitchGuard<'a> {
    pending: &'a AtomicBool,
}

impl Drop for ModeSwitchGuard<'_> {
    fn drop(&mut self) {
        self.pending.store(false, Ordering::SeqCst);
    }
}

impl LiveSession {
    /// Stop admitting new user turns while a mode switch waits for this one to finish.
    /// Existing turns and their approval dialogs remain usable until they settle.
    pub async fn begin_mode_switch(&self) -> Result<ModeSwitchGuard<'_>, String> {
        let _gate = self.turn_gate.lock().await;
        if self.mode_switch_pending.swap(true, Ordering::SeqCst) {
            return Err("an approval mode switch is already pending".to_string());
        }
        Ok(ModeSwitchGuard {
            pending: &self.mode_switch_pending,
        })
    }

    /// Send a turn-starting command only while no mode switch is pending.
    pub async fn send_user_turn(
        &self,
        command: serde_json::Value,
        what: &str,
    ) -> Result<(), String> {
        let _gate = self.turn_gate.lock().await;
        self.ensure_turn_admitted()?;
        let client = self
            .client()
            .ok_or_else(|| "the session is shutting down".to_string())?;
        send(&client, command, what).await
    }

    /// Wait for the foreground turn (and anything already queued behind it) to finish.
    /// A fresh engine snapshot is needed here: a cached `is_streaming: false` may
    /// precede `agent_start` for a prompt OMP has accepted but not begun yet.
    pub async fn wait_for_turn_to_finish(&self) -> Result<(), String> {
        let mut idle_samples = 0;
        loop {
            let client = self
                .client()
                .ok_or_else(|| "the session closed before its turn finished".to_string())?;
            let response = call(&client, commands::get_state(), None, "session state").await?;
            let fresh = SessionControl::decode(&response["data"])
                .ok_or_else(|| "the agent's state payload had no session id".to_string())?;
            let idle = {
                let cached = self
                    .control
                    .lock()
                    .map_err(|_| "the control state lock was poisoned".to_string())?;
                mode_switch_idle(&fresh, &cached, self.dialogs.count())
            };

            if idle {
                idle_samples += 1;
                if idle_samples >= 2 {
                    return Ok(());
                }
            } else {
                idle_samples = 0;
            }
            tokio::time::sleep(MODE_SWITCH_POLL_INTERVAL).await;
        }
    }

    fn ensure_turn_admitted(&self) -> Result<(), String> {
        if self.mode_switch_pending.load(Ordering::SeqCst) {
            Err("the approval mode will change after the current turn finishes".to_string())
        } else {
            Ok(())
        }
    }

    /// Make a persisted tool grant effective for subsequent approvals in this session.
    pub fn grant_tool(&self, tool: &str) -> Result<(), String> {
        self.granted_tools
            .lock()
            .map_err(|_| "the session's tool grants were unavailable".to_string())?
            .insert(tool.to_string());
        Ok(())
    }

    /// Remember the direct executable from this approved command for this session only.
    pub fn grant_command(&self, command: &str) -> Result<String, String> {
        let program = command_program(command)?;
        self.granted_commands
            .lock()
            .map_err(|_| "the session's command grants were unavailable".to_string())?
            .insert(program.clone());
        Ok(program)
    }

    /// Snapshot everything the UI shows.
    ///
    /// Fallible rather than infallible: a poisoned lock means a thread panicked
    /// while reducing state, and reporting that is more useful than inventing an
    /// empty session that looks like a working one.
    pub fn status(&self) -> Result<SessionStatus, String> {
        let control = self
            .control
            .lock()
            .map_err(|_| "the control state lock was poisoned".to_string())?
            .clone();

        let transcript_rows = self
            .transcript
            .lock()
            .map_err(|_| "the transcript lock was poisoned".to_string())?
            .len();

        Ok(SessionStatus {
            binary: self.binary.clone(),
            workspace: self.workspace.clone(),
            sidecar_pid: self.pid(),
            ready: ReadySnapshot {
                protocol_version: self.ready.protocol_version,
                supported_protocol_versions: self.ready.supported_protocol_versions.clone(),
                max_frame_bytes: self.ready.max_frame_bytes,
                max_reassembled_frame_bytes: self.ready.max_reassembled_frame_bytes,
                negotiated_v2: self.ready.supports_v2(),
            },
            control: ControlSnapshot {
                session_id: control.session_id.clone(),
                session_name: control.session_name.clone(),
                model: control.model.as_ref().map(|model| ModelSnapshot {
                    provider: model.provider.clone(),
                    id: model.id.clone(),
                    name: model.name.clone(),
                }),
                thinking_level: control.thinking_level.clone(),
                is_streaming: control.is_streaming,
                is_compacting: control.is_compacting,
                message_count: control.message_count,
                queued_message_count: control.queued_message_count,
                context: control.context_usage.map(|usage| ContextSnapshot {
                    tokens: usage.tokens,
                    context_window: usage.context_window,
                    percent: usage.percent,
                }),
                todo_phases: crate::panel::phases(&control.todo_phases),
                transcript_rows,
                session_file: control.session_file.clone(),
                auto_compaction_enabled: Some(control.auto_compaction_enabled),
            },
            counters: CounterSnapshot::from(self.counters.as_ref()),
            approval_mode: self.approval_mode.clone(),
        })
    }

    /// The sidebar's row for this thread ([`ThreadSnapshot`], `docs/12` §2.2).
    ///
    /// Derived on every read, and deliberately infallible: a poisoned lock in one
    /// thread's state must not hide every other thread's row, so a value that cannot be
    /// read degrades to the safe reading (not streaming, nothing pending, no error).
    /// [`LiveSession::status`] is the fallible read, for a pane that can show the
    /// problem.
    pub fn snapshot(&self) -> ThreadSnapshot {
        let (streaming, title) = self
            .control
            .lock()
            .map(|control| (control.is_streaming, control.session_name.clone()))
            .unwrap_or((false, None));

        ThreadSnapshot {
            id: self.thread.current(),
            workspace: self.workspace.clone(),
            title,
            streaming,
            pending_approvals: self.dialogs.count(),
            error: self.last_error(),
        }
    }

    /// The last failed turn, as the conversation recorded it.
    ///
    /// Read back out of the transcript rather than tracked beside it (`docs/12` §2.2
    /// draws the red dot from a failed turn or an exhausted `auto_retry_*`): the reducer
    /// is already the one place a failure becomes something the user can see, and a
    /// second flag would be a second answer to the same question. Absent when no turn has
    /// failed, which is every fresh thread.
    fn last_error(&self) -> Option<String> {
        let transcript = self.transcript.lock().ok()?;

        last_failure(&transcript)
    }

    /// The client handle, when the session is still open.
    pub fn client(&self) -> Option<Arc<OmpClient>> {
        self.client.lock().ok().and_then(|slot| slot.clone())
    }

    pub fn pid(&self) -> Option<u32> {
        self.client
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().and_then(|client| client.pid()))
    }

    /// The transcript's rows, flattened for the frontend.
    ///
    /// The initial read, not the update path: a patch published before the window
    /// subscribed would be lost, so a freshly opened thread loads its conversation
    /// once and then follows [`ROWS_EVENT`].
    pub fn rows(&self) -> Result<Vec<RowSnapshot>, String> {
        let transcript = self
            .transcript
            .lock()
            .map_err(|_| "the transcript lock was poisoned".to_string())?;

        Ok(transcript.rows().iter().map(row_snapshot).collect())
    }

    /// The dialogs the agent is waiting on, oldest first.
    ///
    /// Read rather than pushed: a dialog can arrive before the window has
    /// subscribed (extensions push one at startup), so the frontend asks once on
    /// open and then relies on [`UI_REQUESTS_EVENT`] for changes.
    pub fn pending_ui_requests(&self) -> Result<Vec<UiRequestSnapshot>, String> {
        self.dialogs.snapshot()
    }

    /// Answer a dialog, writing the response to the engine.
    ///
    /// The dialog leaves the store before the write, not after: a failed write means
    /// the pipe is gone, and the pump clears the set when the stream ends — so there
    /// is nothing left to retry, and a dialog that survived its own answer could be
    /// answered twice.
    ///
    /// Removing it also announces the new set, so the window stops showing the
    /// approval the moment it is decided.
    pub async fn respond(&self, id: &str, response: UiResponse) -> Result<(), String> {
        let frame = self.dialogs.answer(id, response)?;

        let client = self
            .client()
            .ok_or_else(|| "the session is shutting down".to_string())?;

        client
            .send(&frame)
            .await
            .map_err(|error| format!("the answer was not written: {error}"))
    }

    /// Stop the turn in flight: `abort`, in the composer's words.
    pub async fn stop_turn(&self) -> Result<(), String> {
        self.end_turn(commands::abort(), "stop").await
    }

    /// Stop the turn in flight and send a message in its place.
    ///
    /// Takes the images for the same reason `prompt` does: an attachment is part of
    /// the message being sent, and `abort_and_prompt` carries them (`docs/rpc.md`).
    pub async fn stop_turn_and_send(
        &self,
        message: String,
        images: &[ImageContent],
    ) -> Result<(), String> {
        let _gate = self.turn_gate.lock().await;
        self.ensure_turn_admitted()?;
        self.end_turn(commands::abort_and_prompt(message, images), "stop-and-send")
            .await
    }

    /// Refuse whatever the run is waiting on, then send the command that ends it.
    ///
    /// **Measured at v18.2.6: `abort` alone does not stop a parked run.** A turn
    /// waiting on an approval is blocked inside the tool call, and the engine's
    /// `abort` handler waits for the turn to stop before answering — so the command
    /// times out with the dialog still on screen (30 s, no response, reproduced).
    /// The dialog is the only way out, so stopping refuses what is pending first.
    ///
    /// Which is also what a stop button means: the approval on screen is part of the
    /// run the user is trying to end.
    async fn end_turn(&self, command: serde_json::Value, what: &str) -> Result<(), String> {
        // `cancelled` is the documented "this request will not be answered" response
        // (`docs/12` §10) — the same frame the dialog's own dismiss sends. Refusing is
        // safe here because the turn is stopped immediately afterwards, so nothing is
        // left waiting on the refusal.
        for dialog in self.pending_ui_requests()? {
            // `timed_out: false` — this is the host refusing, not the engine's
            // deadline expiring, and the engine reports the two differently.
            let refusal = UiResponse::Cancelled { timed_out: false };
            if let Err(error) = self.respond(&dialog.id, refusal).await {
                // A dialog that vanished between the read and the write is already
                // resolved, which is what this loop wanted.
                eprintln!(
                    "[omp-desktop] could not refuse dialog {}: {error}",
                    dialog.id
                );
            }
        }

        let client = self
            .client()
            .ok_or_else(|| "the session is shutting down".to_string())?;

        send(&client, command, what).await
    }

    /// Re-read the control-plane snapshot from the engine.
    ///
    /// Not every change to a chip has an event behind it: toggling auto-compaction emits
    /// nothing (the `auto_compaction_*` frames describe a compaction *running*, not the
    /// setting), so a caller that changes session state the chrome renders asks for the
    /// snapshot rather than leaving [`LiveSession::status`] behind until the next turn
    /// ends. `docs/12` §7.5 reads that toggle straight from `get_state`.
    ///
    /// Failures are logged and dropped by callers that only need the chrome current,
    /// which is what makes this safe to call after a command whose effect has already
    /// been accepted: a missed refresh is a stale chip, not a failed action. It is
    /// returned as well as folded in because a flow's *next step* can depend on what
    /// moved — `flows::branch` reads the thread's new session id out of it.
    pub async fn reread_control(&self) -> Result<SessionControl, String> {
        let Some(client) = self.client() else {
            return Err("the session is shutting down".to_string());
        };

        refresh_control(&client, &self.control).await
    }

    /// The thread's plan, re-read where there is an engine to ask (`docs/12` §8.1).
    ///
    /// `get_state` is the only source there is: measured, the engine's event stream carries
    /// `todo_reminder` and `todo_auto_clear` and nothing else todo-shaped, so the plan
    /// moving is noticed through the re-read the `todo` tool's end triggers
    /// ([`SessionControl::apply`]) and through this call when the panel opens — which can
    /// be any moment, and later than the cached control.
    ///
    /// A sidecar that will not answer falls back to the cached control rather than failing,
    /// for the same reason a closed thread does: there is nothing to ask, and the plan the
    /// panel last showed beats an error over an empty column.
    pub async fn todo_phases(&self) -> Vec<TodoPhase> {
        match self.reread_control().await {
            Ok(control) => control.todo_phases,
            Err(error) => {
                eprintln!("[omp-desktop] could not re-read the plan: {error}");
                self.cached_phases()
            }
        }
    }

    /// Replace the plan, and answer with the list the engine stored.
    ///
    /// Two measured properties decide the shape of this call:
    ///
    /// * the engine stores what it is given, in memory only — nothing is persisted and no
    ///   event is emitted — so the answer must be rendered rather than an optimistic copy
    ///   of the request, and the host must not "correct" a plan on its way through;
    /// * the answer is the engine's own projection of it (`{name, tasks:[{content, status,
    ///   blocker?}]}`), so folding it into the cached control is not a guess at what
    ///   happened: it is what the engine says it holds.
    ///
    /// The plan therefore survives until the thread is resumed, at which point the engine
    /// rebuilds it from the `todo` tool calls in the transcript — which is where a durable
    /// plan lives.
    pub async fn set_todo_phases(
        &self,
        phases: serde_json::Value,
    ) -> Result<Vec<TodoPhase>, String> {
        let client = self
            .client()
            .ok_or_else(|| "the session is shutting down".to_string())?;

        let response = call(&client, commands::set_todos(phases), None, "the todo list").await?;

        let stored = {
            let raw = &response["data"]["todoPhases"];
            if !raw.is_array() {
                return Err(format!(
                    "the agent's todo answer carried no list: {response}"
                ));
            }

            omp_session::decode_phases(raw)
        };

        if let Ok(mut control) = self.control.lock() {
            control.todo_phases = stored.clone();
        }

        Ok(stored)
    }

    /// `get_state.sessionFile`, for the artifacts that live beside it (`docs/12` §8.2).
    ///
    /// Absent when the engine runs without a session store (`--no-session`): a session
    /// with no file has no artifacts directory either, so there is nothing to resolve.
    pub fn session_file(&self) -> Option<String> {
        self.control
            .lock()
            .ok()
            .and_then(|control| control.session_file.clone())
    }

    /// The plan as last read, without asking the engine.
    fn cached_phases(&self) -> Vec<TodoPhase> {
        self.control
            .lock()
            .map(|control| control.todo_phases.clone())
            .unwrap_or_default()
    }

    /// The commands the palette offers (`docs/12` §7.3).
    ///
    /// Fetched the first time and cached after: the engine also *pushes* the same array
    /// at startup, but measured, that push is emitted **before** a subscriber can attach
    /// to the frame stream — so waiting for it would leave a fresh session's palette
    /// empty forever. Later changes do arrive as `available_commands_update`, which the
    /// pump turns into a refresh (and an event, for a window that is already open).
    /// This session's agents, with the engine's live list folded in.
    ///
    /// The reducer holds the rows, because the frames are the only place a *settled*
    /// subagent remains one; this is the poll that corrects them, since `get_subagents` is
    /// authoritative for which agents are live and carries the `task` text no frame does
    /// (`omp_session::agents`). A sidecar that will not answer is not fatal — the frames
    /// keep the roster going, and a stale roster beats an empty one — but it is not silent
    /// either, because the alternative to a logged miss is a panel that quietly stops
    /// updating.
    pub async fn agents(&self) -> Vec<Subagent> {
        let client = self.client.lock().ok().and_then(|slot| slot.clone());

        if let Some(client) = client {
            match client.request(commands::get_subagents(), None).await {
                Ok(response) => {
                    let live: Vec<Subagent> = response["data"]["subagents"]
                        .as_array()
                        .map(|rows| rows.iter().filter_map(Subagent::decode_snapshot).collect())
                        .unwrap_or_default();

                    if let Ok(mut roster) = self.agents.lock() {
                        roster.reconcile(&live, now_ms());
                    }
                }
                Err(error) => eprintln!("[omp-desktop] could not read the agent roster: {error}"),
            }
        }

        self.agents
            .lock()
            .map(|roster| roster.agents().to_vec())
            .unwrap_or_default()
    }

    /// One page of a subagent's transcript, as the engine will serve it.
    ///
    /// `None` means the engine would not answer — an id its registry has forgotten (a fork,
    /// a switch, a restart all clear it), or no engine at all — and the caller falls back to
    /// the file the subagent wrote. Nothing about that is exceptional enough to report: the
    /// transcript is on disk either way, and which of the two answered is a fact the panel
    /// shows rather than a failure.
    ///
    /// Rows come back in the app's own shape so a subagent's conversation renders through
    /// the same rows as the thread that spawned it.
    pub async fn agent_messages(&self, agent: &str, from_byte: u64) -> Option<AgentPage> {
        let client = self.client.lock().ok().and_then(|slot| slot.clone())?;

        let response = match client
            .request(
                commands::get_subagent_messages(Some(agent), None, Some(from_byte)),
                None,
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                if cfg!(debug_assertions) {
                    println!("[omp-desktop] the engine would not serve `{agent}`: {error}");
                }
                return None;
            }
        };

        let data = &response["data"];
        let messages = data["messages"]
            .as_array()
            .map(|messages| messages.iter().map(Message::decode).collect::<Vec<_>>())
            .unwrap_or_default();

        Some(AgentPage {
            session_file: data
                .get("sessionFile")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            next_byte: data
                .get("nextByte")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(from_byte),
            reset: data
                .get("reset")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            rows: messages
                .iter()
                .map(|message| row_snapshot(&Row::Message(message.clone())))
                .collect(),
        })
    }

    pub async fn commands(&self) -> Result<Vec<CommandSnapshot>, String> {
        if let Ok(cached) = self.commands.lock() {
            if !cached.is_empty() {
                return Ok(command_snapshots(&cached));
            }
        }

        let client = self
            .client()
            .ok_or_else(|| "the session is shutting down".to_string())?;

        let response = client
            .request(commands::get_available_commands(), None)
            .await
            .map_err(|error| format!("the agent did not answer get_available_commands: {error}"))?;

        if !protocol::is_success(&response) {
            return Err(format!(
                "the agent refused get_available_commands: {response}"
            ));
        }

        let list = palette::commands_from(&response);
        if let Ok(mut cached) = self.commands.lock() {
            *cached = list.clone();
        }

        Ok(command_snapshots(&list))
    }

    /// Stop the pump and close the session gracefully.
    ///
    /// The pump is stopped first so it cannot race the shutdown for the client's
    /// streams; then `shutdown` closes stdin and lets the engine exit 0, flushing
    /// its session file rather than being killed mid-write.
    ///
    /// This works even while other handles to the client exist — `shutdown` takes
    /// `&self` — which matters because "someone else holds a reference" must never
    /// turn into an orphaned agent process.
    pub async fn shutdown(&self, grace: Duration) {
        let task = self.pump.lock().ok().and_then(|mut slot| slot.take());
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }

        let client = self.client.lock().ok().and_then(|slot| slot.clone());
        let Some(client) = client else {
            return;
        };

        match client.shutdown(grace).await {
            Ok(Some(code)) => {
                if cfg!(debug_assertions) {
                    println!("[omp-desktop] the agent exited with {code}");
                }
            }
            Ok(None) => {}
            Err(error) => {
                // Draining is best-effort; the child must not survive either way.
                eprintln!("[omp-desktop] the agent did not drain cleanly: {error}");
                client.terminate();
            }
        }
    }
}

fn mode_switch_idle(
    fresh: &SessionControl,
    cached: &SessionControl,
    pending_dialogs: usize,
) -> bool {
    !fresh.is_streaming
        && !fresh.is_compacting
        && fresh.queued_message_count == 0
        && !cached.is_streaming
        && !cached.is_compacting
        && pending_dialogs == 0
}

/// One page of a subagent's transcript, as the engine served it.
pub struct AgentPage {
    /// The file the page came from, per the engine.
    pub session_file: String,
    /// Where the next page starts.
    pub next_byte: u64,
    /// The file shrank under the cursor, so this page replaces what was shown.
    pub reset: bool,
    pub rows: Vec<RowSnapshot>,
}

/// Where streamed activity goes.
///
/// A seam rather than a direct `AppHandle` dependency: it keeps the session actor
/// free of Tauri types, so it can be exercised headlessly (`tests/m0.rs`,
/// `tests/approval.rs`) instead of only through a window. The same shape as
/// `PageSource` in `omp_session`.
///
/// Every session-scoped stream carries the thread it came from. Two live sessions are
/// otherwise indistinguishable to a subscriber, and a row patch from one folded into the
/// other's conversation is the exact bug `docs/12` §2.2's sidebar exists to avoid.
pub trait ActivitySink: Send + Sync + 'static {
    fn activity(&self, thread: &str, activity: ActivitySnapshot);

    /// The full pending-dialog set, after any change to it.
    fn ui_requests(&self, thread: &str, requests: Vec<UiRequestSnapshot>);

    /// The conversation, replaced from the first changed row.
    fn rows(&self, thread: &str, patch: RowPatch);

    /// The palette's list, after the engine changed it.
    fn commands(&self, thread: &str, commands: Vec<CommandSnapshot>);

    /// One thread's agent roster, after a subagent started, reported or settled.
    fn agents(&self, thread: &str, agents: Vec<AgentSnapshot>);

    /// One fact the host is reporting, addressed to the thread it is about (`docs/12` §13).
    ///
    /// Carries its own thread rather than being wrapped in [`ThreadEvent`]: a job the engine
    /// recorded no owner for belongs to no thread, and an envelope that required one would
    /// force the host to invent it.
    fn notifications(&self, notification: NotificationEvent);

    /// One fire-and-forget chrome request from the engine (`docs/12` §13).
    fn chrome(&self, event: ChromeEvent);

    /// The sidebar's roster, after anything in it changed.
    ///
    /// The one stream that is **not** thread-tagged: the roster is every live thread,
    /// which no single session can describe. The caller decides *when* (the pump on its
    /// tick, the registry on open and close); the roster itself comes from [`Roster`].
    fn threads(&self, snapshots: Vec<ThreadSnapshot>);
}

/// The live-thread roster, as the pump needs it.
///
/// The pump is what notices a turn starting or a dialog arriving, so it is what has to
/// republish the sidebar's rows — but the roster is the *set* of live threads, which
/// belongs to the registry ([`crate::threads::Threads`]). This trait is that seam, in
/// the same spirit as [`ActivitySink`]: the actor knows it has a roster to announce,
/// never how one is assembled.
pub trait Roster: Send + Sync + 'static {
    /// Every live thread's row, in a stable order.
    fn snapshots(&self) -> Vec<ThreadSnapshot>;

    /// Retire a thread whose engine is gone, for good.
    ///
    /// The pump is what notices a sidecar dying — its frame stream simply ends, and its
    /// broadcast senders live inside the client the pump itself holds, so no `Closed` ever
    /// arrives on them. Without this the registry would keep serving a row for a thread with
    /// no process: a streaming dot over a conversation that will never move again, and
    /// commands written into a pipe with nobody on the far end (`docs/14` §7 #6).
    ///
    /// Defaults to doing nothing, for a roster that is only ever read.
    fn retire(&self, _thread: &str) {}
}

/// Where one live session's activity goes, and which thread it belongs to.
///
/// One value rather than a stream of arguments because the three halves are one thing: a
/// session-scoped event without its thread tag, or a tick that never republishes the
/// roster, is exactly the multi-thread bug this layer exists to prevent. The pump and
/// the dialog store both hold a copy of it, which is why it is cheap to clone.
#[derive(Clone)]
pub struct Reporting {
    pub sink: Arc<dyn ActivitySink>,
    /// The engine's session id — the tag on every event this session emits.
    ///
    /// The same cell the [`LiveSession`] holds, so a fork's new id is what the *next*
    /// event is tagged with rather than something a second copy has to be told about.
    pub thread: ThreadId,
    /// The registry the sidebar reads, republished when this thread's state moves.
    pub roster: Arc<dyn Roster>,
}

impl Reporting {
    /// The thread's event tail: one tagged envelope per session event.
    pub fn activity(&self, activity: ActivitySnapshot) {
        self.sink.activity(&self.thread.current(), activity);
    }

    /// This thread's agent roster, re-rendered from the shared reducer.
    ///
    /// Copied out of the lock before publishing, deliberately: the sink is a Tauri emit,
    /// and holding a mutex the pump also writes from across it would make a window's
    /// event loop part of the pump's critical path.
    pub fn agents(&self, agents: &Mutex<AgentRoster>) {
        let snapshots = agents
            .lock()
            .map(|roster| roster.agents().iter().map(agent_snapshot).collect())
            .unwrap_or_default();

        self.sink.agents(&self.thread.current(), snapshots);
    }
}

impl ActivitySink for AppHandle {
    fn activity(&self, thread: &str, activity: ActivitySnapshot) {
        // `Emitter::emit` needs the event name; the sink exists so callers do not.
        let _ = self.emit(ACTIVITY_EVENT, tagged(thread, activity));
    }

    fn ui_requests(&self, thread: &str, requests: Vec<UiRequestSnapshot>) {
        let _ = self.emit(UI_REQUESTS_EVENT, tagged(thread, requests));
    }

    fn rows(&self, thread: &str, patch: RowPatch) {
        let _ = self.emit(ROWS_EVENT, tagged(thread, patch));
    }

    fn commands(&self, thread: &str, commands: Vec<CommandSnapshot>) {
        let _ = self.emit(COMMANDS_EVENT, tagged(thread, commands));
    }

    fn agents(&self, thread: &str, agents: Vec<AgentSnapshot>) {
        let _ = self.emit(AGENTS_EVENT, tagged(thread, agents));
    }

    fn notifications(&self, notification: NotificationEvent) {
        let _ = self.emit(NOTIFICATIONS_EVENT, notification);
    }

    fn chrome(&self, event: ChromeEvent) {
        let _ = self.emit(CHROME_EVENT, event);
    }

    fn threads(&self, snapshots: Vec<ThreadSnapshot>) {
        let _ = self.emit(THREADS_EVENT, snapshots);
    }
}

/// One event, addressed to the thread that produced it.
fn tagged<T>(thread: &str, payload: T) -> ThreadEvent<T> {
    ThreadEvent {
        thread: thread.to_string(),
        payload,
    }
}

/// So one sink can be shared: the actor and the dialogs hold the same handle.
impl<T: ActivitySink + ?Sized> ActivitySink for Arc<T> {
    fn activity(&self, thread: &str, activity: ActivitySnapshot) {
        (**self).activity(thread, activity);
    }

    fn agents(&self, thread: &str, agents: Vec<AgentSnapshot>) {
        (**self).agents(thread, agents);
    }

    fn ui_requests(&self, thread: &str, requests: Vec<UiRequestSnapshot>) {
        (**self).ui_requests(thread, requests);
    }

    fn rows(&self, thread: &str, patch: RowPatch) {
        (**self).rows(thread, patch);
    }

    fn commands(&self, thread: &str, commands: Vec<CommandSnapshot>) {
        (**self).commands(thread, commands);
    }

    fn notifications(&self, notification: NotificationEvent) {
        (**self).notifications(notification);
    }

    fn chrome(&self, event: ChromeEvent) {
        (**self).chrome(event);
    }

    fn threads(&self, snapshots: Vec<ThreadSnapshot>) {
        (**self).threads(snapshots);
    }
}

/// An [`ActivitySink`] that discards activity, for headless runs and tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullSink;

impl ActivitySink for NullSink {
    fn activity(&self, _thread: &str, _activity: ActivitySnapshot) {}

    fn ui_requests(&self, _thread: &str, _requests: Vec<UiRequestSnapshot>) {}

    fn rows(&self, _thread: &str, _patch: RowPatch) {}

    fn commands(&self, _thread: &str, _commands: Vec<CommandSnapshot>) {}

    fn agents(&self, _thread: &str, _agents: Vec<AgentSnapshot>) {}

    fn notifications(&self, _notification: NotificationEvent) {}

    fn chrome(&self, _event: ChromeEvent) {}

    fn threads(&self, _snapshots: Vec<ThreadSnapshot>) {}
}

/// Spawn, handshake, negotiate v2, read the starting state, and start reducing.
///
/// The whole M0 path in one function, deliberately free of Tauri types so a test
/// can call exactly what the `open_thread` command calls.
///
/// `roster` is the registry the pump republishes the sidebar's rows through: the
/// session does not need to be in it yet (the caller registers the session once this
/// returns, under the id this function settles on).
pub async fn open(
    spec: &SidecarSpec,
    options: ClientOptions,
    sink: impl ActivitySink,
    roster: Arc<dyn Roster>,
) -> Result<Arc<LiveSession>, String> {
    let binary = spec.program.display().to_string();
    let workspace = spec
        .cwd
        .as_ref()
        .map(|cwd| cwd.display().to_string())
        .unwrap_or_default();

    let client = Arc::new(
        OmpClient::connect(spec, options)
            .await
            .map_err(|error| format!("could not start the agent: {error}"))?,
    );

    let ready = client
        .ready()
        .ok_or_else(|| "the agent never sent its `ready` handshake".to_string())?;

    let state_response = client
        .request(commands::get_state(), None)
        .await
        .map_err(|error| format!("the agent did not answer get_state: {error}"))?;
    let control = SessionControl::decode(&state_response["data"])
        .ok_or_else(|| "the agent's state payload had no session id".to_string())?;

    let counters = Arc::new(Counters::default());
    let transcript = Arc::new(Mutex::new(Transcript::new()));

    // A resumed session starts its event stream where we connect, so everything the
    // thread already contains is invisible without asking for it — measured: after
    // `--resume`, the transcript was empty until the pager was read (`docs/12` §6.2).
    // Failing the open rather than showing an empty conversation is deliberate: a
    // thread that looks new when it is not is worse than an error that says why.
    if spec.resumed_session_file().is_some() {
        hydrate(&client, &transcript)
            .await
            .map_err(|error| format!("could not restore the resumed conversation: {error}"))?;
    }
    // The thread's identity, read before the state moves into its lock: every event is
    // tagged with it, so the pump cannot start without one. It comes from the same
    // `get_state` that hydrates the chrome below — the protocol's only carrier of
    // session identity (`new_session` and `branch` return none, which is why a caller
    // that changes identity has to re-read this).
    let thread = ThreadId::new(control.session_id.clone());

    // Asked for here, before the pump subscribes: a frame that arrives before it has a
    // consumer is dropped, and the first `subagent_lifecycle` is the one that says an
    // agent exists at all.
    //
    // A refusal is recorded rather than fatal. The engine answers "Subagent event bus is
    // unavailable" in the modes that never had a registry, and a thread whose *conversation*
    // works perfectly is not worth refusing because its agent *roster* cannot be read — but
    // the panel says which of the two it is looking at, because an empty roster and a
    // roster that cannot be read look identical otherwise.
    let roster_error = match client
        .request(
            commands::set_subagent_subscription(SUBAGENT_SUBSCRIPTION),
            None,
        )
        .await
    {
        Ok(_) => None,
        Err(error) => Some(error.to_string()),
    };

    let control_state = Arc::new(Mutex::new(control));
    let reporting = Reporting {
        sink: Arc::new(sink),
        thread: thread.clone(),
        roster: Arc::clone(&roster),
    };
    let dialogs = Arc::new(Dialogs::new(reporting.clone(), Arc::clone(&transcript)));
    let granted_tools = Arc::new(Mutex::new(HashSet::new()));
    let granted_commands = Arc::new(Mutex::new(HashSet::new()));
    let commands = Arc::new(Mutex::new(Vec::new()));

    let agents = Arc::new(Mutex::new(AgentRoster::new()));

    let title_generation_in_flight = Arc::new(AtomicBool::new(false));
    let pump = spawn_pump(
        reporting,
        Arc::clone(&client),
        Reducers {
            transcript: Arc::clone(&transcript),
            control: Arc::clone(&control_state),
            dialogs: Arc::clone(&dialogs),
            granted_tools: Arc::clone(&granted_tools),
            granted_commands: Arc::clone(&granted_commands),
            counters: Arc::clone(&counters),
            commands: Arc::clone(&commands),
            agents: Arc::clone(&agents),
            title_generation_in_flight: Arc::clone(&title_generation_in_flight),
        },
    );

    Ok(Arc::new(LiveSession {
        client: Mutex::new(Some(client)),
        ready,
        binary,
        workspace,
        thread,
        approval_mode: spec.approval_mode().map(str::to_string),
        turn_gate: AsyncMutex::new(()),
        mode_switch_pending: AtomicBool::new(false),
        transcript,
        control: control_state,
        dialogs,
        granted_tools,
        granted_commands,
        counters,
        commands,
        agents,
        agents_error: roster_error,
        title_generation_in_flight,
        pump: Mutex::new(Some(pump)),
    }))
}

/// Re-read the whole conversation from the engine, replacing what the transcript holds.
///
/// The one path that builds a transcript from the engine's message **list** rather than
/// from its event stream, and two callers need it for the same reason: the stream starts
/// where the host connects, so a history that already exists is invisible without asking
/// for it. Measured, both cases are real — after `--resume` the transcript was empty until
/// the pager was read (`docs/12` §6.2), and after `branch` the engine replaces its own
/// messages in place and emits no replay at all.
///
/// Failing rather than leaving the old rows is deliberate: a thread that shows the
/// conversation it used to have is worse than one that says it could not read the new one.
/// (The caller decides whether that failure is fatal — `open` refuses the thread, a fork
/// reports it after the engine has already moved.)
pub async fn hydrate(client: &OmpClient, transcript: &Mutex<Transcript>) -> Result<(), String> {
    let outcome = restore::restore_history(client, &restore::RestorePolicy::default())
        .await
        .map_err(|error| error.to_string())?;

    transcript
        .lock()
        .map_err(|_| "the transcript lock was poisoned during restore".to_string())?
        .restore(&outcome.messages);

    Ok(())
}

/// The palette's rows, flattened for the frontend (`dto::CommandSnapshot`).
fn command_snapshots(commands: &[AdvertisedCommand]) -> Vec<CommandSnapshot> {
    commands
        .iter()
        .map(|command| CommandSnapshot {
            name: command.name.clone(),
            source: command.source.clone(),
            aliases: command.aliases.clone(),
            description: command.description.clone(),
            hint: command.hint.clone(),
            subcommands: command
                .subcommands
                .iter()
                .map(|sub| SubcommandSnapshot {
                    name: sub.name.clone(),
                    description: sub.description.clone(),
                })
                .collect(),
        })
        .collect()
}

/// Send one command and hand back its response frame.
///
/// The one place the outcome of a command the app *awaits an answer* from is read, so the
/// two shapes cannot drift: a `success: false` frame becomes [`refusal`], the engine's own
/// words. Showing them is the point rather than brevity — it is the difference between
/// "the steering message was rejected" and why.
///
/// `timeout` overrides the client's default for the commands whose answer takes real work
/// (`flows::handoff` is a model call, `flows::branch` a session transition); everything
/// else takes the transport's default, which bounds *command acceptance* rather than a
/// whole turn.
pub async fn call(
    client: &OmpClient,
    command: serde_json::Value,
    timeout: Option<Duration>,
    what: &str,
) -> Result<serde_json::Value, String> {
    let response = client
        .request(command, timeout)
        .await
        .map_err(|error| format!("the {what} was not accepted: {error}"))?;

    if protocol::is_success(&response) {
        return Ok(response);
    }

    Err(refusal(&response, what))
}

/// The engine's own reason, for a response that reports failure.
///
/// A refusal without an `error` field is possible (the protocol's own parse failures carry
/// prose only sometimes), so the fallback names the action rather than inventing a reason.
pub fn refusal(response: &serde_json::Value, what: &str) -> String {
    response
        .get("error")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("the agent rejected the {what}"))
}

/// Send one command whose whole outcome is its `response` frame.
///
/// Every command the app sends that only reports accepted-or-refused has this shape —
/// the composer's `prompt`/`steer`/`follow_up`, the turn controls, and the chip row's
/// model/effort/compaction commands — so the body lives here once.
pub async fn send(
    client: &OmpClient,
    command: serde_json::Value,
    what: &str,
) -> Result<(), String> {
    call(client, command, None, what).await.map(|_| ())
}

/// Send a user prompt and, for an unnamed session, ask OMP to generate its title.
///
/// OMP's interactive input controller calls `maybeStartTitleGeneration`; its RPC prompt
/// handler does not. `/rename` without an argument is the RPC-accessible path to that same
/// configured title generator, including the tiny-model setting and current-model fallback.
pub async fn prompt(
    live: Arc<LiveSession>,
    message: String,
    images: &[ImageContent],
) -> Result<(), String> {
    live.send_user_turn(commands::prompt(message, images, None), "prompt")
        .await
}

/// Ask OMP to name an existing unnamed session, without blocking the conversation.
///
/// Used both after the first prompt and when an older titleless RPC session is resumed.
pub fn generate_title_if_unnamed(
    live: Arc<LiveSession>,
    sink: impl ActivitySink,
    roster: Arc<dyn Roster>,
) {
    let Some(client) = live.client() else {
        return;
    };
    let reporting = Reporting {
        sink: Arc::new(sink),
        thread: live.thread.clone(),
        roster,
    };
    spawn_title_generation(
        client,
        Arc::clone(&live.control),
        Arc::clone(&live.title_generation_in_flight),
        reporting,
    );
}

/// Run OMP's generator once the foreground turn has settled.
///
/// Generating beside the main model call looks tempting, but OMP deliberately invalidates a
/// title when the session revision changes. Waiting for terminal `agent_end` avoids spending a
/// model call on a result the engine then discards.
fn spawn_title_generation(
    client: Arc<OmpClient>,
    control: Arc<Mutex<SessionControl>>,
    in_flight: Arc<AtomicBool>,
    reporting: Reporting,
) {
    let unnamed = control
        .lock()
        .map(|control| control.session_name.is_none())
        .unwrap_or(false);
    if !unnamed
        || in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return;
    }

    tokio::spawn(async move {
        let result = call(
            &client,
            commands::prompt("/rename", &[], None),
            Some(TITLE_GENERATION_TIMEOUT),
            "generated title",
        )
        .await;
        match result {
            Ok(_) => {
                // `/rename` persists the title but does not reliably emit a
                // `session_info_update`. Read the authoritative value after the command
                // completes and push the live roster immediately; otherwise the disk
                // catalogue is the first place the frontend sees the new name.
                if let Err(error) = refresh_control(&client, &control).await {
                    eprintln!("[omp-desktop] could not refresh generated title: {error}");
                } else {
                    reporting.sink.threads(reporting.roster.snapshots());
                }
            }
            Err(error) => {
                in_flight.store(false, Ordering::Release);
                eprintln!("[omp-desktop] could not generate session title: {error}");
            }
        }
    });
}

/// Internal `/rename` feedback belongs in diagnostics, not in the conversation.
fn is_generated_title_receipt(text: &str) -> bool {
    text.starts_with("Session renamed to ")
        || text.starts_with("Could not generate a session title.")
        || text.starts_with("Session name not changed ")
}

/// One row, flattened for the frontend.
///
/// The single place a transcript row becomes the frontend's contract, shared by
/// the initial read and the patch publisher so the two cannot drift — a mismatch
/// there would show one conversation on load and a different one as it streamed.
/// The jobs an `async-result` delivery accounts for (`docs/12` §9).
///
/// Empty for every other message, which is what makes `customType == "async-result"` and a
/// non-empty list the same claim.
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
        omp_session::Row::Message(message) => RowSnapshot {
            role: message.role().to_string(),
            timestamp: message.timestamp,
            text: message.text(),
            thinking: None,
            streaming: false,
            tool: None,
            attachments: attachments(message),
            custom_type: message.custom_type.clone(),
            jobs: jobs(message),
        },
        omp_session::Row::Assistant { message, streaming } => RowSnapshot {
            role: "assistant".to_string(),
            timestamp: message.timestamp,
            text: message.text(),
            thinking: optional(message.thinking()),
            streaming: *streaming,
            tool: None,
            attachments: attachments(message),
            custom_type: message.custom_type.clone(),
            jobs: jobs(message),
        },
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
        },
        omp_session::Row::Notice { level, text, .. } => RowSnapshot {
            role: format!("notice:{level}"),
            timestamp: None,
            text: text.clone(),
            thinking: None,
            streaming: false,
            tool: None,
            attachments: Vec::new(),
            custom_type: None,
            jobs: Vec::new(),
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
fn last_failure(transcript: &Transcript) -> Option<String> {
    transcript.rows().iter().rev().find_map(|row| match row {
        omp_session::Row::Assistant { message, .. } => match &message.kind {
            omp_session::MessageKind::Assistant {
                stop_reason,
                error_message,
                ..
            } if stop_reason.as_deref() == Some("error") => Some(
                error_message
                    .clone()
                    .unwrap_or_else(|| "the turn failed".to_string()),
            ),
            _ => None,
        },
        omp_session::Row::Notice { level, text, .. } if level == "error" => Some(text.clone()),
        _ => None,
    })
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
fn optional(text: String) -> Option<String> {
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

/// Tell the UI to re-render the conversation from the earliest changed row.
///
/// Called on a tick rather than per event: see [`ROWS_PUBLISH_INTERVAL`].
fn publish_rows(
    reporting: &Reporting,
    transcript: &Mutex<Transcript>,
    first_dirty: &mut Option<usize>,
) {
    let Some(from) = first_dirty.take() else {
        return;
    };

    let Ok(transcript) = transcript.lock() else {
        eprintln!("[omp-desktop] the transcript lock was poisoned");
        return;
    };

    let rows = transcript.rows();
    // Clamped rather than assumed: a marker left over from a longer transcript
    // would otherwise slice out of bounds, and a panic here takes the window down.
    let from = from.min(rows.len());

    reporting.sink.rows(
        &reporting.thread.current(),
        RowPatch {
            from,
            rows: rows[from..].iter().map(row_snapshot).collect(),
        },
    );
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

fn publish_roster(
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

/// Re-read the control-plane snapshot and fold it into the shared state.
///
/// Fallible, because two callers want different things from the same read: the pump and
/// the chip row treat a miss as a stale chip and drop it, while a flow whose *next step*
/// depends on what moved has to say so — a `branch` learns the thread's new session id
/// here and has nothing to rekey the registry with if this fails.
async fn refresh_control(
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
pub struct Reducers {
    pub transcript: Arc<Mutex<Transcript>>,
    pub control: Arc<Mutex<SessionControl>>,
    pub dialogs: Arc<Dialogs>,
    pub granted_tools: Arc<Mutex<HashSet<String>>>,
    pub granted_commands: Arc<Mutex<HashSet<String>>>,
    pub counters: Arc<Counters>,
    pub commands: Arc<Mutex<Vec<AdvertisedCommand>>>,
    pub agents: Arc<Mutex<AgentRoster>>,
    pub title_generation_in_flight: Arc<AtomicBool>,
}

/// A tool grant answers only that tool's exact approval. A command grant answers only a
/// simple shell invocation of the selected executable; shell operators still require approval.
/// Extension dialogs and changed approval shapes still reach the user.
fn auto_approval_frame(
    incoming: &ui::Incoming,
    granted_tools: &Mutex<HashSet<String>>,
    granted_commands: &Mutex<HashSet<String>>,
) -> Option<serde_json::Value> {
    let ui::Incoming::Request(request) = incoming else {
        return None;
    };
    if request.kind != ui::UiRequestKind::Select || request.options != ["Approve", "Deny"] {
        return None;
    }
    let tool = request
        .title
        .lines()
        .next()?
        .strip_prefix("Allow tool: ")?
        .trim();
    if crate::policy::validate_tool_name(tool).is_err() {
        return None;
    }

    let tool_granted = granted_tools.lock().ok()?.contains(tool);
    let command_granted = if matches!(tool, "bash" | "bash_interactive") {
        command_from_approval(&request.title)
            .and_then(simple_command_program)
            .is_some_and(|program| {
                granted_commands
                    .lock()
                    .is_ok_and(|set| set.contains(&program))
            })
    } else {
        false
    };
    if !tool_granted && !command_granted {
        return None;
    }
    UiResponse::Value("Approve".to_string()).frame(request.kind, &request.id)
}

/// The executable at the beginning of a command, normalized to its basename.
/// This is a display/grant identity only; it never executes or rewrites the command.
pub fn command_program(command: &str) -> Result<String, String> {
    let words = shell_words::split(command)
        .map_err(|_| "the command could not be parsed safely".to_string())?;
    let executable = words
        .first()
        .filter(|word| !word.is_empty())
        .ok_or_else(|| "the command has no executable".to_string())?;
    let program = Path::new(executable)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "the command executable has no readable name".to_string())?;
    crate::policy::validate_tool_name(program)?;
    Ok(program.to_string())
}

fn command_from_approval(title: &str) -> Option<&str> {
    title
        .lines()
        .skip(1)
        .find_map(|line| line.strip_prefix("Command:").map(str::trim))
        .filter(|command| !command.is_empty())
}

/// Reject shell syntax that can invoke another program or expand into a different call.
/// False negatives are intentional: commands with operators continue to prompt.
fn simple_command_program(command: &str) -> Option<String> {
    if command.chars().any(|character| {
        matches!(
            character,
            ';' | '|'
                | '&'
                | '<'
                | '>'
                | '$'
                | '`'
                | '('
                | ')'
                | '{'
                | '}'
                | '*'
                | '?'
                | '['
                | ']'
                | '\n'
                | '\r'
        )
    }) {
        return None;
    }
    command_program(command).ok()
}

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
                        let turn_finished = matches!(
                            &event,
                            SessionEvent::AgentEnd(end) if end.is_terminal()
                        );
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

                        if kinds.insert(event.kind().to_string()) {
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
                            println!("[omp-desktop] {} invalidated cached state", event.kind());
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
                            println!("[omp-desktop] event #{sequence} {}", event.kind());
                        }

                        reporting.activity(ActivitySnapshot {
                            kind: event.kind().to_string(),
                            sequence,
                        });

                        // Some fields are authoritative only in `get_state` — the
                        // engine's own message count, queue depth, context usage.
                        // Events carry changes, not totals, so the snapshot is
                        // re-read when a run settles or when an event says part of
                        // it went stale. Without this the pane shows a stale count
                        // beside a live transcript.
                        if stale || event.ends_run() {
                            // Logged and dropped: a missed refresh is a stale count
                            // beside a live transcript, not a reason to end the pump.
                            if let Err(error) = refresh_control(&client, &control).await {
                                eprintln!("[omp-desktop] could not refresh session state: {error}");
                            }
                        }

                        // Last, so the name and the last answer it reads are the ones the
                        // event just produced.
                        let facts = facts(&reporting.thread.current(), &control, &transcript);
                        if let Some(notification) = notifier.event(&event, &facts) {
                            reporting.sink.notifications(notification);
                        }

                        if turn_finished {
                            spawn_title_generation(
                                Arc::clone(&client),
                                Arc::clone(&control),
                                Arc::clone(&title_generation_in_flight),
                                reporting.clone(),
                            );
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
                    // Rows and the roster share the tick: a streaming turn emits deltas
                    // far faster than either a conversation view or a sidebar redraws.
                    let changed = first_dirty.is_some();
                    publish_rows(&reporting, &transcript, &mut first_dirty);
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
                                    if is_generated_title_receipt(&text)
                                        && title_generation_in_flight.swap(false, Ordering::AcqRel)
                                    {
                                        continue;
                                    }
                                    if let Ok(mut transcript) = transcript.lock() {
                                        transcript.apply_command_output(text);
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
                                let list = palette::commands_from(&frame);
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
                                // `/rename` persists the title and announces this frame, but the
                                // frame is only a hint; `get_state` is the authoritative session
                                // name. Re-read it and republish the live row so the sidebar does
                                // not wait for a process restart to see the generated title.
                                if let Err(error) = refresh_control(&client, &control).await {
                                    eprintln!(
                                        "[omp-desktop] could not refresh generated title: {error}"
                                    );
                                } else {
                                    reporting.sink.threads(reporting.roster.snapshots());
                                }
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
fn unusable_dialog(frame: &serde_json::Value) -> Option<String> {
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

/// What the host knows about a thread when a notification trigger fires.
///
/// Read under the two locks the pump already takes and returned owned: the emit happens
/// outside both, so a window's event loop is never inside the pump's critical section.
fn facts(thread: &str, control: &Mutex<SessionControl>, transcript: &Mutex<Transcript>) -> Facts {
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
fn last_answer(transcript: &Transcript) -> Option<String> {
    transcript.rows().iter().rev().find_map(|row| match row {
        omp_session::Row::Assistant { message, .. } => optional(message.text()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_switch_waits_for_running_queued_and_blocked_work() {
        let idle = SessionControl::decode(&serde_json::json!({
            "sessionId": "thread-1",
            "isStreaming": false,
            "isCompacting": false,
            "queuedMessageCount": 0
        }))
        .expect("control snapshot");
        assert!(mode_switch_idle(&idle, &idle, 0));

        let mut active = idle.clone();
        active.is_streaming = true;
        assert!(!mode_switch_idle(&active, &idle, 0));
        assert!(!mode_switch_idle(&idle, &active, 0));

        let mut queued = idle.clone();
        queued.queued_message_count = 1;
        assert!(!mode_switch_idle(&queued, &idle, 0));

        let mut compacting = idle.clone();
        compacting.is_compacting = true;
        assert!(!mode_switch_idle(&compacting, &idle, 0));
        assert!(!mode_switch_idle(&idle, &compacting, 0));
        assert!(!mode_switch_idle(&idle, &idle, 1));
    }

    #[test]
    fn a_live_grant_only_answers_the_matching_engine_approval() {
        let granted = Mutex::new(HashSet::from(["bash".to_string()]));
        let granted_commands = Mutex::new(HashSet::new());
        let approval = ui::Incoming::Request(ui::UiRequest {
            id: "ui_1".to_string(),
            kind: ui::UiRequestKind::Select,
            title: "Allow tool: bash\nCommand: echo hello".to_string(),
            message: String::new(),
            options: vec!["Approve".to_string(), "Deny".to_string()],
            prefill: None,
            placeholder: None,
            timeout_ms: None,
        });

        assert_eq!(
            auto_approval_frame(&approval, &granted, &granted_commands),
            Some(serde_json::json!({
                "type": "extension_ui_response", "id": "ui_1", "value": "Approve"
            }))
        );

        let ui::Incoming::Request(original) = approval else {
            unreachable!()
        };
        for altered in [
            ui::UiRequest {
                title: "Allow tool: write".to_string(),
                ..original.clone()
            },
            ui::UiRequest {
                options: vec!["Approve".to_string(), "Later".to_string()],
                ..original.clone()
            },
            ui::UiRequest {
                kind: ui::UiRequestKind::Confirm,
                ..original.clone()
            },
            ui::UiRequest {
                title: "Allow tool: bash/other".to_string(),
                ..original.clone()
            },
        ] {
            assert_eq!(
                auto_approval_frame(&ui::Incoming::Request(altered), &granted, &granted_commands),
                None
            );
        }
    }

    #[test]
    fn a_conversation_command_grant_only_answers_simple_calls_of_that_program() {
        let granted_tools = Mutex::new(HashSet::new());
        let granted_commands = Mutex::new(HashSet::from(["node".to_string()]));
        let approval = |command: &str| {
            ui::Incoming::Request(ui::UiRequest {
                id: "ui_1".to_string(),
                kind: ui::UiRequestKind::Select,
                title: format!("Allow tool: bash\nCommand: {command}"),
                message: String::new(),
                options: vec!["Approve".to_string(), "Deny".to_string()],
                prefill: None,
                placeholder: None,
                timeout_ms: None,
            })
        };

        assert!(auto_approval_frame(
            &approval("node check-index.js"),
            &granted_tools,
            &granted_commands,
        )
        .is_some());
        assert!(auto_approval_frame(
            &approval("/usr/bin/node check-index.js"),
            &granted_tools,
            &granted_commands,
        )
        .is_some());
        for command in [
            "npm test",
            "node check-index.js; rm -rf /tmp/probe",
            "node $(touch /tmp/probe)",
            "node check-index.js | tee output.log",
            "NODE_OPTIONS=--inspect node check-index.js",
        ] {
            assert_eq!(
                auto_approval_frame(&approval(command), &granted_tools, &granted_commands),
                None,
                "compound or different command must still ask: {command}"
            );
        }
    }

    #[test]
    fn a_command_grant_uses_the_direct_executable_name() {
        assert_eq!(
            command_program("node check-index.js; echo done").unwrap(),
            "node"
        );
        assert_eq!(
            command_program("/usr/bin/node check-index.js").unwrap(),
            "node"
        );
        assert!(command_program("NODE_OPTIONS=--inspect node check-index.js").is_err());
        assert_eq!(
            simple_command_program("node check-index.js").as_deref(),
            Some("node")
        );
        assert_eq!(simple_command_program("node a.js; echo done"), None);
    }

    #[test]
    fn automatic_title_receipts_are_hidden_but_other_command_output_is_not() {
        assert!(is_generated_title_receipt(
            "Could not generate a session title. Use /rename <title> to set one."
        ));
        assert!(is_generated_title_receipt(
            "Session renamed to Short essay on Roman Empire."
        ));
        assert!(!is_generated_title_receipt("Compacted 12 messages."));
    }

    /// The three answers the pump's silent `continue` used to conflate: a frame the host must
    /// answer but cannot read is reported, chrome is passed on to the chrome stream, and a
    /// well-formed dialog is neither. The middle one is why this is a function rather than a
    /// condition inside the pump: only the *blocking* four are the host's obligation, and the
    /// engine will wait on one until its own deadline.
    #[test]
    fn a_blocking_dialog_the_decoder_cannot_read_is_named_and_the_rest_are_not() {
        let unreadable = serde_json::json!({
            "type": "extension_ui_request",
            "method": "select",
            "title": "Allow tool: bash",
        });
        let named = unusable_dialog(&unreadable).expect("an unanswerable dialog is reported");
        assert!(
            named.contains("select") && named.contains("none"),
            "the method and the id it carried are the whole diagnosis: {named}"
        );

        let chrome = serde_json::json!({
            "type": "extension_ui_request",
            "id": "ui_1",
            "method": "notify",
            "message": "hi",
        });
        assert_eq!(unusable_dialog(&chrome), None, "chrome is not a dialog");

        let readable = serde_json::json!({
            "type": "extension_ui_request",
            "id": "ui_2",
            "method": "select",
            "title": "Allow tool: bash",
        });
        assert_eq!(
            unusable_dialog(&readable),
            None,
            "a decoded dialog is answered"
        );
        assert_eq!(
            unusable_dialog(&serde_json::json!({ "type": "agent_end" })),
            None
        );
    }

    /// The row a `user` message becomes, and the one thing on it that is not text.
    ///
    /// The mapping is what the window consumes, and an attachment lives or dies
    /// here: the model layer carries the bytes (`Message::images`) and the
    /// frontend renders `attachments`, so a field dropped in between is a
    /// thumbnail that never appears with nothing failing. The live path cannot
    /// reach this yet — the composer has no attach affordance until step 8 — so it
    /// is asserted directly.
    #[test]
    fn a_user_message_carries_its_attachments_to_the_frontend() {
        let message = omp_session::Message::decode(&serde_json::json!({
            "role": "user",
            "content": [
                { "type": "text", "text": "what is this" },
                { "type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png" },
            ],
            "timestamp": 1,
        }));

        let row = row_snapshot(&omp_session::Row::Message(message));

        assert_eq!(row.role, "user");
        assert_eq!(row.timestamp, Some(1));
        assert_eq!(row.text, "what is this");
        assert_eq!(row.attachments.len(), 1);
        assert_eq!(row.attachments[0].mime_type, "image/png");
        assert_eq!(row.attachments[0].data, "iVBORw0KGgo=");
    }

    /// A finished card's `details` is where a per-tool renderer finds its diff
    /// (`docs/12` §3.2), so it has to survive the mapping.
    #[test]
    fn a_finished_card_carries_its_details_to_the_frontend() {
        let mut transcript = omp_session::Transcript::new();
        transcript.apply(&omp_transport::events::SessionEvent::decode(
            &serde_json::json!({
                "type": "tool_execution_end",
                "toolCallId": "c1",
                "toolName": "edit",
                "isError": false,
                "result": {
                    "content": [{ "type": "text", "text": "Edited f.ts" }],
                    "details": { "diff": "--- a/f.ts\n+++ b/f.ts\n" },
                },
            }),
        ));

        let omp_session::Row::Tool(card) = &transcript.rows()[0] else {
            panic!("expected a tool card");
        };
        let row = row_snapshot(&omp_session::Row::Tool(card.clone()));
        let tool = row.tool.expect("a tool row carries its card");

        assert!(tool.finished);
        assert!(!tool.is_error);
        assert_eq!(tool.output, "Edited f.ts");
        assert!(
            tool.details.contains("--- a/f.ts"),
            "the diff must reach the frontend, got {:?}",
            tool.details
        );
    }

    /// A call still running has no result, and a card must not read that as an
    /// error or as empty output it should hide.
    #[test]
    fn a_running_card_has_no_details_and_is_not_an_error() {
        let mut transcript = omp_session::Transcript::new();
        transcript.apply(&omp_transport::events::SessionEvent::decode(
            &serde_json::json!({
                "type": "tool_execution_start",
                "toolCallId": "c1",
                "toolName": "bash",
                "args": { "command": "sleep 1" },
            }),
        ));

        let omp_session::Row::Tool(card) = &transcript.rows()[0] else {
            panic!("expected a tool card");
        };
        let row = row_snapshot(&omp_session::Row::Tool(card.clone()));
        let tool = row.tool.expect("a tool row carries its card");

        assert!(!tool.finished);
        assert!(!tool.is_error);
        assert!(tool.details.is_empty());
        assert!(tool.output.is_empty());
        assert!(tool.args.contains("sleep 1"));
    }

    /// A failed turn is what the sidebar's red dot reports (`docs/12` §2.2), and it is
    /// **not** a notice: the engine marks the assistant message itself. A thread that
    /// only looked for error-level notices would show a healthy row over a turn that
    /// never produced an answer — invisible until a real turn fails, which is why the
    /// mapping is asserted here rather than left to a live run.
    #[test]
    fn a_failed_turn_becomes_the_threads_error_and_a_healthy_one_does_not() {
        let mut transcript = omp_session::Transcript::new();
        transcript.apply(&omp_transport::events::SessionEvent::decode(
            &serde_json::json!({
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [],
                    "stopReason": "error",
                    "errorMessage": "429 rate limited",
                    "timestamp": 7,
                },
            }),
        ));

        assert_eq!(
            last_failure(&transcript).as_deref(),
            Some("429 rate limited")
        );

        // A retry that gave up is the other shape, and it is a notice.
        transcript.apply(&omp_transport::events::SessionEvent::decode(
            &serde_json::json!({
                "type": "auto_retry_end",
                "success": false,
                "attempt": 3,
                "finalError": "connection reset",
            }),
        ));

        assert_eq!(
            last_failure(&transcript).as_deref(),
            Some("connection reset")
        );

        // A turn that simply answered carries no failure, however many rows it has.
        let mut healthy = omp_session::Transcript::new();
        healthy.apply(&omp_transport::events::SessionEvent::decode(
            &serde_json::json!({
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "content": [{ "type": "text", "text": "pong" }],
                    "stopReason": "stop",
                    "timestamp": 8,
                },
            }),
        ));

        assert_eq!(last_failure(&healthy), None);
    }
}
