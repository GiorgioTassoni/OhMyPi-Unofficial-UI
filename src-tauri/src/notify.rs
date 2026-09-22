//! What the host tells the window about, and when.
//!
//! Two jobs, and they are the same job seen from two sides:
//!
//! * the **triggers** — the four facts `docs/12` §13 names, folded out of the session's own
//!   event stream and the broker's process list, each firing once per real occurrence;
//! * the **body** of what is said — the engine's own words, cut to one line, never a summary.
//!
//! The rules are a value ([`Notifier`]) fed one event at a time rather than a closure inside
//! the pump, because "fires once" is a property of a *sequence*: a dialog replayed on resume
//! must not announce itself twice, and a failure that a retry already reported must not be
//! reported again by the `agent_end` that follows it. That is testable without a sidecar and
//! is not testable inside a `select!` arm.
//!
//! What this module deliberately does **not** decide: whether a person should be interrupted.
//! There are no focus rules and no preferences here — the window knows what is on screen, and
//! the host reports facts (`docs/12` §13).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use omp_session::SessionControl;
use omp_transport::protocol::ui::UiRequest;
use omp_transport::SessionEvent;
use tauri::async_runtime::JoinHandle;

use crate::agents;
use crate::dto::{BrokerDaemon, BrokerScope, NotificationEvent};
use crate::session::{ActivitySink, LiveSession};
use crate::threads::Threads;

/// A turn finished and nothing is wrong.
pub const KIND_TURN_FINISHED: &str = "turn-finished";

/// The engine is blocked on a dialog.
pub const KIND_NEEDS_YOU: &str = "needs-you";

/// The turn failed, or a retry sequence gave up.
pub const KIND_FAILED: &str = "failed";

/// A background job left the broker's running list.
pub const KIND_JOB_FINISHED: &str = "job-finished";

/// How long a notification body may be.
///
/// An OS banner is one line, and the engine's own sentences are sometimes a whole tool
/// result. Cut rather than summarised, on a character boundary: the point of the body is to
/// carry the engine's words, and paraphrasing them is the one thing this module must not do.
const MAX_BODY_CHARS: usize = 240;

/// What the host knows about a thread the moment a trigger fires.
///
/// Owned rather than borrowed: the pump builds this from two locks it has to drop before it
/// publishes, and holding a transcript lock across an event emit would put a window's event
/// loop inside the pump's critical section.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// The engine's session id — the notification's `thread`.
    pub thread: String,
    /// The session's own name, when it has one.
    pub name: Option<String>,
    /// The conversation's most recent failure, as the transcript recorded it.
    pub failure: Option<String>,
    /// The engine's last assistant text.
    pub answer: Option<String>,
}

/// The trigger rules, as one value per session.
#[derive(Debug, Default)]
pub struct Notifier {
    /// Dialog ids already announced.
    ///
    /// The engine replays a pending dialog when a session is resumed or reconnected, and the
    /// reducer keeps the id it already had (`UiRequests::apply` replaces by id) — so "a
    /// blocking dialog arrives" is not the same as "a blocking dialog is new", and the
    /// difference is exactly what this set holds.
    announced: HashSet<String>,
    /// The failure already reported, so a retry that gave up is not reported twice — once
    /// when `auto_retry_end` said so and again by the `agent_end` that follows it.
    failure: Option<String>,
}

impl Notifier {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one streamed event in, returning the notification it earns.
    ///
    /// The completion rule is the transport's own ([`SessionEvent::ends_run`]: a terminal
    /// `agent_end`, with an absent `isTerminal` read as terminal) and is deliberately not
    /// restated here — it is the same rule the reducer uses to clear `is_streaming`, so a
    /// second spelling of it would be a second answer to "has the turn finished".
    pub fn event(&mut self, event: &SessionEvent, facts: &Facts) -> Option<NotificationEvent> {
        if event.ends_run() {
            return match &facts.failure {
                // A turn that ended *in a failure* is not a finished turn with a footnote: the
                // engine marked the message, and the sentence is the reason.
                Some(error) if self.failure.as_deref() == Some(error.as_str()) => None,
                Some(error) => {
                    self.failure = Some(error.clone());
                    Some(failed(facts, error))
                }
                None => Some(finished(facts)),
            };
        }

        match event {
            // A retry sequence that gave up: the engine's own `finalError`, or the notice the
            // reducer just wrote (which is that error, or the reducer's words for a sequence
            // that ended without one).
            SessionEvent::AutoRetryEnd(end) if !end.success => {
                let error = facts
                    .failure
                    .clone()
                    .or_else(|| end.final_error.clone())
                    .unwrap_or_default();
                self.failure = Some(error.clone());
                Some(failed(facts, &error))
            }
            _ => None,
        }
    }

