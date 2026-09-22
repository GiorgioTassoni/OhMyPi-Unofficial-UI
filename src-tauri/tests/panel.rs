//! The right panel's data, against a real engine (`docs/12` §8).
//!
//! Step 10's host half in one test: the three sources the panel reads and the one thing it
//! writes, observed end to end rather than reasoned about. What it asserts, in order:
//!
//! 1. **The plan.** `set_todos` seeds one, and what comes back is the engine's own list —
//!    compared against a *re-read* of `get_state`, because "the answer is the answer and not
//!    an echo" is only provable by asking twice. The panel's rendering of it
//!    (`ControlSnapshot.todoPhases`) is compared too. Measured here and printed: what the
//!    engine does to a phase it did not write (it projects, it does not normalise).
//! 2. **The tree.** One level of a temporary directory holding a directory and two files:
//!    directories first, cold-cased by name, sizes and timestamps measured rather than
//!    guessed — and a path outside that directory refused.
//! 3. **A real spilled artifact.** The agent is asked to run `seq 1 200000` through the
//!    `bash` tool, which is far past the engine's inline limit: the card carries a
//!    `meta.truncation` record with an id, and the file beside the session holds the whole
//!    output the card cut short. This is M5's "an artifact spills and is viewable".
//! 4. **The closed thread.** With the sidecar gone, the plan is still answered — from the
//!    control the registry remembered — and the session file a closed thread's artifacts
//!    live beside is still known.
//!
//! Isolation, because a live engine writes: the sidecar is given `--session-dir`, so every
//! session this test creates lands in a temp directory that is removed at the end, and the
//! app's `Store` is pointed at the same temp agent directory. The engine's *configuration* —
//! models, credentials, settings — stays where the user put it and is only read.
//!
//! Ignored by default: it spawns a real agent, needs provider credentials and network, and
//! costs a model call that has to actually run a command.
//!
//! ```text
//! cargo test -p omp-desktop --test panel -- --ignored --nocapture
//! ```

mod support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use omp_desktop::dto::RowSnapshot;
use omp_desktop::panel;
use omp_desktop::session::{open, LiveSession};
use omp_desktop::threads::Threads;
use omp_store::Store;
use omp_transport::protocol::{self, commands};
use omp_transport::{ClientOptions, SidecarSpec};
use serde_json::{json, Value};
use support::{say, RecordingSink};

/// The workspace the tree is asserted against: a directory and two files, whose contents
/// are here so the sizes the listing reports are compared with what was written.
const README: &str = "# panel\n";
const SOURCE: &str = "fn main() {}\n";

/// How many lines the command the agent is asked to run prints.
///
/// The command is `seq 1 <this>`, and the count is the point: it is far past any inline
/// limit the engine can have, so the result cannot be shown whole in a card.
const FLOOD_LINES: u64 = 200_000;

/// A flood that still spills but fits the host's read cap.
///
/// The window between the two is what this is: past the engine's inline limit (~50 KiB
/// measured on the flood above, which kept 8 709 of 200 001 lines) and under
/// [`omp_desktop::panel::MAX_ARTIFACT_BYTES`], so the artifact is handed over *whole*.
const CONTAINED_LINES: u64 = 20_000;

fn options() -> ClientOptions {
    ClientOptions {
        request_timeout: Duration::from_secs(60),
        ..Default::default()
    }
}

