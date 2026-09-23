//! The registry of live threads: every session the app is holding at once.
//!
//! One thread is one `omp --mode rpc-ui` sidecar (`docs/11` D5), and a thread's id is
//! the **engine's** session id rather than one the app mints. That is the whole
//! point: the on-disk catalogue lists sessions by that id and a resume resolves it
//! back to a session file, so the sidebar, the registry and the engine's own resume
//! picker are all talking about the same thing without a mapping table in between.
//!
//! The lock never spans an await. It covers a `HashMap` lookup, an insert or a swap,
//! and nothing else; every caller that wants to act on a thread clones its `Arc` out
//! first and only then awaits. That is why this is a registry with no methods that
//! spawn, answer or shut anything down: closing a sidecar is an await of up to
//! `SHUTDOWN_GRACE`, and holding the map across it would block every other thread's
//! command for as long as one engine takes to exit.
//!
//! A thread that *leaves* is remembered ([`Threads::cached_control`]): the right panel
//! (`docs/12` §8) can still be looking at one whose sidecar has gone, and its last
//! control answers the questions that have no engine left to ask — the thread's plan, and
//! where its session file is. The agents panel asks the same kind of question about the
//! same kind of thread ("what was it running?"), so a closed thread's last roster is kept
//! beside its control rather than in a second cache with its own eviction.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use omp_session::SessionControl;

use crate::dto::ThreadSnapshot;
use omp_session::Subagent;

use crate::session::{ActivitySink, LiveSession, Roster};

/// How many closed threads' last controls the registry keeps.
///
/// The point of the cache is the panel that is still looking at a thread whose sidecar has
/// gone (a close, an idle suspension), and that is one thread at a time; 64 is room for the
/// handful a session of clicking through the sidebar leaves behind without letting a
/// long-lived process accumulate a control per session it ever opened.
const REMEMBERED: usize = 64;

/// Every live thread, keyed by the engine's session id.
#[derive(Default)]
pub struct Threads {
    live: Mutex<HashMap<String, Arc<LiveSession>>>,
    /// The last control each thread that has left the registry was read at.
    ///
    /// Kept because "what was this thread's plan?" outlives the sidecar: the panel goes
    /// read-only when a thread is not live, not blank, and the answer costs one snapshot
    /// here against a re-read of the session file that would still not carry it.
    closed: Mutex<Closed>,
    /// The ids the app has released to give their processes back (`docs/11` D5).
    ///
    /// App-owned state, and the one thing that separates "no sidecar because nobody opened
    /// it" from "no sidecar because we stopped it": the catalogue's row has to say which
    /// (`dto::SessionSummaryDto.suspended`), or a released thread is drawn as a live one
    /// (`docs/12` §16). It lives here rather than in the session because the session is gone
    /// by the time it matters.
    suspended: Mutex<HashSet<String>>,
    /// The thread the window is showing, when its surface has said so.
    ///
    /// The idle policy's third guard, and the only one this process cannot derive: *which*
    /// thread a person is looking at is a fact about the window, so it arrives through a
    /// command and is kept here — the registry is what both the supervisor and the command
    /// already hold (`crate::idle`).
    focused: Mutex<Option<String>>,
}

/// What a thread that is no longer live knew.
#[derive(Clone)]
struct ClosedThread {
    control: SessionControl,
    /// The subagents it had when it left (`docs/12` §9).
    ///
    /// Kept for the same reason as the control: a thread whose sidecar is gone can no
    /// longer be asked, and an agents panel that showed nothing for it would read as "this
    /// thread never ran an agent" — a different claim from "no engine is left to ask".
    agents: Vec<Subagent>,
}

/// What threads that are no longer live knew, oldest first.
#[derive(Default)]
struct Closed {
    threads: HashMap<String, ClosedThread>,
    order: VecDeque<String>,
}

impl Closed {
    /// Remember what a thread knew, evicting the oldest when the cache is full.
    fn remember(&mut self, id: &str, thread: ClosedThread) {
        if self.threads.insert(id.to_string(), thread).is_none() {
            self.order.push_back(id.to_string());
        }

        while self.order.len() > REMEMBERED {
            if let Some(oldest) = self.order.pop_front() {
                self.threads.remove(&oldest);
            }
        }
    }

    fn get(&self, id: &str) -> Option<ClosedThread> {
        self.threads.get(id).cloned()
    }
}

impl Threads {
    /// The thread with this id, when one is live.
    pub fn get(&self, id: &str) -> Option<Arc<LiveSession>> {
        self.live.lock().ok()?.get(id).cloned()
    }

    /// The control a thread was last read at, when it is not live any more.
    ///
    /// A live thread answers [`Threads::get`] instead, deliberately: this is the fallback
    /// for the questions that have no engine left to ask (the plan, the session file), and
    /// a caller that preferred a remembered control over a live one would be reading a
    /// snapshot the engine has already moved past.
    pub fn cached_control(&self, id: &str) -> Option<SessionControl> {
        self.closed
            .lock()
            .ok()?
            .get(id)
            .map(|thread| thread.control)
    }

