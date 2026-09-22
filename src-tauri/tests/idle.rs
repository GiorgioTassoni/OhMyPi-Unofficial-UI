//! M6's suspend/resume half, against a real engine (`docs/14` §2).
//!
//! The two-concurrent-streams half lives in `threads.rs`; this is the other one: a thread
//! nobody is using is *released* — the process is gone, not merely forgotten — its row in the
//! catalogue says so, and resuming it through the path the window uses brings the whole
//! conversation back and answers a new command.
//!
//! What it asserts, in the order the acceptance asks for it:
//!
//! 1. A thread whose fingerprint stops moving is released by the real supervisor, and the
//!    process is gone **as the OS sees it** (`/proc/<pid>`), not merely absent from the
//!    registry.
//! 2. The thread the window is showing is never released: focus is the one guard the session
//!    cannot state itself, so it is proved against a live process rather than in a unit test.
//! 3. The catalogue's row still lists the session, with `suspended: true`.
//! 4. Resuming it — `session_path` through the engine's own catalogue, then `--resume <path>`,
//!    exactly what `open_thread` does — returns the same session id, the same transcript rows
//!    (count, first and last text), and an engine that answers a new command.
//! 5. Nothing survives: the released sidecar was gone before the resume, and the resumed one
//!    is gone after the close.
//!
//! Ignored by default: it spawns a real agent, persists a real session, and sends the one turn
//! whose rows it preserves. Nothing here asserts what the model answered — a turn that fails
//! writes the same two rows — but a successful reply is what this machine produces.
//!
//! ```text
//! cargo test -p omp-desktop --test idle -- --ignored --nocapture
//! ```

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use omp_desktop::bridge;
use omp_desktop::dto::SessionSummaryDto;
use omp_desktop::idle::{self, Config};
use omp_desktop::session::open;
use omp_desktop::threads::Threads;
use omp_store::Store;
use omp_transport::protocol::{self, commands};
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
    let path = std::env::temp_dir().join(format!("omp-desktop-idle-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("the workspace");
    path
}

/// How long the released or resumed sidecar gets to exit.
const GRACE: Duration = Duration::from_secs(10);

/// The window this test runs the policy with.
///
/// Short because the *clock* is the policy's business and 600 seconds of it are asserted in
/// `idle.rs`'s unit tests; what a live run adds is that the supervisor is wired to the
/// registry, the real fingerprints, the real shutdown and the real publish.
const WINDOW: Duration = Duration::from_secs(2);

/// The catalogue the `sessions` command answers with.
///
/// A blocking store scan, so it runs on a worker of its own: this is the runtime the pump runs
/// on, and parking it behind a scan would stop the very events being waited for.
async fn catalogue(store: &Store, threads: &Arc<Threads>) -> Vec<SessionSummaryDto> {
    let store = store.clone();
    let threads = Arc::clone(threads);

    tokio::task::spawn_blocking(move || bridge::session_catalogue(Some(&store), &threads))
        .await
        .expect("the catalogue scan runs")
}

/// The row for one thread, once the catalogue lists it.
///
/// Polled rather than read once: the engine creates a session file on its first write, so
/// there is a moment after the open when the session exists and the store does not list it.
async fn row_for(store: &Store, threads: &Arc<Threads>, id: &str) -> SessionSummaryDto {
    let deadline = Instant::now() + Duration::from_secs(120);

    loop {
        let found = catalogue(store, threads)
            .await
            .into_iter()
            .find(|row| row.id == id);

        if let Some(row) = found {
            return row;
        }

        assert!(Instant::now() < deadline, "the catalogue never listed {id}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test]
#[ignore = "spawns a real `omp` sidecar, releases it on an idle window, and resumes it"]
async fn an_idle_thread_is_released_and_resumes_with_its_whole_conversation() {
    let threads = Arc::new(Threads::default());
    let sink = RecordingSink::default();
    let workspace = workspace("session");
    let store = Store::discover().expect("the engine's store is discoverable");

    // ---- 1. A thread with a conversation ----------------------------------
    // Opened exactly as `open_thread` does, and registered under the id the engine reported:
    // the id is the engine's own, so the catalogue, the registry and the resume all agree on
    // it without a mapping table.
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
    let pid = live
        .status()
        .expect("the status reads")
        .sidecar_pid
        .expect("a pid to watch");
    println!("opened {id} as pid {pid}");
    assert!(process_exists(pid));

    // One turn, because a session file only exists once the engine has written a message to it
    // (measured: a local builtin writes nothing), and the file is what the catalogue lists.
    say(&live, "Reply with exactly: ok").await;

    let rows_before = live.rows().expect("the transcript reads");
    let first_before = rows_before.first().map(|row| row.text.clone());
    let last_before = rows_before.last().map(|row| row.text.clone());
    println!(
        "rows before release: {} ({first_before:?} … {last_before:?})",
        rows_before.len()
    );
    assert!(
        !rows_before.is_empty(),
        "the turn must have left the conversation something to preserve"
    );

    let listed = row_for(&store, &threads, &id).await;
    assert!(
        !listed.suspended,
        "a thread that has never been released is not suspended"
    );

    // ---- 2. The policy, running for real ----------------------------------
    let config = Config {
        window: WINDOW,
        interval: Duration::from_millis(200),
        grace: GRACE,
    };
    let supervisor = idle::spawn(Arc::clone(&threads), Arc::new(sink.clone()), config);

    // The focused thread is the one guard the session cannot state itself, so it is proved
    // here against a live process: the window is showing it, and it stays.
    threads.focus(Some(id.clone()));
    tokio::time::sleep(WINDOW * 3).await;
    assert!(
        process_exists(pid),
        "the thread the window is showing must not be released"
    );
    assert!(
        threads.get(&id).is_some(),
        "and it must still be registered"
    );
    println!("focused for {:?}: pid {pid} still alive", WINDOW * 3);

    // ---- 3. Released ------------------------------------------------------
    threads.focus(None);
    wait_for("the idle sidecar to be released", || {
        (!process_exists(pid)).then_some(())
    })
    .await;

    assert!(
        threads.get(&id).is_none(),
        "a released thread must leave the live registry: nothing can address it"
    );
    assert!(
        threads.is_suspended(&id),
        "and the app must remember that it released it"
    );

    wait_for("the roster to stop presenting it as live", || {
        let roster = sink.roster();
        (roster.iter().all(|row| row.id != id)).then_some(())
    })
    .await;
    println!(
        "released: pid {pid} gone, {} live rows left",
        sink.roster().len()
    );

    let released_row = row_for(&store, &threads, &id).await;
    assert!(
        released_row.suspended,
        "the catalogue's row must say the thread was released"
    );
    assert_eq!(
        released_row.message_count, listed.message_count,
        "a release changes no fact about the session file"
    );
    println!(
        "catalogue row after release: {} suspended={}",
        released_row.id, released_row.suspended
    );

    // The supervisor has done its job; leaving it running would release the *resumed* thread
    // underneath the assertions below, which is not what is being tested here.
    supervisor.abort();

    // ---- 4. Resumed through the path the window uses -----------------------
    let path = bridge::session_path(Some(&store), &id).expect("the session resolves to its file");
    assert!(
        Path::new(&path).is_absolute(),
        "the engine gets a path: {path}"
    );

    let resumed = open(
        &SidecarSpec::omp(&workspace).resuming(&path),
        options(),
        sink.clone(),
        threads.clone(),
    )
    .await
    .expect("the released session resumes");
    threads.insert(Arc::clone(&resumed));

    assert_eq!(
        resumed.thread.current(),
        id,
        "a resume of the same file is the same thread, which is the registry's key"
    );
    assert!(
        !threads.is_suspended(&id),
        "the moment it resumes, it is not suspended"
    );

    let resumed_pid = resumed
        .status()
        .expect("the resumed status")
        .sidecar_pid
        .expect("a pid to watch");
    assert!(process_exists(resumed_pid));
    println!("resumed as pid {resumed_pid}");

    // The whole conversation: the event stream starts where the host connects, so these rows
    // can only be here through the pager (`session::hydrate`).
    let rows_after = resumed.rows().expect("the transcript reads");
    println!(
        "rows after resume: {} ({:?} … {:?})",
        rows_after.len(),
        rows_after.first().map(|row| row.text.clone()),
        rows_after.last().map(|row| row.text.clone())
    );
    assert_eq!(
        rows_after.len(),
        rows_before.len(),
        "a release and resume must not change the conversation"
    );
    assert_eq!(
        rows_after.first().map(|row| row.text.clone()),
        first_before,
        "the first row must survive the round trip"
    );
    assert_eq!(
        rows_after.last().map(|row| row.text.clone()),
        last_before,
        "and so must the last"
    );

    let resumed_row = row_for(&store, &threads, &id).await;
    assert!(
        !resumed_row.suspended,
        "a resumed thread's row must stop saying it was released"
    );

    // A new command, on the resumed engine: the control snapshot is re-read from the engine
    // itself, so an answer proves the process is not merely present but working.
    let control = resumed
        .reread_control()
        .await
        .expect("the resumed engine answers get_state");
    assert_eq!(control.session_id, id);
    assert_eq!(
        control.message_count, resumed_row.message_count,
        "the engine's own count agrees with the catalogue's"
    );

    // And one more turn's worth of *command* the host drives: a local builtin, which needs no
    // model, proves the resumed session still runs the engine's dispatch path.
    let client = resumed.client().expect("the resumed session is open");
    let accepted = client
        .request(
            commands::prompt("/skillful status", &[], None),
            Some(Duration::from_secs(30)),
        )
        .await
        .expect("the resumed engine accepts a command");
    assert!(protocol::is_success(&accepted), "rejected: {accepted}");
    wait_for("the resumed session's command output", || {
        resumed
            .rows()
            .ok()?
            .iter()
            .any(|row| row.role == "notice:info")
            .then_some(())
    })
    .await;
    println!("the resumed session answered a command");

    // ---- 5. Nothing survives ---------------------------------------------
    resumed.shutdown(GRACE).await;
    wait_for("the resumed sidecar to exit", || {
        (!process_exists(resumed_pid)).then_some(())
    })
    .await;
    assert!(
        !process_exists(pid) && !process_exists(resumed_pid),
        "neither the released nor the resumed sidecar may survive"
    );

    // The workspaces are this test's; the session files are the engine's (a release resolves
    // through the catalogue, which reads them), so only the former are removed.
    let _ = std::fs::remove_dir_all(&workspace);
}

/// The one thing the live test above cannot show: a thread that *is* the app's focus is not
/// released, and the moment it stops being, it is — with the same registry the window drives.
///
/// Separate from the flow above because the two orderings cannot both happen to one thread,
/// and it needs no turn: nothing here depends on a transcript.
#[tokio::test]
#[ignore = "spawns a real `omp` sidecar and releases it twice over a two-second window"]
async fn the_focused_thread_outlives_the_window_and_is_released_when_focus_moves() {
    let threads = Arc::new(Threads::default());
    let sink = RecordingSink::default();

    let first = open(
        &SidecarSpec::omp(std::env::temp_dir()).ephemeral(),
        options(),
        sink.clone(),
        threads.clone(),
    )
    .await
    .expect("the first session opens");
    let second = open(
        &SidecarSpec::omp(std::env::temp_dir()).ephemeral(),
        options(),
        sink.clone(),
        threads.clone(),
    )
    .await
    .expect("the second session opens");
    threads.insert(Arc::clone(&first));
    threads.insert(Arc::clone(&second));

    let first_id = first.thread.current();
    let first_pid = first.status().expect("status").sidecar_pid.expect("a pid");
    let second_pid = second.status().expect("status").sidecar_pid.expect("a pid");
    println!("first {first_id} pid {first_pid}, second pid {second_pid}");

    // Left running: the second release is the last thing this test waits for.
    let _supervisor = idle::spawn(
        Arc::clone(&threads),
        Arc::new(sink.clone()),
        Config {
            window: WINDOW,
            interval: Duration::from_millis(200),
            grace: GRACE,
        },
    );

    threads.focus(Some(first_id.clone()));
    tokio::time::sleep(WINDOW * 3).await;

    // Both are idle by every other measure; only the focused one survives.
    assert!(
        process_exists(first_pid),
        "the focused thread must outlast the window"
    );
    wait_for("the unfocused thread to be released", || {
        (!process_exists(second_pid)).then_some(())
    })
    .await;
    assert!(
        threads.is_suspended(&second.thread.current()),
        "the thread nobody was looking at is the one that goes"
    );
    println!("the unfocused thread was released; the focused one is still pid {first_pid}");

    // Focus moves, and the thread that had it goes the same way.
    threads.focus(None);
    wait_for("the previously focused thread to be released", || {
        (!process_exists(first_pid)).then_some(())
    })
    .await;
    assert!(threads.is_suspended(&first_id));
    println!("focus moved: the second thread was released too");
}
