//! Idle suspension: giving a sidecar's process back while nobody is using it.
//!
//! One `omp --mode rpc-ui` child per open thread is a ~200 MB process running its own
//! discovery, MCP connections and LSP clients (`docs/11` §3.1), so a window with several
//! threads open is several of them. OMP resumes a session losslessly, so the app's answer is
//! to stop the ones nobody is using and start them again on demand through the cold-open path
//! that already exists — this module decides *when*, and nothing else about it changes.
//!
//! Two halves, deliberately split:
//!
//! * [`IdlePolicy`] is the decision — pure over a set of samples and an injected `now`, so
//!   every guard is asserted without waiting on a clock;
//! * [`spawn`] is the sampling loop, which reads each live session's fingerprint, asks the
//!   policy, and releases what it names through the same shutdown the app uses on quit.
//!
//! Nothing here learns about sessions from the transport's side: the fingerprint is read from
//! the host's own reducers (`SessionControl`, `Dialogs`), which is why this lives in the host.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::async_runtime::JoinHandle;

use crate::session::{ActivitySink, LiveSession};
use crate::threads::Threads;

/// The idle window, in seconds, unless the environment overrides it.
///
/// Ten minutes is the sign-off's number (`docs/12` §16). It is **not** a user-facing setting
/// in v1 — the settings screen renders the engine's own catalog, and an app-owned knob has no
/// place in it (`docs/12` §12) — so it lives here as one named constant plus the environment
/// variable below, which is what a test or a person debugging a release needs.
pub const DEFAULT_WINDOW_SECS: u64 = 600;

/// The environment variable that overrides the window, in seconds.
///
/// Read once at startup, and only by the process that asked for it: this is a developer's
/// lever, not a preference, which is why nothing persists it.
pub const WINDOW_ENV: &str = "OMP_DESKTOP_IDLE_SECS";

/// How often the live threads are sampled, by default.
///
/// Well under the window, so a release lands within fifteen seconds of the window elapsing —
/// and cheap enough to ignore: sampling copies a few scalars per thread and takes two locks
/// that are never held across an await.
pub const DEFAULT_SAMPLE_SECS: u64 = 15;

/// How long a sidecar gets to finish cleanly before it is killed, for the same reason
/// `bridge::SHUTDOWN_GRACE` exists: the engine flushes its session file on a graceful exit,
/// and a released thread is resumed from that file.
pub const DEFAULT_GRACE_SECS: u64 = 5;

/// Everything about one thread the policy decides on.
///
/// One value rather than four arguments because the guards and the clock belong together: the
/// first three fields *are* the fingerprint (a change in any of them restarts the clock), and
/// only `focused` is a separate kind of fact — a statement about the window rather than about
/// the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// The engine's own message count (`get_state.messageCount`).
    pub messages: u64,
    /// Messages the user queued for the next turn.
    pub queued: u64,
    /// The ids of the dialogs the engine is waiting on, in the order it asked.
    ///
    /// Ids rather than a count: one approval answered as another arrives is a change, and a
    /// count would read it as none.
    pub dialogs: Vec<String>,
    /// A turn is in flight.
    pub streaming: bool,
}

impl Fingerprint {
    /// Whether a thread in this state may be released at all.
    ///
    /// The three guards from `docs/11` §3.1, as one question: a streaming turn is work in
    /// progress, a pending dialog is the engine *blocked* on an answer (an unanswered
    /// approval is a hung agent, and killing it would be destroying the question), and a
    /// queued message is a prompt the user has already typed.
    pub fn releasable(&self) -> bool {
        !self.streaming && self.queued == 0 && self.dialogs.is_empty()
    }
}

/// One thread, as one sampling instant saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    pub thread: String,
    pub fingerprint: Fingerprint,
    /// The window is showing this thread, so it is never idle.
    pub focused: bool,
}