    /// The agents a thread had when it left, when nothing live answers for it.
    ///
    /// Live threads answer through their own session instead ([`LiveSession::agents`]),
    /// because their roster is still moving; this is the frozen answer for the ones whose
    /// engine has gone.
    pub fn cached_agents(&self, id: &str) -> Option<Vec<Subagent>> {
        self.closed.lock().ok()?.get(id).map(|thread| thread.agents)
    }

    /// Every thread the cache is holding, oldest first — the agents panel's rows for
    /// threads that have left.
    pub fn closed_ids(&self) -> Vec<String> {
        self.closed
            .lock()
            .map(|closed| closed.order.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Register a session under its own id ([`LiveSession::thread`]).
    ///
    /// An id that is already live is replaced, which is why the caller that resumes one
    /// session twice is responsible for the session it displaces — see
    /// [`crate::bridge::open_thread`], which shuts the old sidecar down rather than
    /// dropping the handle to it.
    pub fn insert(&self, session: Arc<LiveSession>) {
        let id = session.thread.current();

        if let Ok(mut live) = self.live.lock() {
            live.insert(id.clone(), session);
        }

        // A thread that is live is not suspended, whatever it was a moment ago: this is the
        // half of the resume path that has to exist, because a released thread's id would
        // otherwise keep its row marked as released for the rest of the process's life.
        self.clear_suspension(&id);
    }

    /// Re-file a live thread under the id the engine just gave it, dropping `from`.
    ///
    /// `branch` is the command behind this: it mints a new session **inside the same
    /// sidecar** (measured — the response is `{text, cancelled}` and carries no
    /// identity), so the session that was keyed by the old id is the same object the new
    /// id belongs to. Both keys are moved under one lock, deliberately: a registry
    /// holding the thread under two ids draws the sidebar's row twice, and one holding it
    /// under neither is a live sidecar nothing can address.
    ///
    /// The id that stops existing is remembered rather than forgotten: a window still
    /// showing the thread that was branched from asks for its plan by the old id, and the
    /// plan the engine held a moment ago is a truer answer than "no such thread".
    pub fn refile(&self, from: &str, session: Arc<LiveSession>) {
        let Ok(mut live) = self.live.lock() else {
            return;
        };

        if let Some(previous) = live.remove(from) {
            self.remember(from, &previous);
        }
        live.insert(session.thread.current(), session);
    }

    /// Take a thread out, so nothing can address it while it is being closed.
    ///
    /// What the thread knew when it left is kept: see [`Threads::cached_control`].
    pub fn remove(&self, id: &str) -> Option<Arc<LiveSession>> {
        let session = self.live.lock().ok()?.remove(id)?;
        self.remember(id, &session);
        // A thread that leaves the registry is not suspended any more, whichever way it left
        // — a close, a delete, a restart into a new sidecar. Without this the mark would
        // outlive the reason for it, and only ever be cleared by the resume path; the idle
        // supervisor re-marks the ids it releases itself, immediately after this returns.
        self.clear_suspension(id);

        Some(session)
    }

    /// Remove only the sidecar a caller inspected before an async wait.
    /// A mode switch must not stop a different session that took the same id
    /// while it was waiting for the old one's turn to finish.
    pub fn remove_if_same(
        &self,
        id: &str,
        expected: &Arc<LiveSession>,
    ) -> Option<Arc<LiveSession>> {
        let session = {
            let mut live = self.live.lock().ok()?;
            if !Arc::ptr_eq(live.get(id)?, expected) {
                return None;
            }
            live.remove(id)?
        };
        self.remember(id, &session);
        self.clear_suspension(id);
        Some(session)
    }

    /// Mark a thread as released: the app stopped its sidecar to give the process back.
    ///
    /// Called by the supervisor *after* the thread has left the registry and its engine has
    /// exited, so the mark only ever describes a process that is already gone.
    pub fn suspend(&self, id: &str) {
        if let Ok(mut suspended) = self.suspended.lock() {
            suspended.insert(id.to_string());
        }
    }

    /// Whether this id's sidecar was released by the app for being idle.
    ///
    /// Infallible: a poisoned set answers `false`, which is the reading that shows no
    /// released badge rather than hiding a live thread's row.
    pub fn is_suspended(&self, id: &str) -> bool {
        self.suspended
            .lock()
            .map(|suspended| suspended.contains(id))
            .unwrap_or(false)
    }

    /// The id the window last said it was showing, if any.
    pub fn focused(&self) -> Option<String> {
        self.focused.lock().ok().and_then(|focused| focused.clone())
    }

    /// Record which thread the window is showing.
    ///
    /// `None` for "no thread in particular" — a window that closed the last column, or a
    /// surface that has no focus concept — which is the same as never having said: nothing
    /// is excluded from the idle policy, and the other three guards still hold.
    pub fn focus(&self, id: Option<String>) {
        if let Ok(mut focused) = self.focused.lock() {
            *focused = id;
        }
    }

    /// Stop calling `id` suspended: it is live again, or its file is gone.
    ///
    /// Called by everything that ends a release — the resume path, a delete, and this
    /// registry's own `insert`/`remove`, so a mark cannot outlive the state it describes.
    pub fn clear_suspension(&self, id: &str) {
        if let Ok(mut suspended) = self.suspended.lock() {
            suspended.remove(id);
        }
    }

    /// Keep the control this session was last read at, for the id it is leaving under.
    fn remember(&self, id: &str, session: &LiveSession) {
        // A poisoned or contended cache is not worth failing a close over: the panel falls
        // back to the catalogue, which answers the session-file questions but not the
        // plan, so this is a degraded answer rather than a broken one.
        let Ok(control) = session.control.lock().map(|control| control.clone()) else {
            return;
        };
        let agents = session
            .agents
            .lock()
            .map(|roster| roster.agents().to_vec())
            .unwrap_or_default();

        if let Ok(mut closed) = self.closed.lock() {
            closed.remember(id, ClosedThread { control, agents });
        }
    }

    /// Every live thread, in a stable order (by session id).
    ///
    /// The agents panel needs the *set* rather than one id: its rows are the threads that
    /// have agents, and asking for them one at a time would be a command per thread for a
    /// question that is one list.
    pub fn live(&self) -> Vec<Arc<LiveSession>> {
        let Ok(live) = self.live.lock() else {
            return Vec::new();
        };
        let mut sessions: Vec<Arc<LiveSession>> = live.values().cloned().collect();
        sessions.sort_by_key(|session| session.thread.current());

        sessions
    }

    /// Take every thread out, for a shutdown that must leave nothing running.
    ///
    /// Returned rather than dropped: the caller is the one that has to stop each
    /// sidecar, and an `Arc` dropped here would leave the process behind.
    pub fn drain(&self) -> Vec<Arc<LiveSession>> {
        match self.live.lock() {
            Ok(mut live) => live.drain().map(|(_, session)| session).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Any live thread, for a question that is about the engine rather than about one
    /// conversation — the model catalogue is the one such command (`docs/12` §7.1).
    ///
    /// Which thread is deliberately unspecified: the rows are the engine build's and its
    /// provider configuration's, so any of them answers the same. The registry happens to
    /// hold them in a hash map, and "the first one" is not a promise this makes.
    pub fn any(&self) -> Option<Arc<LiveSession>> {
        self.live.lock().ok()?.values().next().cloned()
    }

    /// Every live thread's row, sorted by id (`docs/12` §2.2).
    ///
    /// The `Arc`s are cloned out and the lock dropped **before** a snapshot is taken: a
    /// snapshot reads a session's control state, transcript and dialogs, so deriving it
    /// under this lock would make every thread's row wait on every other thread's state.
    pub fn snapshots(&self) -> Vec<ThreadSnapshot> {
        let sessions: Vec<Arc<LiveSession>> = match self.live.lock() {
            Ok(live) => live.values().cloned().collect(),
            // A poisoned registry means a thread panicked while registering. An empty
            // roster is a worse answer than the truth, but it is an answer, and the
            // sidebar keeps drawing the other half of itself.
            Err(_) => Vec::new(),
        };

        // Sorted after the snapshot rather than before it: the id a row carries is the
        // thread's *current* one, which a fork can have moved, and the sort and the rows
        // must never disagree about which session a row is.
        let mut rows: Vec<ThreadSnapshot> =
            sessions.iter().map(|session| session.snapshot()).collect();
        rows.sort_by(|left, right| left.id.cmp(&right.id));

        rows
    }

    /// Announce the roster on the sink's `threads-updated` event.
    ///
    /// The registry is what knows the *set*, so it is what publishes: an open or a close
    /// cannot be seen by any single thread's pump, and the two must not both be trying to
    /// describe the same list.
    pub fn publish(&self, sink: &impl ActivitySink) {
        sink.threads(self.snapshots());
    }
}

/// The pump asks for the roster through this: it holds a registry and republishes it on
/// its own tick, without knowing what a registry is or that there is more than one.
impl Roster for Threads {
    fn snapshots(&self) -> Vec<ThreadSnapshot> {
        Threads::snapshots(self)
    }

    fn retire(&self, thread: &str) {
        // The sidecar is already gone by the time the pump says so, so the handle this drops
        // is a dead one: the registry's half is what has to move, because a row for a thread
        // with no process is a command that can never be answered.
        self.remove(thread);
    }
}
