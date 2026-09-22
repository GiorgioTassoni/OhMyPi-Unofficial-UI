//! The session flows, against a real engine (`docs/12` §2.3).
//!
//! Step 9's second half in one test: everything a thread row's context menu can do,
//! observed end to end rather than reasoned about. What it asserts, in the order the
//! acceptance asks for it:
//!
//! 1. **Rename.** `rename_thread` answers, and the new name is in the control snapshot
//!    `thread_status` reads — which only holds because the flow re-reads `get_state`
//!    (measured: the engine answers a bare ack and emits nothing at all for a rename).
//!    An empty name is refused **in the host**, with the engine's own sentence.
//! 2. **Fork.** `branch_targets` answers with the messages a fork can start from,
//!    `branch_thread` reports an id that is **not** the one it was addressed to, the new
//!    id is the registry's and the old one is gone from it — and the transcript is the
//!    fork's (the message it forked at is not in it, the one before it is).
//! 3. **Handoff.** Reports success, and **no file** appears under the session's artifacts
//!    directory: measured at v18.2.6, the RPC path never sets `savedPath` because that
//!    assignment sits behind `autoTriggered`, and the document is committed as a
//!    compaction entry in the same session instead.
//! 4. **Export.** The returned path exists, is absolute, and is inside the app's own
//!    export directory — the one the allowlist admits and `open_path` may open.
//! 5. **Delete.** Refused while the thread is live (the sidecar would recreate the file
//!    on its next write), and removes the session file once it is not.
//!
//! Isolation, because a live engine writes: the sidecar is given `--session-dir`, so every
//! session this test creates lands in a temp directory that is removed at the end, and the
//! app's `Store` is pointed at the same temp agent directory. The engine's *configuration*
//! — models, credentials, settings — stays where the user put it and is only read: pointing
//! `PI_CODING_AGENT_DIR` at a temp directory would be the simpler isolation, but it would
//! also hide the user's own credentials from the agent, and copying them out of the real
//! store to get them back is not something a test should do.
//!
//! One settings overlay rides along (`--config`), and it is not decoration: measured
//! without it, the handoff is refused with `Nothing to hand off (already compacted)` —
//! `prepareCompaction` returns nothing when everything is inside the retained tail, and
//! `compaction.keepRecentTokens` defaults to 20 000 tokens, which a conversation this test
//! can afford never reaches. The overlay is an *extra* config layer, loaded after the
//! user's own settings (`Settings::#rebuildMerged`), so none of theirs is replaced.
//!
//! Ignored by default: it spawns a real agent, persists real sessions, needs provider
//! credentials, and costs a handful of model calls (the handoff document is generated).
//!
//! ```text
//! cargo test -p omp-desktop --test flows -- --ignored --nocapture
//! ```

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use omp_desktop::dto::{BranchTarget, SessionStatus, ThreadSnapshot};
use omp_desktop::flows;
use omp_desktop::session::{open, LiveSession};
use omp_desktop::threads::Threads;
use omp_store::Store;
use omp_transport::protocol::{self, commands};
use omp_transport::{ClientOptions, SidecarSpec};
use support::{wait_for, RecordingSink};

fn options() -> ClientOptions {
    ClientOptions {
        request_timeout: Duration::from_secs(60),
        ..Default::default()
    }
}