    /// A blocking dialog arrived, returning the notification the *first* arrival earns.
    ///
    /// Once per id: the engine re-publishes its pending set on resume and reconnects, and a
    /// banner per re-publication would be the same question asked over and over.
    pub fn dialog(&mut self, request: &UiRequest, facts: &Facts) -> Option<NotificationEvent> {
        if !self.announced.insert(request.id.clone()) {
            return None;
        }

        // The engine formats an approval as a title — `Allow tool: bash\nCommand: …`
        // (`docs/02` §7) — and a `confirm` puts its body in `message`, so the two fields are
        // read in that order rather than inventing a third line of our own.
        let prompt = if request.title.trim().is_empty() {
            request.message.clone()
        } else {
            request.title.clone()
        };

        Some(NotificationEvent {
            thread: facts.thread.clone(),
            kind: KIND_NEEDS_YOU.to_string(),
            title: title(facts),
            body: clamp(&prompt),
            job_id: None,
        })
    }

    /// A failure the host itself saw, as `failed`.
    ///
    /// The engine cannot send this one: a sidecar that dies mid-turn leaves nothing behind
    /// but an ended stream, and the pump is the only thing in the app that sees it.
    pub fn host_failure(&self, facts: &Facts, error: &str) -> NotificationEvent {
        failed(facts, error)
    }

    /// A job left the broker's running list.
    ///
    /// Attribution is the engine's own (`owner` is the session that started the daemon, which
    /// is what the agents panel joins on); when it recorded none, the thread is left empty
    /// rather than guessed at from a working directory that several sessions can share.
    pub fn job(&self, job: &DepartedJob, name: Option<&str>) -> NotificationEvent {
        let owner = job.owner.clone().unwrap_or_default();
        let status = if job.state.is_empty() {
            job.command.clone()
        } else if job.command.is_empty() {
            job.state.clone()
        } else {
            format!("{}: {}", job.state, job.command)
        };

        NotificationEvent {
            thread: owner,
            kind: KIND_JOB_FINISHED.to_string(),
            title: name
                .map(ToString::to_string)
                .unwrap_or_else(|| job.name.clone()),
            body: clamp(&status),
            job_id: Some(job.name.clone()),
        }
    }
}

/// A turn that finished with nothing wrong: the session's name and the engine's last words.
fn finished(facts: &Facts) -> NotificationEvent {
    NotificationEvent {
        thread: facts.thread.clone(),
        kind: KIND_TURN_FINISHED.to_string(),
        title: title(facts),
        body: clamp(facts.answer.as_deref().unwrap_or_default()),
        job_id: None,
    }
}

/// A turn that failed, or a retry that gave up.
fn failed(facts: &Facts, error: &str) -> NotificationEvent {
    NotificationEvent {
        thread: facts.thread.clone(),
        kind: KIND_FAILED.to_string(),
        title: title(facts),
        body: clamp(error),
        job_id: None,
    }
}

/// The session's own name, or its id when the engine never gave it one.
fn title(facts: &Facts) -> String {
    facts
        .name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| facts.thread.clone())
}

/// One line, at most.
fn clamp(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= MAX_BODY_CHARS {
        return trimmed.to_string();
    }

    let mut cut: String = trimmed.chars().take(MAX_BODY_CHARS).collect();
    cut.push('…');
    cut
}

