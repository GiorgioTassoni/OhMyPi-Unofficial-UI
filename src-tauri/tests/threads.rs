//! Several threads at once, which is what the registry exists for.
//!
//! Step 9's claim in one test: the host holds more than one live session, each with its
//! own sidecar (D5), addresses every command by thread, and leaves nothing running when
//! a thread closes. A window can show one dot at a time; this is what proves the second
//! session is real, independently closeable, and not a row the host invented.
//!
//! What it asserts, in the order the acceptance asks for it:
//!
//! 1. Both threads are registered, keyed by the **engine's** session id, and the status
//!    of each reports its own workspace — the two differ, and each session id came back
//!    from the engine's own `get_state` rather than from the id the host passed in.
//! 2. The events reached the sink *tagged*: a row patch from one thread cannot be folded
//!    into the other's conversation, which is the whole point of the envelope.
//! 3. The catalogue (`sessions`) lists the second workspace's session, with nothing but
//!    the engine's on-disk store behind it.
//! 4. Closing one thread leaves the other live and still answering.
//! 5. Closing both leaves no sidecar behind — the orphan check the M0 test already has.
//!
//! Ignored by default: it spawns two real agents, persists two real sessions, and needs
//! provider credentials for the one turn it sends.
//!
//! ```text
//! cargo test -p omp-desktop --test threads -- --ignored --nocapture
//! ```

mod support;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use omp_desktop::bridge;
use omp_desktop::dto::SessionSummaryDto;
use omp_desktop::session::{open, LiveSession};
use omp_desktop::threads::Threads;
use omp_store::Store;
use omp_transport::protocol::{self, commands};
use omp_transport::{ClientOptions, SidecarSpec};
use support::{process_exists, wait_for, RecordingSink};

fn options() -> ClientOptions {
    ClientOptions {
        request_timeout: Duration::from_secs(60),
        ..Default::default()
    }
}

