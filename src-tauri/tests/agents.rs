//! The agents panel's data, against a real engine (`docs/12` §9).
//!
//! Step 11's host half in one test. Everything here is a wire shape the engine owns and
//! nothing in this app could have guessed, so each assertion is a measurement rather than a
//! reading of the engine's source:
//!
//! 1. **The roster, pushed.** The subscription is asked for at open, and a real `task` spawn
//!    is what makes the frames arrive: a row appears with the engine's own status word, and
//!    the `sessionFile` its transcript lives at.
//! 2. **The transcript by cursor.** `get_subagent_messages` serves it while the engine still
//!    owns the id, and the second call from `nextByte` is the delta — the pair is what makes
//!    a pane that appends rather than re-reads possible.
//! 3. **What survives settling.** The engine's registry deletes a subagent the moment it
//!    finishes, which is the fact this whole panel is built around: asserted here by
//!    polling `get_subagents` *after* the spawn ends and seeing the row the frames built
//!    still there, marked as no longer listed rather than given a status nobody reported.
//! 4. **The parked transcript on disk.** The file the frames named is where the engine says
//!    it is, and the scan finds it by the id the parent's own card carries.
//! 5. **A real background job.** The engine's auto-background threshold is lowered for this
//!    run through `--config`, so a `bash` call that outlives it is backgrounded: the card
//!    says so and names the job, and the job's result arrives later as a turn of its own.
//!    The panel derives its job rows from exactly that card, so this is that surface's
//!    oracle.
//! 6. **Broker processes.** `omp ps --json` parses, and every daemon it reports has the
//!    fields the panel renders.
//!
//! Isolation matches `panel.rs`: `--session-dir` puts every session this run creates in a
//! temp directory that is removed at the end, and `--config` points at an overlay written
//! for this run rather than at the user's own settings.
//!
//! Ignored by default: it spawns a real agent, needs provider credentials and network, and
//! costs model calls that have to actually run a command and spawn a subagent.
//!
//! ```text
//! cargo test -p omp-desktop --test agents -- --ignored --nocapture
//! ```

mod support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use omp_desktop::agents;
use omp_desktop::dto::AgentSnapshot;
use omp_desktop::session::{agent_snapshot, open};
use omp_desktop::threads::Threads;
use omp_store::Store;
use omp_transport::{ClientOptions, SidecarSpec};
use serde_json::Value;
use support::{say, wait_for, RecordingSink};

/// How long the subagent is asked to live, so the roster has something to report.
///
/// Long enough that a poll during the spawn cannot miss it, short enough that the test is
/// not waiting on a sleep of its own making.
const SUBAGENT_SLEEP: u64 = 20;

/// The command the background-job section runs, and the word that must come back later.
const JOB_COMMAND: &str = "sleep 25; echo done-sleeping";

/// The auto-background threshold this run forces, in the config overlay.
///
/// The engine's default is 60 s, and in RPC mode it re-applies that default unless the path
/// was explicitly configured — a `--config` overlay counts as configured, which is why the
/// overlay can lower it at all (`main.ts`, `RPC_BACKGROUND_DEFAULTED_SETTING_PATHS`).
const BACKGROUND_THRESHOLD_MS: u64 = 3000;

fn options() -> ClientOptions {
    ClientOptions {
        // A spawning turn plus a backgrounded job's own delivery: the default is tighter
        // than a subagent's first token.
        request_timeout: Duration::from_secs(120),
        ..Default::default()
    }
}

fn root() -> PathBuf {
    let path = std::env::temp_dir().join(format!("omp-desktop-agents-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);

    path
}

/// The row for one agent id, from a roster snapshot.
fn row<'a>(roster: &'a [AgentSnapshot], id: &str) -> Option<&'a AgentSnapshot> {
    roster.iter().find(|agent| agent.id == id)
}