/// One broker daemon, as the watcher last saw it while it was still running.
///
/// Held rather than re-read at the moment of departure, because the departure *is* the
/// absence: the fields a reader wants (what it was, how it ended) are only there to be read
/// while it is still listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepartedJob {
    /// The broker's own name for the daemon, which is what `omp ps stop` takes.
    pub name: String,
    /// The engine's lifecycle word: `exited` or `failed`.
    pub state: String,
    /// The command the broker launched, as the engine recorded it.
    pub command: String,
    /// The session that started it, when the engine recorded one.
    pub owner: Option<String>,
}

impl DepartedJob {
    fn of(daemon: &BrokerDaemon) -> Self {
        Self {
            name: daemon.name.clone(),
            state: daemon.state.clone(),
            command: daemon.command.clone(),
            owner: daemon.owner.clone().filter(|owner| !owner.is_empty()),
        }
    }
}

/// A daemon's identity in the broker's list: its scope and its name.
///
/// Names are unique per scope, not per machine — two projects can both supervise a `vite` —
/// so the scope is part of the key.
fn key(scope: &str, daemon: &BrokerDaemon) -> String {
    format!("{scope}\u{0}{}", daemon.name)
}

/// Whether the engine still lists this daemon as running.
///
/// Measured from the engine's own vocabulary (`launch/protocol.ts:162-165` closes the set to
/// `starting | running | ready | restarting | stopping | exited | failed`, and
/// `launch/broker.ts:122` calls the last two terminal): a daemon that is not in a terminal
/// state is in the running list, and the transition out of it is what this watcher reports.
fn still_running(daemon: &BrokerDaemon) -> bool {
    !matches!(daemon.state.as_str(), "exited" | "failed")
}

/// Every daemon the broker currently lists as running, keyed and ready to be compared with
/// the next poll's list.
pub fn running_jobs(scopes: &[BrokerScope]) -> HashMap<String, DepartedJob> {
    let mut running = HashMap::new();

    for scope in scopes {
        for daemon in &scope.daemons {
            if still_running(daemon) {
                running.insert(key(&scope.project_dir, daemon), DepartedJob::of(daemon));
            }
        }
    }

    running
}

/// The jobs that were running at the previous poll and are not running now.
pub fn departures(
    previous: &HashMap<String, DepartedJob>,
    current: &HashMap<String, DepartedJob>,
) -> Vec<DepartedJob> {
    let mut left: Vec<DepartedJob> = previous
        .iter()
        .filter(|(key, _)| !current.contains_key(*key))
        .map(|(_, job)| job.clone())
        .collect();
    // Sorted, so two departures in one poll arrive in a stable order rather than in a hash
    // map's order.
    left.sort_by(|left, right| left.name.cmp(&right.name));

    left
}

/// How often the broker is asked while a job is known to be running.
const POLL_RUNNING: Duration = Duration::from_secs(2);

/// How often it is asked when none is.
///
/// **What this costs, and why it is acceptable.** The engine backgrounds a job on its own
/// initiative (a `bash` call past the auto-background threshold), so there is no frame that
/// says "a job now exists": the broker's own list is the only signal, and the only way to
/// notice a job *leaving* it is to ask again. At rest that is one short-lived
/// `omp ps --json --all` per minute, and not even that when no thread is open — a job's owner
/// has to be a live session for the notification to mean anything here. While a job runs it
/// is one ask every two seconds, because the alternative is telling the user about a finished
/// job a minute after they stopped caring. The one thing the idle cadence cannot see is a job
/// that both starts and finishes inside that minute; that is the price of not spawning an
/// engine process every two seconds forever, and it is paid deliberately.
const POLL_IDLE: Duration = Duration::from_secs(60);