/// This test's own directory: the workspace, the relocated session store, and the app's
/// config directory, all under one path that is removed when the run ends.
///
/// Named for the process so a run that dies before its cleanup cannot make the next one
/// pass on stale state — and so two runs cannot fight over one bucket.
fn root() -> PathBuf {
    let path = std::env::temp_dir().join(format!("omp-desktop-flows-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    path
}

/// The config overlay the handoff needs — see the module docs for why.
fn write_overlay(root: &Path) -> PathBuf {
    let path = root.join("overlay.yml");
    std::fs::write(
        &path,
        "# The compaction tail a test-sized conversation can fill, set to nothing so a\n\
         # handoff has something to summarize.\n\
         compaction:\n  keepRecentTokens: 1\n",
    )
    .expect("the overlay is written");

    path
}

/// The status `thread_status` answers with: the registry lookup plus the session's own
/// snapshot, which is the whole body of that command.
fn thread_status(threads: &Threads, thread: &str) -> SessionStatus {
    threads
        .get(thread)
        .expect("the thread is registered")
        .status()
        .expect("the status is readable")
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
            Some(Duration::from_secs(60)),
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

/// The session file the engine is writing, straight from `get_state`.
async fn session_file(live: &LiveSession) -> String {
    let client = live.client().expect("the session is open");
    let state = client
        .request(commands::get_state(), None)
        .await
        .expect("get_state is answered");
    let file = state["data"]["sessionFile"]
        .as_str()
        .expect("the session is persisted");

    file.to_string()
}

/// The session's artifacts directory: its file's path with `.jsonl` removed (the engine's
/// own rule).
fn artifacts_dir(session_file: &str) -> PathBuf {
    PathBuf::from(
        session_file
            .strip_suffix(".jsonl")
            .expect("the session file ends in .jsonl"),
    )
}

/// Every file under `dir`, sorted — empty when the directory does not exist.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    files.sort();

    files
}

/// The rows' text, for comparing a transcript with what was asked.
fn texts(live: &LiveSession) -> Vec<String> {
    live.rows()
        .expect("the transcript is readable")
        .into_iter()
        .map(|row| row.text)
        .collect()
}

#[tokio::test]
#[ignore = "spawns a real `omp` sidecar, needs provider credentials, and costs model calls"]
async fn the_session_flows_against_a_real_engine() {
    let root = root();
    let workspace = root.join("workspace");
    let agent = root.join("agent");
    let sessions = agent.join("sessions/flows");
    let config = root.join("config");
    for dir in [&workspace, &config] {
        std::fs::create_dir_all(dir).expect("a test directory");
    }

    // The catalogue over the relocated store, exactly as the app builds it (from the agent
    // directory it discovered); here it points at this test's own.
    let store = Store::at(&agent);

    let threads = Arc::new(Threads::default());
    let sink = RecordingSink::default();
    let spec = SidecarSpec::omp(&workspace)
        // Every session this run creates lands here, and is removed with the directory.
        .with_arg("--session-dir", sessions.to_str().expect("a utf-8 path"))
        .with_arg(
            "--config",
            write_overlay(&root).to_str().expect("a utf-8 path"),
        );

    let live = open(&spec, options(), sink.clone(), threads.clone())
        .await
        .expect("the sidecar opens");
    threads.insert(Arc::clone(&live));
    let id = live.thread.current();
    println!("session: {id}");
    println!("session file: {}", session_file(&live).await);

    // ---- 1. Rename --------------------------------------------------------
    // The name is trimmed in the host, which is what makes the assertion below the
    // engine's own value rather than an echo of what was sent.
    flows::rename(&live, "  flows, measured  ")
        .await
        .expect("the rename is accepted");

    let status = thread_status(&threads, &id);
    println!("name after rename: {:?}", status.control.session_name);
    assert_eq!(
        status.control.session_name.as_deref(),
        Some("flows, measured"),
        "the re-read state must carry the new name: the engine emits nothing for a rename"
    );

    // The engine's sentence, refused before it is ever sent.
    let refused = flows::rename(&live, "   ")
        .await
        .expect_err("an empty name is refused");
    println!("empty name refused: {refused}");
    assert_eq!(refused, "Session name cannot be empty");
    assert_eq!(
        thread_status(&threads, &id).control.session_name.as_deref(),
        Some("flows, measured"),
        "a refused rename must not have moved the name"
    );

    // ---- 2. Fork ----------------------------------------------------------
    // Two turns, so the fork at the second message has a history to keep (and, later, a
    // conversation the handoff can actually summarize).
    say(&live, "Reply with exactly: alpha").await;
    say(&live, "Reply with exactly: beta").await;

    let targets: Vec<BranchTarget> = flows::branch_targets(&live)
        .await
        .expect("the fork targets are answered");
    println!("fork targets: {targets:?}");
    assert_eq!(
        targets.len(),
        2,
        "both user messages must be fork points: {targets:?}"
    );
    assert!(
        targets[0].text.contains("alpha") && targets[1].text.contains("beta"),
        "the targets must be the user messages, in order: {targets:?}"
    );

    // A message the engine cannot branch at is refused in the host rather than sent: the
    // handler throws on an unknown id, and a throw is not an answer.
    let bogus = flows::branch(&threads, &sink, &live, "not-an-entry").await;
    println!(
        "bad entry id refused: {}",
        bogus.as_ref().expect_err("a bad id is refused")
    );
    assert!(bogus.is_err());

    let forked: ThreadSnapshot = flows::branch(&threads, &sink, &live, &targets[1].entry_id)
        .await
        .expect("the fork is accepted");
    let forked_id = forked.id.clone();
    println!("forked: {id} -> {forked_id}");
    assert_ne!(forked_id, id, "a fork mints a new session id");
    assert!(
        threads.get(&id).is_none(),
        "the id that was branched from must stop being addressable"
    );
    assert!(
        threads.get(&forked_id).is_some(),
        "the new id must be the registry's, or nothing can be said to the forked thread"
    );
    assert_eq!(live.thread.current(), forked_id);
    assert_eq!(
        thread_status(&threads, &forked_id).control.session_id,
        forked_id,
        "the engine's own state must agree with the id the registry moved to"
    );
    assert!(
        sink.roster().iter().any(|row| row.id == forked_id),
        "the roster the sidebar reads must carry the new id: {:?}",
        sink.roster()
    );

    // The transcript is the fork's, not the one the sidecar had before: the engine
    // replaces its own messages and emits no replay, so this can only be the hydration.
    let rows = texts(&live);
    println!("rows after fork: {rows:?}");
    assert!(
        rows.iter().any(|text| text.contains("alpha")),
        "the fork keeps the history it forked from: {rows:?}"
    );
    assert!(
        !rows.iter().any(|text| text.contains("beta")),
        "the fork drops the message it forked at: {rows:?}"
    );

    let forked_file = session_file(&live).await;
    println!("forked session file: {forked_file}");
    assert!(
        forked_file.contains(&forked_id),
        "the new session's file is its own: {forked_file}"
    );

    // ---- 3. Handoff -------------------------------------------------------
    let artifacts = artifacts_dir(&forked_file);
    println!(
        "artifacts dir: {} ({:?})",
        artifacts.display(),
        files_under(&artifacts)
    );

    let before = files_under(&artifacts);
    flows::handoff(&live, Some("Summarize what was set up, briefly."))
        .await
        .expect("the handoff commits");
    let after = files_under(&artifacts);
    println!("handoff: artifacts before {before:?}, after {after:?}");
    assert_eq!(
        after, before,
        "measured: the RPC handoff commits a compaction entry and writes **no** file"
    );

    let context = thread_status(&threads, &forked_id).control.context;
    println!("context after handoff: {context:?}");
    assert!(
        context.is_some(),
        "the handoff must have left a readable context snapshot behind it"
    );

    // ---- 4. Export --------------------------------------------------------
    let exported = flows::export(&live, Some(config.as_path()))
        .await
        .expect("the export is written");
    println!("exported to {exported}");

    let path = PathBuf::from(&exported);
    let exports =
        flows::exports_dir(Some(config.as_path())).expect("the app has an export directory");
    assert!(path.is_absolute(), "{exported} must be absolute");
    assert!(path.exists(), "{exported} must have been written");
    assert!(
        path.starts_with(&exports),
        "{exported} must be under {}",
        exports.display()
    );

    // The same path through the allowlist `open_path` uses, and a path beside it that it
    // must refuse. `permit` rather than `open`: a test that launched a browser to check an
    // allowlist would be a test nobody could run.
    let allowlist = flows::Allowlist::collect(Some(&store), &threads, Some(exports.clone()));
    assert!(
        allowlist.permit(&exported).is_ok(),
        "an export must be openable by the app that wrote it"
    );
    assert!(
        allowlist.permit("/etc/hosts").is_err(),
        "a path the app is not showing must not be openable"
    );

    // ---- 5. Delete --------------------------------------------------------
    let refusal = flows::delete(Some(&store), &threads, &forked_id)
        .expect_err("a live session cannot be deleted");
    println!("delete refused while live: {refusal}");
    assert!(refusal.contains("open in this window"), "{refusal}");

    // Close it the way the window does, then delete what is left.
    let closed = threads
        .remove(&forked_id)
        .expect("the thread is registered");
    closed.shutdown(Duration::from_secs(10)).await;

    let session = store
        .find(&forked_id)
        .expect("the catalogue lists the session the test created");
    println!("deleting {}", session.path.display());
    assert!(session.path.exists());

    flows::delete(Some(&store), &threads, &forked_id).expect("the delete removes it");

    assert!(
        !session.path.exists(),
        "{} must be gone",
        session.path.display()
    );
    assert!(
        !artifacts.exists() || files_under(&artifacts).is_empty(),
        "the artifacts directory must not outlive the session"
    );
    assert!(
        store.find(&forked_id).is_none(),
        "the catalogue must not list a deleted session"
    );

    // The app's own store was never the engine's; everything this run wrote is here.
    let _ = std::fs::remove_dir_all(&root);
    println!(
        "done: {forked_id} deleted, artifacts at {}",
        artifacts.display()
    );
}