#[tokio::test]
#[ignore = "spawns a real `omp` sidecar, needs provider credentials, and costs model calls"]
async fn the_roster_a_transcript_and_a_job_against_a_real_engine() {
    let root = root();
    let workspace = root.join("workspace");
    let agent_dir = root.join("agent");
    let sessions = agent_dir.join("sessions/agents");
    std::fs::create_dir_all(&workspace).expect("a test workspace");

    // The one setting this run changes, in the engine's own overlay format. Nothing else is
    // touched: the user's settings are read, never written.
    let overlay = root.join("config.yml");
    std::fs::write(
        &overlay,
        format!("bash:\n  autoBackground:\n    thresholdMs: {BACKGROUND_THRESHOLD_MS}\n"),
    )
    .expect("a config overlay");

    let _store = Store::at(&agent_dir);
    let threads = Arc::new(Threads::default());
    let spec = SidecarSpec::omp(&workspace)
        .with_arg("--session-dir", sessions.to_str().expect("a utf-8 path"))
        .with_arg("--config", overlay.to_str().expect("a utf-8 path"))
        // This test is about the roster, not the gate in front of it: a subagent runs
        // `bash`, and an unanswered dialog would stall the spawn it needs to observe.
        .with_approval_mode("yolo");

    let sink = RecordingSink::default();
    let live = open(&spec, options(), sink.clone(), threads.clone())
        .await
        .expect("the sidecar opens");
    threads.insert(Arc::clone(&live));
    let id = live.thread.current();
    println!("session: {id}");
    assert_eq!(
        live.agents_error, None,
        "the engine must accept the subscription this app asks for at open"
    );

    // ---- 1. The roster, pushed --------------------------------------------
    say(
        &live,
        &format!(
            "Run the task tool exactly once, with a single task: agent name RosterProbe, task \
             `sleep {SUBAGENT_SLEEP} in bash, then reply with the word finished`. Wait for it \
             and then reply with only its result."
        ),
    )
    .await;

    // The frames arrive while the spawn runs, so the *published* rosters are what prove the
    // pump routed them at all — a poll could be satisfied by `get_subagents` alone, and the
    // frame path is the one that still works once the engine has pruned its registry.
    let rosters = wait_for("a subagent to appear in the published roster", || {
        let rosters = sink.agent_rosters(&id);
        rosters
            .iter()
            .any(|roster| !roster.is_empty())
            .then_some(rosters)
    })
    .await;

    let running = rosters
        .iter()
        .find_map(|roster| {
            roster
                .iter()
                .find(|agent| agent.status.as_deref() == Some("running"))
        })
        .expect("a roster published while the agent was running");
    let agent = running.id.clone();
    println!("agent: {agent}");
    println!(
        "running row: listed={} detached={} sessionFile={:?} parentToolCallId={:?}",
        running.listed, running.detached, running.session_file, running.parent_tool_call_id
    );
    // Printed because it is a number a future change could regret: the pump publishes on
    // every roster change, and the engine reports progress often enough that one 20-second
    // spawn produced this many. A row is a few hundred bytes, so a whole roster is cheap —
    // but the count is the measurement, not an assumption.
    println!("rosters published for this thread: {}", rosters.len());

    assert!(running.listed, "the engine is reporting it");
    assert!(
        running.session_file.is_some(),
        "the frames must carry the transcript's own file"
    );
    assert!(
        running.parent_tool_call_id.is_some(),
        "the spawning `task` call is what the panel's jump back into the conversation uses"
    );
    // Measured, and worth recording because the engine's own comment says otherwise: a
    // `task` spawn reports `detached: true` even though the tool call blocks the parent's
    // turn until it returns (`pi-tui`'s `SubagentLifecyclePayload.detached` calls that case
    // unset). The panel shows the engine's word rather than a rule this app invented.
    assert!(
        running.detached,
        "measured: an in-turn `task` spawn carries detached"
    );

    // ---- 2. The transcript by cursor --------------------------------------
    let first = live
        .agent_messages(&agent, 0)
        .await
        .expect("the engine serves a live subagent's transcript");
    println!(
        "engine page: {} rows, nextByte={}, reset={}, file={}",
        first.rows.len(),
        first.next_byte,
        first.reset,
        first.session_file
    );
    assert!(!first.reset, "a first read at zero cannot be a reset");
    assert!(
        first.next_byte > 0,
        "the page must report where to continue"
    );
    assert!(
        !first.rows.is_empty(),
        "a running subagent has already written its spawn entry"
    );

    let delta = live
        .agent_messages(&agent, first.next_byte)
        .await
        .expect("the delta is served");
    assert!(!delta.reset, "the file did not shrink");
    assert!(
        delta.next_byte >= first.next_byte,
        "the cursor only moves forward"
    );
    assert_eq!(
        delta.session_file, first.session_file,
        "the same subagent is one file"
    );

    // ---- 3. What survives settling ----------------------------------------
    // The turn that spawned it is over by the time `say` returned, so the engine has already
    // pruned its registry. Everything below is the asymmetry the panel exists for.
    // Through the same mapping the command uses, so what the window receives is what is
    // asserted rather than the model behind it.
    let after: Vec<AgentSnapshot> = live.agents().await.iter().map(agent_snapshot).collect();
    let settled_row = row(&after, &agent).expect("the row must survive its own completion");
    println!(
        "after settling: status={:?} listed={} ({} reported by the engine)",
        settled_row.status,
        settled_row.listed,
        after.len()
    );
    assert!(
        settled_row
            .status
            .as_deref()
            .is_some_and(|status| status != "running"),
        "a settled agent must carry a terminal status, not the one it had while running"
    );
    assert!(
        !settled_row.listed,
        "the engine stops listing a settled agent, and the panel must be able to say so"
    );
    // The frames are what makes that difference visible: the same row, one poll later, is
    // the only place a settled agent still exists.
    assert_eq!(
        after.len(),
        1,
        "the settled row survives the poll that no longer lists it"
    );

    let still_readable = live
        .agent_messages(&agent, 0)
        .await
        .expect("the engine still serves a settled agent's transcript");
    assert!(!still_readable.rows.is_empty());

    // ---- 4. The parked transcript on disk ---------------------------------
    let session_file = live.session_file().expect("the session is persisted");
    let parked = agents::parked(&session_file).expect("the scan reads the artifacts directory");
    let found = parked
        .iter()
        .find(|row| row.id == agent)
        .unwrap_or_else(|| {
            panic!(
                "the scan must find `{agent}`: it found {:?}",
                parked.iter().map(|row| &row.id).collect::<Vec<_>>()
            )
        });
    println!(
        "parked: {} bytes at {} (cwd={}, parent={:?})",
        found.bytes, found.path, found.cwd, found.parent
    );
    // The engine's own byte cursor and the file agree, which is what makes reading the file
    // a fallback rather than a second opinion.
    assert_eq!(
        found.bytes, first.next_byte,
        "the page's cursor is the file's size: {} bytes on disk",
        found.bytes
    );
    assert!(found.bytes > 0, "a transcript with content");
    assert!(
        found.path.ends_with(&format!("{agent}.jsonl")),
        "the file is named after the id the parent's card carries"
    );
    assert_eq!(
        Some(found.path.clone()),
        row(&after, &agent).and_then(|row| row.session_file.clone()),
        "the file the frames named and the file on disk are the same"
    );
    assert_eq!(
        found.cwd,
        workspace.to_string_lossy(),
        "the subagent ran in the thread's own workspace"
    );
    // Measured, and not what the engine's documentation suggests: even a subagent this
    // session spawned *directly* records `parentSession`, pointing at the parent session
    // file. So the field is always a file path — the thread's own session for a first-level
    // agent, its parent's transcript for a nested one — and the panel nests by comparing it
    // rather than by treating its absence as "top level".
    assert_eq!(
        found.parent.as_deref(),
        Some(session_file.as_str()),
        "a first-level spawn names the session it belongs to"
    );

    // The scan's answer and the engine's page are the same file, which is what makes the
    // fallback a fallback rather than a different view.
    let from_file = omp_session::read_transcript(std::path::Path::new(&found.path), 0)
        .expect("the parked transcript reads");
    assert!(
        !from_file.messages.is_empty(),
        "the parked file holds the same entries the engine served"
    );

    // ---- 5. A real background job -----------------------------------------
    say(
        &live,
        &format!(
            "Run exactly this shell command with the bash tool, with no pipes and no \
             redirection: `{JOB_COMMAND}`. Then reply with only the word done."
        ),
    )
    .await;

    let job = wait_for("the bash call to be backgrounded", || {
        live.rows().ok()?.iter().rev().find_map(|row| {
            let tool = row.tool.as_ref()?;
            if !tool.finished || !tool.output.contains("Backgrounded as job") {
                return None;
            }
            Some((tool.output.clone(), tool.details.clone()))
        })
    })
    .await;
    println!("job card: {}", job.0.trim());
    let details: Value = serde_json::from_str(&job.1).unwrap_or(Value::Null);
    println!(
        "job details.async: {}",
        details.get("async").cloned().unwrap_or(Value::Null)
    );
    assert!(
        details
            .get("async")
            .and_then(|async_| async_.get("jobId"))
            .is_some(),
        "the card must name the job the panel lists"
    );

    // The job's result arrives on its own, as a turn this test never asked for. That it
    // reaches the transcript at all is what tells a reader the job finished.
    wait_for("the backgrounded job's result to be delivered", || {
        live.rows()
            .ok()?
            .iter()
            .any(|row| row.text.contains("done-sleeping"))
            .then_some(())
    })
    .await;

    // ---- 6. Broker processes ----------------------------------------------
    // Run from the repository rather than the temp workspace: the question is whether the
    // engine's own output parses, and a scope with daemons in it is the shape worth parsing.
    let scopes = agents::broker_scopes(env!("CARGO_MANIFEST_DIR"))
        .await
        .expect("`omp ps --json` answers with the JSON this app reads");
    println!(
        "broker scopes: {} ({} daemons)",
        scopes.len(),
        scopes
            .iter()
            .map(|scope| scope.daemons.len())
            .sum::<usize>()
    );
    for scope in &scopes {
        for daemon in &scope.daemons {
            println!(
                "  {} [{}] supervised={} owner={:?} cwd={}",
                daemon.name, daemon.state, daemon.supervised, daemon.owner, daemon.cwd
            );
            assert!(
                !daemon.name.is_empty(),
                "a daemon the panel can stop by name"
            );
            assert!(!daemon.state.is_empty(), "the engine's own lifecycle word");
        }
    }

    live.shutdown(Duration::from_secs(5)).await;
    std::fs::remove_dir_all(&root).ok();
}