/// Watch the engine's broker and report every job that leaves its running list.
///
/// A task rather than a poll inside a command because a departure has to be noticed while
/// nobody is asking: the whole point is that the window is elsewhere.
///
/// Spawned on Tauri's runtime for the reason [`crate::idle::spawn`] spells out: it starts from
/// `setup`, where a bare `tokio::spawn` panics with "there is no reactor running".
pub fn spawn_job_watcher(threads: Arc<Threads>, sink: Arc<dyn ActivitySink>) -> JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let notifier = Notifier::new();
        let mut running: HashMap<String, DepartedJob> = HashMap::new();

        loop {
            let live = threads.live();

            if live.is_empty() {
                // Nothing open can own a job, so there is nothing to compare and no reason to
                // spend a process finding that out. The map is dropped too: a job that was
                // running when the last thread closed would otherwise be reported as finished
                // the moment one opens again.
                running.clear();
                tokio::time::sleep(POLL_IDLE).await;
                continue;
            }

            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let scopes = match agents::broker_scopes(&cwd.to_string_lossy()).await {
                Ok(scopes) => scopes,
                // A broker that will not answer is not a reason to stop watching: it is
                // exactly the state `omp ps` exists to report, and the next poll gets the
                // list. The baseline is left alone rather than cleared, so a job that was
                // running before the miss and is gone after it is still reported.
                Err(error) => {
                    eprintln!("[omp-desktop] the broker could not be listed: {error}");
                    tokio::time::sleep(POLL_IDLE).await;
                    continue;
                }
            };

            let current = running_jobs(&scopes);

            for job in departures(&running, &current) {
                let name = job
                    .owner
                    .as_deref()
                    .and_then(|owner| threads.get(owner))
                    .and_then(|session| session_name(&session));

                sink.notifications(notifier.job(&job, name.as_deref()));
            }

            let delay = if current.is_empty() {
                POLL_IDLE
            } else {
                POLL_RUNNING
            };
            running = current;
            tokio::time::sleep(delay).await;
        }
    })
}

