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
    /// The thread's event tail: the newest kind, tagged with its thread.
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
pub(crate) fn tagged<T>(thread: &str, payload: T) -> ThreadEvent<T> {
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