/// The policy: one clock per thread, over the samples it is given.
///
/// A clock records the fingerprint it last saw and when it first saw it. A sample that
/// differs from the recorded one restarts the clock, which is what makes "unchanged for the
/// window" mean *unchanged* rather than "sampled at least twice".
#[derive(Debug, Default)]
pub struct IdlePolicy {
    clocks: HashMap<String, Clock>,
}

#[derive(Debug, Clone)]
struct Clock {
    fingerprint: Fingerprint,
    since: Instant,
}

impl IdlePolicy {
    /// Fold one round of samples in, and name the threads to release.
    ///
    /// `now` is a parameter rather than a call to `Instant::now()` so the rule can be
    /// asserted at any point on the timeline; the caller owns the real clock.
    ///
    /// A thread seen for the first time is never released in the same round: its clock starts
    /// at `now`, so the elapsed time is zero and no window — however short — has passed. That
    /// is the whole reason the clock exists (`docs/11` §3.1: nothing is suspended the moment
    /// it opens).
    pub fn review(&mut self, now: Instant, window: Duration, samples: &[Sample]) -> Vec<String> {
        // A thread that is no longer live has no clock: keeping one would let a released
        // thread's id be re-released by a stale reading when a session with the same id is
        // opened again.
        self.clocks
            .retain(|thread, _| samples.iter().any(|sample| &sample.thread == thread));

        let mut release = Vec::new();

        for sample in samples {
            match self.clocks.get_mut(&sample.thread) {
                Some(clock) if clock.fingerprint == sample.fingerprint => {
                    if sample.focused || !sample.fingerprint.releasable() {
                        continue;
                    }
                    if now.duration_since(clock.since) >= window {
                        release.push(sample.thread.clone());
                    }
                }
                _ => {
                    self.clocks.insert(
                        sample.thread.clone(),
                        Clock {
                            fingerprint: sample.fingerprint.clone(),
                            since: now,
                        },
                    );
                }
            }
        }

        release.sort();
        release
    }
}

/// What the supervisor needs to know: how long, how often, and how patient.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub window: Duration,
    pub interval: Duration,
    pub grace: Duration,
}

impl Config {
    /// The app's own settings, with the window overridable from the environment.
    ///
    /// A window of zero is refused and the default kept: "release as soon as the fingerprint
    /// is unchanged" would release a thread on its second sample, which is a suspension policy
    /// nobody asked for and the one value the env var could set by accident.
    pub fn from_env() -> Self {
        let window = std::env::var(WINDOW_ENV)
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .filter(|secs| *secs > 0)
            .unwrap_or(DEFAULT_WINDOW_SECS);

        Self {
            window: Duration::from_secs(window),
            interval: Duration::from_secs(DEFAULT_SAMPLE_SECS),
            grace: Duration::from_secs(DEFAULT_GRACE_SECS),
        }
    }
}

/// The activity fingerprint of one live session, as its own reducers hold it.
///
/// Infallible on purpose, like [`LiveSession::snapshot`]: a poisoned lock degrades to "as idle
/// as it looks", and the guards it feeds are the ones that keep a release safe — a session
/// whose state cannot be read is not releasable by a wrong reading, only by this one, so the
/// safe direction is to keep its clock *unchanged* rather than to invent activity.
pub fn fingerprint(session: &LiveSession) -> Fingerprint {
    let (messages, queued, streaming) = session
        .control
        .lock()
        .map(|control| {
            (
                control.message_count,
                control.queued_message_count,
                control.is_streaming,
            )
        })
        .unwrap_or((0, 0, false));

    let dialogs = session
        .pending_ui_requests()
        .map(|pending| pending.into_iter().map(|request| request.id).collect())
        .unwrap_or_default();

    Fingerprint {
        messages,
        queued,
        dialogs,
        streaming,
    }
}