/// A workspace named for this test, so a run that dies before cleanup cannot make the
/// next one pass on stale state.
///
/// Persistent rather than `--no-session`: the catalogue assertion below needs the engine
/// to write a session file, and that is the file the sidebar lists.
fn workspace(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("omp-desktop-threads-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("the workspace");
    path
}

/// Open one thread exactly as `open_thread` does: spawn in `workspace`, handshake, then
/// register under the id the engine reported.
async fn open_thread(
    threads: &Arc<Threads>,
    sink: &RecordingSink,
    workspace: &Path,
) -> Arc<LiveSession> {
    let live = open(
        &SidecarSpec::omp(workspace),
        options(),
        sink.clone(),
        threads.clone(),
    )
    .await
    .expect("the session opens");

    threads.insert(Arc::clone(&live));

    live
}

/// Send one prompt and wait for the turn to *finish*.
///
/// Waiting on `!is_streaming` alone is a race: the flag is false before the turn starts,
/// so the wait would return immediately. The engine's own `messageCount` is what moves,
/// twice per turn, so the wait is for it to move *and* for the turn to be over.
async fn say(live: &LiveSession, prompt: &str) {
    let before = live.status().expect("status").control.message_count;

    let client = live.client().expect("the session is open");
    let accepted = client
        .request(
            commands::prompt(prompt, &[], None),
            Some(Duration::from_secs(30)),
        )
        .await
        .expect("the prompt is accepted");
    assert!(protocol::is_success(&accepted), "rejected: {accepted}");

    wait_for("the turn to finish", || {
        let status = live.status().ok()?;
        (status.control.message_count > before && !status.control.is_streaming).then_some(())
    })
    .await;
}

/// The catalogue the `sessions` command answers with, once it lists `id`.
///
/// Polled rather than read once: the engine creates a session file on its first write,
/// so there is a moment after the handshake when the session exists and the store does
/// not know about it yet.
///
/// The scan runs on a blocking worker, not inline: this is the same runtime the pump
/// runs on, and parking it behind a store scan would stop the very events being waited
/// for elsewhere in this test.
async fn catalogue_lists(
    store: &Store,
    threads: &Arc<Threads>,
    id: &str,
) -> Vec<SessionSummaryDto> {
    let deadline = Instant::now() + Duration::from_secs(120);

    loop {
        let store = store.clone();
        let threads = Arc::clone(threads);
        let rows =
            tokio::task::spawn_blocking(move || bridge::session_catalogue(Some(&store), &threads))
                .await
                .expect("the catalogue scan runs");

        if rows.iter().any(|row| row.id == id) {
            return rows;
        }

        assert!(
            Instant::now() < deadline,
            "the catalogue never listed {id}: {} rows",
            rows.len()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test]
#[ignore = "spawns two real `omp` sidecars, persists two sessions, and needs credentials"]
async fn two_threads_live_at_once_and_close_independently() {
    let threads = Arc::new(Threads::default());
    let sink = RecordingSink::default();
    let first_workspace = workspace("first");
    let second_workspace = workspace("second");

    // ---- 1. Two threads, two sidecars -------------------------------------
    let first = open_thread(&threads, &sink, &first_workspace).await;
    let second = open_thread(&threads, &sink, &second_workspace).await;
    let first_id = first.thread.current();
    let second_id = second.thread.current();

    assert_ne!(first_id, second_id, "two sessions cannot share an id");
    println!("first:  {first_id}\nsecond: {second_id}");

    // Registry order is by id, so the rows are compared as a set of ids rather than as a
    // sequence: the point is which threads are in it, not which one sorts first.
    let ids: BTreeSet<String> = threads.snapshots().into_iter().map(|row| row.id).collect();
    assert_eq!(
        ids,
        BTreeSet::from([first_id.clone(), second_id.clone()]),
        "both threads must be registered"
    );

    // `thread_status`, without the Tauri state: the registry lookup plus the session's
    // own status. Each must report its own workspace, and the session id inside the
    // status must be the key it is filed under — that is what makes the id the engine's
    // rather than one the host invented.
    let first_status = threads
        .get(&first_id)
        .expect("the first thread is registered")
        .status()
        .expect("the first status is readable");
    let second_status = threads
        .get(&second_id)
        .expect("the second thread is registered")
        .status()
        .expect("the second status is readable");

    assert_eq!(
        first_status.workspace,
        first_workspace.display().to_string()
    );
    assert_eq!(
        second_status.workspace,
        second_workspace.display().to_string()
    );
    assert_ne!(
        first_status.workspace, second_status.workspace,
        "the two threads must not be reporting one workspace"
    );
    assert_eq!(first_status.control.session_id, first_id);
    assert_eq!(second_status.control.session_id, second_id);

    let first_pid = first_status.sidecar_pid.expect("a pid to watch");
    let second_pid = second_status.sidecar_pid.expect("a pid to watch");
    assert!(process_exists(first_pid) && process_exists(second_pid));
    println!("pids: first {first_pid}, second {second_pid}");

    // ---- 2. One turn in the second thread ---------------------------------
    // The catalogue needs a session file, which the engine writes on its first message,
    // and a turn is also what makes the pump publish rows at all.
    say(&second, "Reply with exactly: ok").await;

    let roster = wait_for("the roster to carry both threads", || {
        let roster = sink.roster();
        (roster.len() == 2).then_some(roster)
    })
    .await;
    println!("roster: {roster:?}");
    assert_eq!(
        roster
            .iter()
            .map(|row| row.workspace.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            first_workspace.to_str().expect("utf-8"),
            second_workspace.to_str().expect("utf-8"),
        ]),
        "the sidebar's roster must carry one row per live thread, in its own workspace"
    );

    let live_threads = BTreeSet::from([first_id.clone(), second_id.clone()]);
    assert!(
        sink.event_threads().contains(&second_id),
        "the second thread's events must be tagged with it"
    );
    assert!(
        sink.event_threads()
            .iter()
            .all(|thread| live_threads.contains(thread)),
        "every event must be addressed to a live thread: {:?}",
        sink.event_threads()
    );
    assert!(
        sink.patch_threads().contains(&second_id),
        "a conversation patch must be tagged with the thread it came from: {:?}",
        sink.patch_threads()
    );
    assert!(
        sink.patch_threads()
            .iter()
            .all(|thread| thread == &second_id),
        "only the thread that ran a turn can have published rows: {:?}",
        sink.patch_threads()
    );

    // ---- 3. The catalogue knows about the second thread -------------------
    let store = Store::discover().expect("the engine's store is discoverable");
    let catalogue = catalogue_lists(&store, &threads, &second_id).await;

    let listed = catalogue
        .iter()
        .find(|row| row.id == second_id)
        .expect("the catalogue lists the second thread");
    println!("catalogue row: {listed:?}");
    assert_eq!(
        listed.cwd,
        second_workspace.display().to_string(),
        "the catalogue groups by the session's own cwd"
    );
    assert!(
        !listed.first_message.is_empty(),
        "the row carries what to show for a session with no title"
    );

    let projects = bridge::project_groups(Some(&store));
    assert!(
        projects
            .iter()
            .any(|project| project.path == second_workspace.display().to_string()),
        "the second workspace must appear as a project: {projects:?}"
    );

    // ---- 4. Closing one leaves the other alive ---------------------------
    // The close `close_thread` performs: out of the registry first, so nothing can
    // address a session that is going away, then the engine drains.
    let closed = threads
        .remove(&first_id)
        .expect("the first thread is registered");
    closed.shutdown(Duration::from_secs(10)).await;
    wait_for("the first sidecar to exit", || {
        (!process_exists(first_pid)).then_some(())
    })
    .await;

    let survivors = threads.snapshots();
    assert_eq!(survivors.len(), 1, "exactly one thread must survive");
    assert_eq!(survivors[0].id, second_id);
    assert!(
        threads.get(&first_id).is_none(),
        "the closed thread is gone"
    );

    let survivor = threads
        .get(&second_id)
        .expect("the second thread must still be registered");
    assert!(
        process_exists(second_pid),
        "closing one thread must not stop the other's sidecar"
    );
    assert!(
        !survivor
            .status()
            .expect("the survivor's status")
            .control
            .is_streaming,
        "the survivor must still answer its own commands"
    );

    // ---- 5. Closing both leaves nothing running --------------------------
    let closed = threads
        .remove(&second_id)
        .expect("the second thread is registered");
    closed.shutdown(Duration::from_secs(10)).await;
    wait_for("the second sidecar to exit", || {
        (!process_exists(second_pid)).then_some(())
    })
    .await;

    assert!(
        threads.snapshots().is_empty(),
        "no thread may survive its own close"
    );
    assert!(
        !process_exists(first_pid) && !process_exists(second_pid),
        "no sidecar may survive the close of both threads"
    );

    // ---- 6. A session id resolves back to the file a resume needs ---------
    // The other half of the id story: the sidebar lists ids, and the engine must be given
    // a path (`session_path`). An id it cannot resolve is refused *before* a sidecar is
    // spawned — measured, `omp --resume <unknown-id>` exits(1) before `ready`, which the
    // host would otherwise see as a sidecar that died for no reason.
    assert!(
        bridge::session_path(Some(&store), "no-such-session-id").is_err(),
        "an unknown id must be refused before anything is launched"
    );

    let path = bridge::session_path(Some(&store), &second_id)
        .expect("the second session resolves to its file");
    println!("resume path: {path}");
    assert!(
        Path::new(&path).is_absolute(),
        "the engine gets a path, never a bare id: {path}"
    );

    let resumed = open(
        &SidecarSpec::omp(&second_workspace).resuming(&path),
        options(),
        sink.clone(),
        threads.clone(),
    )
    .await
    .expect("the resumed session opens");

    assert_eq!(
        resumed.thread.current(),
        second_id,
        "a resume of the same file keeps the session's own id, which is the registry key"
    );

    // The hydration the resume path is for: the event stream starts at connect, so the
    // conversation can only be here through the pager (`Transcript::restore`).
    let rows = resumed.rows().expect("the transcript is readable");
    println!("resumed rows: {rows:?}");
    assert_eq!(
        rows.first().map(|row| row.text.as_str()),
        Some("Reply with exactly: ok"),
        "a resumed thread must show the conversation it already had"
    );

    let resumed_pid = resumed
        .status()
        .expect("the resumed status")
        .sidecar_pid
        .expect("a pid to watch");
    resumed.shutdown(Duration::from_secs(10)).await;
    wait_for("the resumed sidecar to exit", || {
        (!process_exists(resumed_pid)).then_some(())
    })
    .await;

    // The workspaces are this test's; the session files are the engine's (they are what
    // step 2's catalogue reads, and step 6 resumes), so only the former are removed.
    let _ = std::fs::remove_dir_all(&first_workspace);
    let _ = std::fs::remove_dir_all(&second_workspace);
}
