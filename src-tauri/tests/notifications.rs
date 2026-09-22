//! The notification triggers, against a real engine (`docs/12` §13, `docs/14` §2)
//!
//! Two things a unit test cannot answer, and both are about the *stream* rather than the rule:
//! that a completed turn really produces `agent_end` in the shape the rule reads, and that a
//! sidecar dying mid-conversation is noticed at all — the second one mattered, because the
//! pump used to sit on a broadcast stream whose senders it owned itself, so the engine's death
//! was invisible: a streaming dot over a conversation that would never move again, and an
//! approval dialog left on screen behind a dead agent.
//!
//! Ignored by default: it spawns real agents and drives one real turn each (credentials).
//!
//! ```text
//! cargo test -p omp-desktop --test notifications -- --ignored --nocapture
//! ```

mod support;

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use omp_desktop::notify::{KIND_FAILED, KIND_TURN_FINISHED};
use omp_desktop::session::open;
use omp_desktop::threads::Threads;
use omp_transport::{ClientOptions, SidecarSpec};
use support::{process_exists, say, wait_for, RecordingSink};

fn options() -> ClientOptions {
    ClientOptions {
        request_timeout: Duration::from_secs(60),
        ..Default::default()
    }
}

/// A workspace named for this test, so a run that dies before cleanup cannot make the next one
/// pass on stale state.
fn workspace(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("omp-desktop-notify-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("the workspace");
    path
}

#[tokio::test]
#[ignore = "spawns two real `omp` sidecars and needs credentials for the turn"]
async fn a_completed_turn_is_reported_for_its_own_thread_only() {
    let threads = Arc::new(Threads::default());
    let sink = RecordingSink::default();
    let first_workspace = workspace("first");
    let second_workspace = workspace("second");

    let first = open(
        &SidecarSpec::omp(&first_workspace),
        options(),
        sink.clone(),
        threads.clone(),
    )
    .await
    .expect("the first session opens");
    let second = open(
        &SidecarSpec::omp(&second_workspace),
        options(),
        sink.clone(),
        threads.clone(),
    )
    .await
    .expect("the second session opens");
    threads.insert(Arc::clone(&first));
    threads.insert(Arc::clone(&second));

    let first_id = first.thread.current();
    let second_id = second.thread.current();
    println!("first {first_id}, second {second_id}");

    // ---- one turn, in the first thread ------------------------------------
    say(&first, "Reply with exactly: ok").await;

    let reported = wait_for("the first thread's completion to be reported", || {
        let notifications = sink.notifications_for(&first_id);
        (!notifications.is_empty()).then_some(notifications)
    })
    .await;
    println!("reported for the first thread: {reported:?}");

    assert_eq!(
        reported.len(),
        1,
        "one turn is one notification, not one per event"
    );
    let completion = &reported[0];
    assert_eq!(completion.kind, KIND_TURN_FINISHED);
    assert_eq!(completion.thread, first_id);
    assert_eq!(completion.job_id, None);

    // The body is the engine's own last words, not a summary of them: the same string the
    // conversation's last row holds. (What the model says is not this test's business.)
    let rows = first.rows().expect("the transcript reads");
    let answer = rows
        .iter()
        .rev()
        .find(|row| row.role == "assistant")
        .map(|row| row.text.trim().to_string())
        .expect("the turn produced an assistant row");
    assert_eq!(completion.body, answer);
    assert!(
        !completion.title.is_empty(),
        "the title is the session's name or, until it has one, its id"
    );

    // ---- and nothing for the thread that did not run ----------------------
    assert!(
        sink.notifications_for(&second_id).is_empty(),
        "another thread's turn must not be reported against this one: {:?}",
        sink.notifications_for(&second_id)
    );

    // A second turn in the *second* thread is reported for the second thread, and the first
    // thread's list does not grow: the tag is the engine's session id, and two live sessions
    // are otherwise indistinguishable to a subscriber.
    say(&second, "Reply with exactly: ok").await;
    wait_for("the second thread's completion to be reported", || {
        (!sink.notifications_for(&second_id).is_empty()).then_some(())
    })
    .await;

    let first_after: Vec<String> = sink
        .notifications_for(&first_id)
        .iter()
        .map(|notification| notification.thread.clone())
        .collect();
    assert_eq!(first_after, vec![first_id.clone()]);
    println!(
        "measured: first={:?} second={:?}",
        sink.notifications_for(&first_id)
            .iter()
            .map(|notification| notification.kind.clone())
            .collect::<Vec<_>>(),
        sink.notifications_for(&second_id)
            .iter()
            .map(|notification| notification.kind.clone())
            .collect::<Vec<_>>()
    );

    first.shutdown(Duration::from_secs(10)).await;
    second.shutdown(Duration::from_secs(10)).await;
    let _ = std::fs::remove_dir_all(&first_workspace);
    let _ = std::fs::remove_dir_all(&second_workspace);
}

/// A sidecar killed outright is noticed, and the app says so once.
///
/// This is the shape of the audit's finding: the pump's `Closed` arms can never fire, because
/// the broadcast senders live inside the `OmpClient` the pump itself holds. Everything a dead
/// engine leaves behind is invisible without the one signal that does report it — so the test
/// kills the process and checks all three consequences: the reason lands in the conversation
/// (where `docs/12` §2.2's red dot is derived from), the roster stops presenting the thread as
/// live, and exactly one `failed` notification is published for it.
#[tokio::test]
#[ignore = "spawns a real `omp` sidecar and kills it"]
async fn a_sidecar_that_dies_is_reported_and_leaves_no_live_row() {
    let threads = Arc::new(Threads::default());
    let sink = RecordingSink::default();
    let workspace = workspace("death");

    let live = open(
        &SidecarSpec::omp(&workspace),
        options(),
        sink.clone(),
        threads.clone(),
    )
    .await
    .expect("the session opens");
    threads.insert(Arc::clone(&live));

    let id = live.thread.current();
    let pid = live.status().expect("status").sidecar_pid.expect("a pid");
    println!("opened {id} as pid {pid}");

    // `kill -9`: not a graceful close, which is the point — the engine dies the way a crash or
    // an OOM kill does, with no frame to say goodbye.
    let killed = Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .expect("kill runs");
    assert!(
        killed.success(),
        "the sidecar had to be killed for this test"
    );

    wait_for("the dead sidecar to be reported", || {
        let rows = live.rows().ok()?;
        rows.iter()
            .any(|row| row.role == "notice:error" && row.text.contains("sidecar"))
            .then_some(())
    })
    .await;

    // The thread is out of the registry, so `threads-updated` cannot present it as live and a
    // command addressed to it is refused rather than written into a pipe nobody reads.
    wait_for("the dead thread to leave the registry", || {
        threads.get(&id).is_none().then_some(())
    })
    .await;
    wait_for("the roster to stop listing it", || {
        sink.roster().iter().all(|row| row.id != id).then_some(())
    })
    .await;

    let reported = sink.notifications_for(&id);
    println!("reported after the kill: {reported:?}");
    assert_eq!(reported.len(), 1, "a death is one fact, not one per tick");
    assert_eq!(reported[0].kind, KIND_FAILED);
    assert!(
        reported[0].body.contains("stdout") || reported[0].body.contains("sidecar"),
        "the body is the reader's own reason: {}",
        reported[0].body
    );

    // The row that says so is the transcript's, which is where a failure is visible: a
    // `streaming` dot would have stayed true for ever otherwise.
    let row = live
        .rows()
        .expect("the transcript reads")
        .into_iter()
        .rev()
        .find(|row| row.role == "notice:error")
        .expect("the reason is a row");
    println!("row: {}", row.text);
    assert!(!row.streaming);

    assert!(
        !process_exists(pid),
        "the killed sidecar must not have been replaced by another"
    );
    let _ = std::fs::remove_dir_all(&workspace);
}