/// The name a session's own chrome carries, if it has one.
fn session_name(session: &LiveSession) -> Option<String> {
    let control: SessionControl = session.control.lock().ok()?.clone();

    control.session_name
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn facts_of(thread: &str, name: &str) -> Facts {
        Facts {
            thread: thread.to_string(),
            name: Some(name.to_string()),
            failure: None,
            answer: None,
        }
    }

    fn event(raw: serde_json::Value) -> SessionEvent {
        SessionEvent::decode(&raw)
    }

    fn dialog(id: &str, title: &str) -> UiRequest {
        UiRequest {
            id: id.to_string(),
            kind: omp_transport::protocol::ui::UiRequestKind::Select,
            title: title.to_string(),
            message: String::new(),
            options: vec!["Approve".to_string(), "Deny".to_string()],
            prefill: None,
            placeholder: None,
            timeout_ms: None,
        }
    }

    /// A turn that ends cleanly is reported once, with the engine's own last words as the
    /// body and the session's own name as the title.
    #[test]
    fn a_finished_turn_reports_once_with_the_engines_words() {
        let mut notifier = Notifier::new();
        let mut facts = facts_of("session-7", "Refactor the parser");
        facts.answer = Some("Done — 3 files changed.".to_string());

        let end = event(json!({ "type": "agent_end", "messages": [] }));
        let notification = notifier
            .event(&end, &facts)
            .expect("a terminal end reports");

        assert_eq!(notification.kind, KIND_TURN_FINISHED);
        assert_eq!(notification.thread, "session-7");
        assert_eq!(notification.title, "Refactor the parser");
        assert_eq!(notification.body, "Done — 3 files changed.");
        assert_eq!(notification.job_id, None);
    }

    /// The whole vocabulary over one synthetic sequence: a turn that runs, is asked for
    /// something, finishes, then a job that leaves the broker's list.
    #[test]
    fn one_sequence_produces_each_kind_once() {
        let mut notifier = Notifier::new();
        let mut facts = facts_of("session-7", "Ship the release");
        let mut kinds = Vec::new();

        for raw in [
            json!({ "type": "agent_start" }),
            json!({ "type": "turn_start" }),
            json!({ "type": "message_start", "message": { "role": "user", "content": [] } }),
        ] {
            assert_eq!(notifier.event(&event(raw), &facts), None);
        }

        if let Some(notification) = notifier.dialog(&dialog("ui_1", "Allow tool: bash"), &facts) {
            kinds.push(notification.kind);
        }
        if let Some(notification) = notifier.dialog(&dialog("ui_1", "Allow tool: bash"), &facts) {
            kinds.push(notification.kind);
        }
        if let Some(notification) = notifier.dialog(&dialog("ui_2", "Allow tool: write"), &facts) {
            kinds.push(notification.kind);
        }

        facts.answer = Some("Released v1.0.0.".to_string());
        if let Some(notification) = notifier.event(
            &event(json!({ "type": "agent_end", "messages": [] })),
            &facts,
        ) {
            kinds.push(notification.kind);
        }

        if let Some(notification) = notifier.event(
            &event(json!({ "type": "auto_retry_end", "success": false, "attempt": 2, "finalError": "socket hang up" })),
            &facts,
        ) {
            kinds.push(notification.kind);
        }

        assert_eq!(
            kinds,
            vec![
                KIND_NEEDS_YOU,
                KIND_NEEDS_YOU,
                KIND_TURN_FINISHED,
                KIND_FAILED
            ],
            "each real occurrence fires once, and a repeated dialog does not"
        );

        let departed = DepartedJob {
            name: "vite".to_string(),
            state: "exited".to_string(),
            command: "bun run dev".to_string(),
            owner: Some("session-7".to_string()),
        };
        assert_eq!(
            notifier.job(&departed, Some("Ship the release")).kind,
            KIND_JOB_FINISHED
        );
    }

    /// The other half of the completion rule: `isTerminal: false` means an async delivery
    /// scheduled more work, so the turn has *not* finished and nothing is reported.
    #[test]
    fn a_non_terminal_end_and_a_mid_turn_event_report_nothing() {
        let mut notifier = Notifier::new();
        let facts = facts_of("session-7", "Refactor the parser");

        assert_eq!(
            notifier.event(
                &event(json!({ "type": "agent_end", "isTerminal": false, "messages": [] })),
                &facts
            ),
            None
        );
        for raw in [
            json!({ "type": "agent_start" }),
            json!({ "type": "turn_end", "message": {}, "toolResults": [] }),
            json!({ "type": "auto_retry_end", "success": true, "attempt": 1 }),
        ] {
            assert_eq!(notifier.event(&event(raw), &facts), None);
        }
    }

    /// A failed turn is `failed`, and its body is the engine's own sentence rather than the
    /// transcript's rendering of it.
    #[test]
    fn a_failed_turn_reports_the_engines_sentence_and_a_retry_that_gave_up_reports_once() {
        let mut notifier = Notifier::new();
        let mut facts = facts_of("session-7", "Refactor the parser");
        facts.failure = Some("429 rate limited".to_string());

        let notification = notifier
            .event(
                &event(json!({ "type": "agent_end", "messages": [] })),
                &facts,
            )
            .expect("a failed turn reports");

        assert_eq!(notification.kind, KIND_FAILED);
        assert_eq!(notification.body, "429 rate limited");

        // The transcript keeps the failure as a row, so the next `agent_end` still sees it —
        // and must not report the same failure again.
        assert_eq!(
            notifier.event(
                &event(json!({ "type": "agent_end", "messages": [] })),
                &facts
            ),
            None
        );

        // A retry sequence that gave up reports on its own, before any `agent_end`.
        let mut retrying = Notifier::new();
        let mut facts = facts_of("session-7", "Refactor the parser");
        facts.failure = Some("connection reset".to_string());
        let notification = retrying
            .event(
                &event(json!({
                    "type": "auto_retry_end",
                    "success": false,
                    "attempt": 3,
                    "finalError": "connection reset",
                })),
                &facts,
            )
            .expect("an exhausted retry reports");

        assert_eq!(notification.kind, KIND_FAILED);
        assert_eq!(notification.body, "connection reset");
    }

    /// The dialog rule: once per id, and never on a re-publication of a set already seen.
    #[test]
    fn a_blocking_dialog_is_announced_once_per_id() {
        let mut notifier = Notifier::new();
        let facts = facts_of("session-7", "Deploy the app");

        let first = notifier
            .dialog(
                &dialog("ui_1", "Allow tool: bash\nCommand: rm -rf build"),
                &facts,
            )
            .expect("a new dialog needs you");
        assert_eq!(first.kind, KIND_NEEDS_YOU);
        assert_eq!(first.title, "Deploy the app");
        assert_eq!(first.body, "Allow tool: bash\nCommand: rm -rf build");

        // The engine replays its pending set on resume, and the reducer keeps the id: the
        // same dialog arriving again is the same question.
        assert_eq!(
            notifier.dialog(
                &dialog("ui_1", "Allow tool: bash\nCommand: rm -rf build"),
                &facts
            ),
            None
        );

        // A second dialog is a second question.
        assert!(notifier
            .dialog(&dialog("ui_2", "Allow tool: write"), &facts)
            .is_some());
    }

    /// A job's departure carries the broker's own name and the engine's own fields, and an
    /// unattributable one says so instead of picking a thread.
    #[test]
    fn a_departure_is_reported_with_its_owner_or_without_one() {
        let notifier = Notifier::new();

        let owned = DepartedJob {
            name: "vite".to_string(),
            state: "exited".to_string(),
            command: "bun run dev".to_string(),
            owner: Some("session-7".to_string()),
        };
        let notification = notifier.job(&owned, Some("Deploy the app"));
        assert_eq!(notification.kind, KIND_JOB_FINISHED);
        assert_eq!(notification.thread, "session-7");
        assert_eq!(notification.title, "Deploy the app");
        assert_eq!(notification.body, "exited: bun run dev");
        assert_eq!(notification.job_id.as_deref(), Some("vite"));

        let unowned = DepartedJob {
            owner: None,
            ..owned.clone()
        };
        let notification = notifier.job(&unowned, None);
        assert_eq!(
            notification.thread, "",
            "no owner means no thread, not a guessed one"
        );
        assert_eq!(notification.title, "vite");
    }

    /// The running-list rule against the engine's own vocabulary, and the diff that turns two
    /// polls into departures.
    #[test]
    fn only_a_daemon_leaving_the_running_list_is_a_departure() {
        let scope =
            |scopes: serde_json::Value| agents::decode_scopes(scopes.to_string().as_bytes());
        let before = scope(json!([{
            "kind": "project",
            "projectDir": "/projects/app",
            "daemons": [
                { "name": "vite", "state": "running", "command": "bun run dev", "owner": "s1" },
                { "name": "omp.lsp.mux", "state": "starting" },
            ],
        }]))
        .expect("the engine's own shape");

        assert_eq!(running_jobs(&before).len(), 2);

        // `vite` left; the mux moved into a terminal state; a daemon in another scope that was
        // already exited is not a departure.
        let after = scope(json!([
            {
                "kind": "project",
                "projectDir": "/projects/app",
                "daemons": [
                    { "name": "vite", "state": "exited", "command": "bun run dev", "owner": "s1", "exitCode": 143 },
                    { "name": "omp.lsp.mux", "state": "failed" },
                ],
            },
            {
                "kind": "project",
                "projectDir": "/projects/other",
                "daemons": [{ "name": "vite", "state": "exited", "command": "bun run dev" }],
            },
        ]))
        .expect("the engine's own shape");

        let left = departures(&running_jobs(&before), &running_jobs(&after));
        assert_eq!(left.len(), 2, "both running daemons left: {left:?}");
        assert_eq!(
            left.iter().map(|job| job.name.as_str()).collect::<Vec<_>>(),
            ["omp.lsp.mux", "vite"],
            "sorted, so two departures in one poll arrive in a stable order"
        );
        assert_eq!(left[1].owner.as_deref(), Some("s1"));
        assert_eq!(left[0].owner, None);
    }

    /// A body is the engine's text, cut — never a sentence of ours.
    #[test]
    fn a_long_body_is_cut_rather_than_summarised() {
        let mut notifier = Notifier::new();
        let mut facts = facts_of("session-7", "Refactor");
        facts.answer = Some("x".repeat(MAX_BODY_CHARS * 2));

        let body = notifier
            .event(
                &event(json!({ "type": "agent_end", "messages": [] })),
                &facts,
            )
            .expect("a terminal end reports")
            .body;

        assert_eq!(body.chars().count(), MAX_BODY_CHARS + 1);
        assert!(body.starts_with("xxxx"));
        assert!(body.ends_with('…'));
    }
}