/// This test's own directory: the workspace, the relocated session store, and the agent
/// directory the catalogue reads, all under one path that is removed when the run ends.
fn root() -> PathBuf {
    let path = std::env::temp_dir().join(format!("omp-desktop-panel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    path
}

/// The `seq 1 200000` output, as the file the engine spills should hold it.
///
/// Generated rather than read back, so the assertion is against what the command *must* have
/// printed: comparing the artifact with the card would prove only that they agree with each
/// other, and the card is the thing under test. `seq` writes one number per line, newline
/// terminated, and nothing else.
fn flood_output(lines: u64) -> String {
    let mut text = String::with_capacity(lines as usize * 7);
    for number in 1..=lines {
        text.push_str(&number.to_string());
        text.push('\n');
    }

    text
}

/// The truncation record a spilled tool row carries, as the card parser reads it.
///
/// The path is the one `frontend/src/lib/toolView.ts` walks (`details.meta.truncation`), and
/// the tool's own output is returned beside it: the artifact has to be *more* than this text
/// for the panel to be worth opening.
fn spill(rows: &[RowSnapshot]) -> Option<(String, Value, String)> {
    rows.iter().rev().find_map(|row| {
        let tool = row.tool.as_ref()?;
        if !tool.finished || tool.details.is_empty() {
            return None;
        }

        let details: Value = serde_json::from_str(&tool.details).ok()?;
        let record = details.get("meta")?.get("truncation")?.clone();
        let id = record.get("artifactId").and_then(Value::as_str)?;
        if id.is_empty() {
            return None;
        }

        Some((id.to_string(), record, tool.output.clone()))
    })
}

/// Ask the agent to run `seq 1 <lines>` through the `bash` tool, and answer with the
/// truncation record of the result, the card's own text, and what the command must have
/// printed.
///
/// The command is spelled out because the test's subject is the *engine's* truncation, not
/// the agent's judgement: a model that piped this into `wc -l` would produce a small result
/// and no artifact at all. Two attempts are allowed for exactly that reason — the failure
/// mode is the model's choice, not the host's behaviour.
async fn flooded(live: &LiveSession, lines: u64) -> (String, Value, String, String) {
    for attempt in 1..=2 {
        say(
            live,
            &format!(
                "Run exactly this shell command with the bash tool, with no pipes, no \
                 redirection, no `wc` and no other command: `seq 1 {lines}`. Then reply with \
                 only the last number it printed."
            ),
        )
        .await;

        let rows = live.rows().expect("the transcript is readable");
        if let Some((id, record, card)) = spill(&rows) {
            return (id, record, card, flood_output(lines));
        }

        println!("attempt {attempt}: the agent did not spill anything; asking again");
    }

    panic!("the agent must have run `seq 1 {lines}` through the bash tool: that output is far past the engine's inline limit");
}

#[tokio::test]
#[ignore = "spawns a real `omp` sidecar, needs provider credentials, and costs a model call"]
async fn the_panels_three_sources_against_a_real_engine() {
    let root = root();
    let workspace = root.join("workspace");
    let nested = workspace.join("src");
    let agent = root.join("agent");
    let sessions = agent.join("sessions/panel");
    for dir in [&workspace, &nested] {
        std::fs::create_dir_all(dir).expect("a test directory");
    }
    // Two files and a directory, which is what the level below is asserted against.
    std::fs::write(workspace.join("README.md"), README).expect("a file");
    std::fs::write(nested.join("main.rs"), SOURCE).expect("a file");

    let store = Store::at(&agent);
    let threads = Arc::new(Threads::default());
    let spec = SidecarSpec::omp(&workspace)
        // Every session this run creates lands here, and is removed with the directory.
        .with_arg("--session-dir", sessions.to_str().expect("a utf-8 path"))
        // This test is about the panel's data, not the gate in front of it: the app's own
        // launch is `--approval-mode write`, which asks before an exec, and an unanswered
        // dialog would stall the turn this test needs to finish. `approval.rs` is where the
        // asking is asserted.
        .with_approval_mode("yolo");

    let live = open(&spec, options(), RecordingSink::default(), threads.clone())
        .await
        .expect("the sidecar opens");
    threads.insert(Arc::clone(&live));
    let id = live.thread.current();
    let session_file = live.session_file().expect("the session is persisted");
    println!("session: {id}");
    println!("session file: {session_file}");

    // When the catalogue can see a *live* session, which is not the same question as
    // whether the session exists: the engine owns when the file lands. Printed rather than
    // asserted, because what the host needs is the answer for a *closed* thread (asserted
    // below) and this is the measurement that says the two are not the same thing.
    let mut waited = 0;
    while store.find(&id).is_none() && waited < 30 {
        waited += 1;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    println!(
        "the catalogue {} the live session at the handshake",
        match waited {
            0 => "already listed".to_string(),
            waited if waited >= 30 => "had not listed after 3 s of polling".to_string(),
            waited => format!("listed after {} ms of polling", waited * 100),
        }
    );

    // ---- 1. The plan ------------------------------------------------------
    // The write half of `set_todos`: `panel::outgoing` is the host's only rule (an empty
    // list is refused), and what is asserted here is the engine's answer to the rest.
    let seeded = panel::outgoing(&[omp_desktop::dto::TodoPhaseInput {
        name: "Measurement".to_string(),
        tasks: vec![
            omp_desktop::dto::TodoTaskInput {
                content: "seed the plan".to_string(),
                status: "completed".to_string(),
                blocker: None,
            },
            omp_desktop::dto::TodoTaskInput {
                content: "read it back".to_string(),
                status: "in_progress".to_string(),
                blocker: None,
            },
        ],
    }])
    .expect("a plan with a phase in it is sent");

    let stored = live
        .set_todo_phases(seeded)
        .await
        .expect("the engine stores the plan");
    println!("set_todos answered: {stored:?}");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].name, "Measurement");
    assert_eq!(stored[0].tasks.len(), 2);
    assert_eq!(stored[0].tasks[0].content, "seed the plan");
    assert_eq!(stored[0].tasks[1].status, "in_progress");

    // The answer is only worth rendering if it is what the engine *holds*, so ask again by
    // the other road: `thread_todos` re-reads `get_state`, and it must agree.
    let reread = live.todo_phases().await;
    assert_eq!(
        panel::phases(&reread),
        panel::phases(&stored),
        "a re-read must agree with the answer, or the panel is rendering a copy"
    );

    // And the control snapshot the window renders carries the same list, whole.
    let control = live.status().expect("status").control;
    assert_eq!(control.todo_phases, panel::phases(&stored));

    // What the engine does with a plan it did not write. Measured rather than assumed, and
    // printed: it stores what it is given (no normalisation) and answers with its own
    // projection of it, so a task can come back *without* the status it was sent.
    let client = live.client().expect("the session is open");
    let response = client
        .request(
            commands::set_todos(json!([{
                "name": "Raw",
                "tasks": [
                    { "content": "no status", "id": 7, "note": "extra" },
                    { "content": "with a blocker", "status": "blocked", "blocker": "the review" },
                ],
            }])),
            None,
        )
        .await
        .expect("the engine answers set_todos");
    assert!(protocol::is_success(&response), "refused: {response}");
    let echoed = &response["data"]["todoPhases"];
    println!("the engine's answer to a raw plan: {echoed}");
    assert_eq!(echoed[0]["name"], json!("Raw"));
    let task = echoed[0]["tasks"][0]
        .as_object()
        .expect("a task comes back as an object");
    assert_eq!(task.get("content"), Some(&json!("no status")));
    assert!(
        !task.contains_key("id") && !task.contains_key("note"),
        "the engine answers with its own projection and drops what it does not know: {task:?}"
    );
    assert_eq!(echoed[0]["tasks"][1]["blocker"], json!("the review"));
    println!("a task sent with no status came back as {task:?}");

    // Put a real plan back, which is also what the next section reads off the control.
    let restored = live
        .set_todo_phases(json!([{
            "name": "Measurement",
            "tasks": [{ "content": "read it back", "status": "in_progress" }],
        }]))
        .await
        .expect("the plan is restored");
    assert_eq!(restored[0].tasks.len(), 1);

    // ---- 2. The tree ------------------------------------------------------
    let level = panel::tree(&live.workspace, None).expect("the workspace lists");
    println!(
        "workspace level: {:?}",
        level.iter().map(|entry| &entry.name).collect::<Vec<_>>()
    );
    let names: Vec<&str> = level.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["src", "README.md"],
        "directories first, then the files, case-insensitively by name"
    );

    let directory = &level[0];
    assert!(directory.is_dir);
    assert_eq!(directory.size, 0, "a directory has no size to draw");
    assert!(
        directory.path.starts_with(&live.workspace),
        "a row's path is inside the workspace: {}",
        directory.path
    );

    let file = &level[1];
    assert!(!file.is_dir);
    assert_eq!(file.size, README.len() as u64, "the size is the file's own");
    assert!(file.modified_at > 0, "a real file has a timestamp");

    // The path a row carries is the path the next level is asked for — the panel passes it
    // straight back, so a listing that could not be re-listed would be a dead row.
    let below = panel::tree(&live.workspace, Some(&directory.path)).expect("the next level");
    assert_eq!(below.len(), 1);
    assert_eq!(below[0].name, "main.rs");
    assert_eq!(below[0].size, SOURCE.len() as u64);

    // Everything outside the thread's workspace is refused, including the directory the
    // workspace sits in — that is the whole of the containment rule.
    let outside = &root.to_string_lossy().to_string();
    let refused = panel::tree(&live.workspace, Some(outside))
        .expect_err("a path outside the workspace is refused");
    println!("refused: {refused}");
    assert!(
        refused.contains("outside the thread's workspace"),
        "{refused}"
    );

    // ---- 3. A real spilled artifact ---------------------------------------
    // The id names a file beside the session's own `.jsonl`, which is the engine's rule and
    // the whole reason the host can find it without an RPC command.
    let directory = panel::artifacts_dir(&session_file).expect("the engine's rule");

    let (artifact_id, record, card_text, expected) = flooded(&live, FLOOD_LINES).await;
    println!("truncation record: {record}");
    println!("the card kept {} bytes of text", card_text.len());

    let read = panel::artifact(&directory, &artifact_id).expect("the artifact is read");
    println!(
        "artifact {} : {} bytes at {} (truncated in the read: {})",
        read.id, read.bytes, read.path, read.truncated
    );
    println!("the whole result is {} bytes", expected.len());

    assert_eq!(read.id, artifact_id);
    assert!(
        std::path::Path::new(&read.path).starts_with(&directory),
        "the file read is inside the session's artifacts directory: {}",
        read.path
    );
    // The artifact is the *complete* output, which is what the card could not show. The
    // record's own account of the stream is the engine's, and it must match the file.
    assert_eq!(
        read.bytes,
        expected.len() as u64,
        "the spilled file *is* the whole output the card cut short: byte for byte, not a \
         summary of it"
    );
    if let Some(total) = record.get("totalBytes").and_then(Value::as_u64) {
        assert_eq!(
            total, read.bytes,
            "the truncation record's own account of the stream is the file it spilled to"
        );
    }
    assert!(
        (card_text.len() as u64) < read.bytes,
        "the card was truncated, the artifact is not: {} bytes of text against {} on disk",
        card_text.len(),
        read.bytes
    );
    // The head of the file is the command's own first lines: a summary or a tail would not
    // start here. (The read is capped at 256 KiB, so a 1.3 MB artifact arrives truncated —
    // and this is the assertion that says the *start* of what arrived is the real output.)
    assert!(
        read.text.starts_with("1\n2\n3\n"),
        "the artifact begins with the command's output, got {:?}",
        read.text.chars().take(40).collect::<String>()
    );
    assert!(
        expected.starts_with(&read.text),
        "everything the read returned is the command's own output, in order ({} bytes read)",
        read.text.len()
    );
    println!(
        "the artifact is the command's whole output: {} bytes on disk, {} generated",
        read.bytes,
        expected.len()
    );
    if read.truncated {
        assert_eq!(
            read.text.len(),
            panel::MAX_ARTIFACT_BYTES as usize,
            "a capped read stops exactly at the cap"
        );
    }
    println!(
        "direction {:?}, truncatedBy {:?}, totalLines {:?}, outputLines {:?}, elidedBytes {:?}",
        record.get("direction"),
        record.get("truncatedBy"),
        record.get("totalLines"),
        record.get("outputLines"),
        record.get("elidedBytes"),
    );

    // ---- 4. An artifact the host can hand over whole -----------------------
    // The engine spilled this one too — the card was cut short — but the file fits the
    // host's cap, so what the panel shows *is* the whole result: the same claim the section
    // above makes about the file's size, made about its text.
    let (contained_id, record, card_text, expected) = flooded(&live, CONTAINED_LINES).await;
    println!("contained artifact {contained_id}: record {record}");

    let contained = panel::artifact(&directory, &contained_id).expect("the artifact is read");
    println!(
        "{} bytes on disk, {} of text, truncated in the read: {}",
        contained.bytes,
        contained.text.len(),
        contained.truncated
    );
    assert_eq!(contained.bytes, expected.len() as u64);
    assert!(
        !contained.truncated,
        "the whole file is inside the cap, so the flag must not say otherwise"
    );
    assert_eq!(
        contained.text, expected,
        "the text is the command's output, character for character"
    );
    assert!(
        (card_text.len() as u64) < contained.bytes,
        "the card was still the truncated one: {} bytes of text against {} on disk",
        card_text.len(),
        contained.bytes
    );

    // ---- 5. The closed thread --------------------------------------------
    // The same question once the session has done some work: printed, because the two
    // answers are the point — a catalogue scan is a question about the engine's flush
    // schedule while a session is running, and about the file itself once it has closed.
    println!(
        "the catalogue lists the live session after two turns: {}",
        store.find(&id).is_some()
    );

    let live_plan = live.status().expect("status").control.todo_phases;
    threads.remove(&id).expect("the thread is registered");
    live.shutdown(Duration::from_secs(10)).await;

    // The session's own record of where it worked, which is what the tree is rooted at once
    // the sidecar is gone (and the only thing that outlives it). Read after the shutdown
    // rather than before it: a catalogue scan of a *running* session is a scan of a file the
    // engine is still deciding when to write.
    let catalogued = store
        .find(&id)
        .expect("the catalogue lists the session once it has closed");
    assert_eq!(
        catalogued.cwd, live.workspace,
        "the header's cwd is the workspace the panel browses"
    );

    let cached = threads
        .cached_control(&id)
        .expect("a closed thread is remembered, or the panel would go blank");
    assert_eq!(
        panel::phases(&cached.todo_phases),
        live_plan,
        "the plan a closed thread answers with is the one the engine last held"
    );
    assert_eq!(
        cached.session_file.as_deref(),
        Some(session_file.as_str()),
        "and its artifacts are still resolvable through the remembered session file"
    );
    assert!(
        threads.cached_control("01never-opened").is_none(),
        "a thread this window never opened has no plan to answer with"
    );

    // `read_artifact` for a closed thread resolves the same directory: nothing about the
    // read depended on the sidecar being alive.
    let again = panel::artifact(&directory, &artifact_id).expect("the artifact still reads");
    assert_eq!(again, read);

    let _ = std::fs::remove_dir_all(&root);
    println!(
        "done: plan, tree and artifact {} all read back",
        artifact_id
    );
}