/// Sample the live threads on `interval` and release the ones the policy names.
///
/// Suspension goes through [`LiveSession::shutdown`] — the same call the app makes when it
/// closes a thread and when it quits — so a released thread cannot be a new way to orphan a
/// process: the thread leaves the registry first (a command that races the release is refused
/// rather than written to a pipe that is going away), the engine is given its grace to flush
/// its session file, and only then is the id recorded as released.
///
/// Spawned on **Tauri's runtime**, not with a bare `tokio::spawn`: this runs from `setup`, which
/// has no ambient Tokio runtime — a bare spawn there panics with "there is no reactor running"
/// before a single window is drawn, and every test would still pass, because a test has one.
/// `tauri::async_runtime` creates the runtime the rest of the app runs on when Tauri has not set
/// one yet, which is the same reason `search::scan_in_background` uses it.
pub fn spawn(threads: Arc<Threads>, sink: Arc<dyn ActivitySink>, config: Config) -> JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let mut policy = IdlePolicy::default();
        let mut ticker = tokio::time::interval(config.interval);
        // A missed tick is a sampling round that arrived late, not one to catch up on: several
        // rounds in a burst would compare the same state several times and could release a
        // thread whose fingerprint only *looked* stable.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            ticker.tick().await;

            let focused = threads.focused();
            let samples: Vec<Sample> = threads
                .live()
                .into_iter()
                .map(|session| {
                    let thread = session.thread.current();

                    Sample {
                        focused: focused.as_deref() == Some(thread.as_str()),
                        fingerprint: fingerprint(&session),
                        thread,
                    }
                })
                .collect();

            for id in policy.review(Instant::now(), config.window, &samples) {
                let Some(session) = threads.remove(&id) else {
                    continue;
                };

                session.shutdown(config.grace).await;
                threads.suspend(&id);
                sink.threads(threads.snapshots());
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One sample a thread with nothing happening in it.
    fn quiet(thread: &str) -> Sample {
        Sample {
            thread: thread.to_string(),
            fingerprint: Fingerprint {
                messages: 4,
                queued: 0,
                dialogs: Vec::new(),
                streaming: false,
            },
            focused: false,
        }
    }

    const WINDOW: Duration = Duration::from_secs(600);

    /// The rule itself: the same fingerprint for the window, and the window is up.
    #[test]
    fn an_unchanged_fingerprint_past_the_window_is_released() {
        let mut policy = IdlePolicy::default();
        let start = Instant::now();

        assert!(policy.review(start, WINDOW, &[quiet("s1")]).is_empty());
        assert!(policy
            .review(
                start + WINDOW - Duration::from_secs(1),
                WINDOW,
                &[quiet("s1")]
            )
            .is_empty());
        assert_eq!(
            policy.review(start + WINDOW, WINDOW, &[quiet("s1")]),
            ["s1"]
        );
    }

    /// The first sample of a thread starts its clock, so a session is never released the
    /// moment it opens — however long the app has been running, and however short the window.
    #[test]
    fn a_fresh_session_is_never_released_in_its_first_round() {
        let mut policy = IdlePolicy::default();
        let start = Instant::now();

        // A zero window is the extreme case: it would release the instant a fingerprint
        // repeated, and the first round is the one it cannot reach.
        assert!(policy
            .review(start, Duration::ZERO, &[quiet("s1")])
            .is_empty());
        assert_eq!(policy.review(start, Duration::ZERO, &[quiet("s1")]), ["s1"]);

        // Which is why a session that opens after the app has been running for hours is safe:
        // its clock starts at its own first sample, not at the app's.
        let mut later = IdlePolicy::default();
        let opened = start + WINDOW * 10;
        assert!(later.review(opened, WINDOW, &[quiet("s1")]).is_empty());
        assert!(later
            .review(
                opened + WINDOW - Duration::from_secs(1),
                WINDOW,
                &[quiet("s1")]
            )
            .is_empty());
        assert_eq!(
            later.review(opened + WINDOW, WINDOW, &[quiet("s1")]),
            ["s1"]
        );
    }

    /// Each guard on its own: a streaming turn, a pending dialog and a queued message keep the
    /// sidecar past the window even though nothing about the thread has changed for longer
    /// than it — the fingerprint is *stable*, and the guards still say no.
    #[test]
    fn every_guard_holds_the_sidecar() {
        let held = [
            Fingerprint {
                streaming: true,
                ..quiet("s1").fingerprint
            },
            Fingerprint {
                dialogs: vec!["ui_1".to_string()],
                ..quiet("s1").fingerprint
            },
            Fingerprint {
                queued: 1,
                ..quiet("s1").fingerprint
            },
        ];

        for fingerprint in held {
            let mut policy = IdlePolicy::default();
            let start = Instant::now();
            let sample = Sample {
                thread: "s1".to_string(),
                fingerprint,
                focused: false,
            };

            policy.review(start, WINDOW, std::slice::from_ref(&sample));
            let released = policy.review(start + WINDOW * 2, WINDOW, std::slice::from_ref(&sample));

            assert!(
                released.is_empty(),
                "a guard must hold the sidecar: {released:?}"
            );
        }

        // The focused thread shares the other three guards' fate, and is the one guard the
        // session itself cannot state.
        let mut policy = IdlePolicy::default();
        let start = Instant::now();
        policy.review(start, WINDOW, &[quiet("s1")]);
        assert!(policy
            .review(
                start + WINDOW * 2,
                WINDOW,
                &[Sample {
                    focused: true,
                    ..quiet("s1")
                }]
            )
            .is_empty());

        // And the moment it stops being the one on screen, the same state is released.
        assert_eq!(
            policy.review(start + WINDOW * 3, WINDOW, &[quiet("s1")]),
            ["s1"]
        );
    }

    /// A fingerprint that moves resets the clock, however long the thread has been open: that
    /// is the difference between "idle for ten minutes" and "open for ten minutes".
    #[test]
    fn a_changed_fingerprint_restarts_the_clock() {
        let mut policy = IdlePolicy::default();
        let start = Instant::now();

        policy.review(start, WINDOW, &[quiet("s1")]);

        // A message arrives just before the window would have elapsed, and nothing else moves
        // after it.
        let moved = Sample {
            fingerprint: Fingerprint {
                messages: 6,
                ..quiet("s1").fingerprint
            },
            ..quiet("s1")
        };
        let nearly = start + WINDOW - Duration::from_secs(1);
        assert!(policy
            .review(nearly, WINDOW, std::slice::from_ref(&moved))
            .is_empty());

        assert!(
            policy
                .review(start + WINDOW, WINDOW, std::slice::from_ref(&moved))
                .is_empty(),
            "the old clock would have elapsed here; the message restarted it"
        );
        assert!(policy
            .review(
                nearly + WINDOW - Duration::from_secs(1),
                WINDOW,
                std::slice::from_ref(&moved),
            )
            .is_empty());
        assert_eq!(
            policy.review(nearly + WINDOW, WINDOW, &[moved]),
            ["s1"],
            "and it elapses from the sample that moved"
        );
    }

    /// Only the live threads are clocked: a thread that is gone (released, closed, resumed as
    /// another id) cannot be released from a stale reading.
    #[test]
    fn a_thread_that_is_gone_stops_being_clocked() {
        let mut policy = IdlePolicy::default();
        let start = Instant::now();

        policy.review(start, WINDOW, &[quiet("s1"), quiet("s2")]);
        assert_eq!(
            policy.review(start + WINDOW, WINDOW, &[quiet("s2")]),
            ["s2"],
            "the one that stayed is the one that elapses"
        );
        assert_eq!(
            policy.review(start + WINDOW * 2, WINDOW, &[quiet("s1")]),
            Vec::<String>::new(),
            "s1 is new again, so its clock starts now"
        );
    }
}
