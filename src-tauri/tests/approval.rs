//! The approval boundary, proved without a window.
//!
//! `docs/12` §10 calls this the app's single most important surface (D3), and
//! `docs/14` §1 records why it cannot be built last: a `bash` call **blocks** until
//! the host answers an `extension_ui_request`, so with no responder a write-class
//! tool never runs at all.
//!
//! This drives the app's own responder — [`LiveSession::respond`] — and asserts the
//! two outcomes that matter, on one session, in order:
//!
//! 1. **Deny** leaves the command unrun. The check is the file the command would
//!    have created, because "the engine reported a denial" is not the same claim
//!    as "the command did not execute".
//! 2. **Approve** runs it.
//!
//! It also asserts the dialog reached the window's channel (the sink), that
//! answering removed it from the store, and that neither turn left a process
//! behind.
//!
//! Three further tests pin the "always allow" write, each naming in its own doc
//! comment the measurement it fixes: the project-level config path, the write that
//! only the *next* session obeys, and the merge that leaves other tools' policies
//! alone. Two of them write the user's real global config under a guard, and every
//! test in this file serializes on that file — see `ENGINE_CONFIG`.
//!
//! Ignored by default: it spawns a real agent, needs provider credentials, and
//! reaches the network.
//!
//! ```text
//! cargo test -p omp-desktop --test approval -- --ignored --nocapture
//! ```

mod support;

use std::path::{Path, PathBuf};
use std::time::Duration;

use omp_desktop::dto::UiRequestSnapshot;
use omp_desktop::session::{open, LiveSession};
use omp_transport::protocol::ui::UiResponse;
use omp_transport::protocol::{self, commands};
use omp_transport::{ClientOptions, SidecarSpec};
use serde_json::{Map, Value};
use support::{process_exists, registry, wait_for, RecordingSink};

/// A prompt that makes the agent reach for `bash`, and why it is phrased this way.
///
/// The same shape `live_session.rs` measured: an explicit tool, an exact command,
/// and a short expected answer, so the turn ends instead of wandering. The command
/// is a `touch`, whose effect is checkable afterwards.
fn bash_prompt(file: &Path) -> String {
    format!(
        "Use the bash tool to run exactly `touch {}`, then reply with exactly: done",
        file.display()
    )
}

/// Send a prompt that should trip the approval gate, then answer the dialog it
/// raises with `answer`. Returns the dialog, so the caller can assert on what the
/// engine actually asked — its content is evidence in itself.
async fn run_and_answer(
    live: &LiveSession,
    sink: &RecordingSink,
    prompt: &str,
    answer: impl FnOnce() -> UiResponse,
) -> UiRequestSnapshot {
    let client = live.client().expect("the session is open");
    let accepted = client
        .request(
            commands::prompt(prompt, &[], None),
            Some(Duration::from_secs(30)),
        )
        .await
        .expect("the prompt is accepted");
    assert!(
        protocol::is_success(&accepted),
        "prompt rejected: {accepted}"
    );

    let dialog = wait_for("the agent to ask for approval", || {
        sink.dialogs().into_iter().next()
    })
    .await;
    println!("dialog: {dialog:?}");

    live.respond(&dialog.id, answer())
        .await
        .expect("the answer is written");

    // The answered dialog must leave the store: one that survived its own answer
    // could be answered a second time, and a second write would be matched against
    // whatever dialog came next.
    wait_for("the store to drop the answered dialog", || {
        let pending = live.pending_ui_requests().expect("the store is readable");
        (!pending.iter().any(|request| request.id == dialog.id)).then_some(())
    })
    .await;

    wait_for("the turn to end", || {
        let status = live.status().ok()?;
        (!status.control.is_streaming).then_some(())
    })
    .await;

    dialog
}

#[tokio::test]
#[ignore = "requires a real `omp` binary, provider credentials, and network"]
async fn denying_a_tool_call_prevents_it_and_approving_one_runs_it() {
    let _guard = ENGINE_CONFIG.lock().await;

    let workspace = std::env::temp_dir();
    let denied = workspace.join("omp-desktop-denied.txt");
    let approved = workspace.join("omp-desktop-approved.txt");
    // A previous run must not be able to satisfy the assertions below.
    for path in [&denied, &approved] {
        let _ = std::fs::remove_file(path);
    }

    let sink = RecordingSink::default();
    let spec = SidecarSpec::omp(&workspace).ephemeral();
    let live = open(
        &spec,
        ClientOptions {
            request_timeout: Duration::from_secs(30),
            ..Default::default()
        },
        sink.clone(),
        registry(),
    )
    .await
    .expect("the session opens");

    // ---- deny -------------------------------------------------------------
    let dialog = run_and_answer(&live, &sink, &bash_prompt(&denied), || {
        UiResponse::Value("Deny".into())
    })
    .await;

    assert_eq!(dialog.kind, "select", "the approval arrives as a select");
    assert!(
        dialog.options.contains(&"Approve".to_string())
            && dialog.options.contains(&"Deny".to_string()),
        "the engine supplies its own labels: {:?}",
        dialog.options
    );
    assert!(
        dialog.title.contains("bash"),
        "the dialog names the tool it is asking about: {:?}",
        dialog.title
    );
    assert!(
        dialog.title.contains("touch"),
        "the dialog shows the command, which is what the user is deciding on: {:?}",
        dialog.title
    );
    assert_eq!(
        dialog.timeout_ms, None,
        "an approval carries no deadline, so no countdown applies"
    );

    assert!(
        !denied.exists(),
        "a denied command must not have run, but {} exists",
        denied.display()
    );

    // ---- approve ----------------------------------------------------------
    run_and_answer(&live, &sink, &bash_prompt(&approved), || {
        UiResponse::Value("Approve".into())
    })
    .await;

    assert!(
        approved.exists(),
        "an approved command must have run, but {} does not exist",
        approved.display()
    );

    // ---- the boundary held, and nothing was left running ------------------
    let status = live.status().expect("status");
    println!("counters: {:?}", status.counters);
    assert!(
        status.counters.blocking_ui_requests >= 2,
        "both approvals should have been counted as blocking, saw {:?}",
        status.counters
    );

    let pid = status.sidecar_pid.expect("a pid to watch");
    live.shutdown(Duration::from_secs(10)).await;
    wait_for("the agent to exit", || (!process_exists(pid)).then_some(())).await;

    // The approved turn left a file behind; leave the temp dir as it was found.
    let _ = std::fs::remove_file(&approved);
}

/// The engine binary this test drives, resolved exactly as the app resolves it.
///
/// Not `omp` from the `PATH`: the app and the test must fail together if the
/// sidecar is not where it is expected.
fn omp() -> PathBuf {
    omp_transport::resolve_omp_binary()
}

/// The engine's *global* config — the file the app's write path reaches.
fn global_config_path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME is set, as the engine also needs it");
    PathBuf::from(home)
        .join(".omp")
        .join("agent")
        .join("config.yml")
}

/// The engine's global config is one process-wide resource, and this is its lock.
///
/// Two of the tests below write it, and **every** test here asks a question the
/// engine answers when it constructs its session — so a mutation running alongside
/// another test would decide that test's outcome. `always allow bash` turns the
/// denying test's expected dialog into a two-minute timeout, and a concurrent
/// restore makes an assertion of silence vacuous. Each test takes the lock and
/// releases it only after the config is back (the guards' `drop`, which the tests
/// below do explicitly), so the order the tests run in cannot matter. It costs
/// wall-clock time, not correctness, and this suite is minutes either way.
static ENGINE_CONFIG: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// What the engine's config records for `tools.approval` right now.
///
/// Read through the engine's own CLI, and deliberately **not** through
/// [`omp_desktop::policy`]: a check that reuses the code under test would pass even
/// if that code wrote nothing the engine can read back.
fn recorded_policies() -> Map<String, Value> {
    let output = std::process::Command::new(omp())
        .args(["config", "get", "tools.approval", "--json"])
        .output()
        .expect("the engine's config CLI runs");
    assert!(
        output.status.success(),
        "`omp config get` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let parsed: Value =
        serde_json::from_slice(&output.stdout).expect("the engine prints JSON for `--json`");
    parsed
        .get("value")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

/// Write the whole policy record back, the way the CLI insists on.
fn set_policies(record: &Map<String, Value>) {
    let payload = Value::Object(record.clone()).to_string();
    let output = std::process::Command::new(omp())
        .args(["config", "set", "tools.approval", &payload])
        .output()
        .expect("the engine's config CLI runs");
    assert!(
        output.status.success(),
        "`omp config set tools.approval {payload}` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The user's real engine config, put back however the test ends.
///
/// The two tests below exercise the real write path, which reaches the *global*
/// config — the one the user's own sessions read. A guard rather than a call at the
/// end of the test, so a failing assertion or a panic still restores it; `None`
/// means there was no file, and that the one the test caused must go.
struct RestoreGlobalConfig {
    path: PathBuf,
    original: Option<Vec<u8>>,
}

impl RestoreGlobalConfig {
    fn new() -> Self {
        let path = global_config_path();
        let original = std::fs::read(&path).ok();
        println!(
            "[approval] guarding {} ({} bytes read up front)",
            path.display(),
            original.as_ref().map_or(0, Vec::len)
        );
        Self { path, original }
    }
}

impl Drop for RestoreGlobalConfig {
    fn drop(&mut self) {
        let result = match &self.original {
            Some(bytes) => std::fs::write(&self.path, bytes),
            None => std::fs::remove_file(&self.path),
        };

        match result {
            Ok(()) => println!("[approval] restored {}", self.path.display()),
            // Loud on purpose: this is the user's own config, and a silent failure
            // would leave a policy they never asked for.
            Err(error) => eprintln!(
                "[approval] FAILED to restore {} — {error}",
                self.path.display()
            ),
        }
    }
}

/// A directory carrying a project-level `.omp/config.yml`, and its file.
///
/// The workspace is named for the test that owns it, so a run that dies before
/// cleanup cannot make the next one pass on stale state.
fn project_workspace(name: &str, config: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("omp-desktop-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".omp")).expect("the workspace is created");

    let touched = dir.join("touched.txt");
    let _ = std::fs::remove_file(&touched);

    std::fs::write(dir.join(".omp").join("config.yml"), config)
        .expect("the project policy is written");

    (dir, touched)
}

/// Send `prompt` to `live` and wait for `file` to appear, **without** answering
/// anything.
///
/// The file is the only honest signal that the tool ran: the engine asks for
/// approval before executing it, so a file that exists proves the gate let the call
/// through.
async fn run_unanswered(live: &LiveSession, prompt: &str, file: &Path) {
    let client = live.client().expect("the session is open");
    let accepted = client
        .request(
            commands::prompt(prompt, &[], None),
            Some(Duration::from_secs(30)),
        )
        .await
        .expect("the prompt is accepted");
    assert!(
        protocol::is_success(&accepted),
        "prompt rejected: {accepted}"
    );

    wait_for("the bash call to run", || file.exists().then_some(())).await;
}

#[tokio::test]
#[ignore = "requires a real `omp` binary, provider credentials, and network"]
async fn a_project_policy_suppresses_the_prompt() {
    // Pins this measurement, at v18.2.6: a project-level `<cwd>/.omp/config.yml`
    // carrying `tools.approval.bash: allow` **is** honoured at session start. That
    // is what makes the policy path provable without writing the user's real
    // config — the session below is launched with the prompting shape
    // (`--approval-mode write`, as `SidecarSpec::omp` builds it) and the policy is
    // the only thing that can silence it.
    let _guard = ENGINE_CONFIG.lock().await;
    let (dir, touched) =
        project_workspace("project-policy", "tools:\n  approval:\n    bash: allow\n");

    let sink = RecordingSink::default();
    let spec = SidecarSpec::omp(&dir).ephemeral();
    let live = open(
        &spec,
        ClientOptions {
            request_timeout: Duration::from_secs(30),
            ..Default::default()
        },
        sink.clone(),
        registry(),
    )
    .await
    .expect("the session opens");

    run_unanswered(&live, &bash_prompt(&touched), &touched).await;

    assert!(
        touched.exists(),
        "the call really ran, or the assertion below proves nothing: {} does not exist",
        touched.display()
    );
    // No dialog reached the window at any point in that turn.
    assert!(
        sink.dialogs().is_empty(),
        "an `allow` policy must raise no dialog, saw {:?}",
        sink.dialogs()
    );
    let status = live.status().expect("status");
    assert_eq!(
        status.counters.blocking_ui_requests, 0,
        "the engine must not have blocked on the window, saw {:?}",
        status.counters
    );

    let pid = status.sidecar_pid.expect("a pid to watch");
    live.shutdown(Duration::from_secs(10)).await;
    wait_for("the agent to exit", || (!process_exists(pid)).then_some(())).await;

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
#[ignore = "requires a real `omp` binary, provider credentials, and network"]
async fn the_policy_written_from_the_dialog_stops_the_next_session_asking() {
    // The feature's whole value, and the measurement that shapes its copy: the
    // write is **not** live. `allow_tool` records the policy, and the session that
    // stops asking is the *next* one — opened here with the same prompting shape
    // every other test uses (`--approval-mode write`) and no per-session override,
    // so the record is the only thing that can suppress the dialog.
    let _guard = ENGINE_CONFIG.lock().await;
    let restore = RestoreGlobalConfig::new();

    // What the user's config already held, so the assertion below is about the
    // outcome reporting the truth rather than about this machine's state.
    let before = recorded_policies();
    let expected_previous = before
        .get("bash")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    let outcome = omp_desktop::policy::allow_tool("bash")
        .await
        .expect("the policy is recorded");
    println!("outcome: {outcome:?}");

    assert_eq!(outcome.tool, "bash", "the outcome names the tool it wrote");
    assert_eq!(
        outcome.policy, "allow",
        "the outcome names the policy written"
    );
    assert_eq!(
        outcome.previous, expected_previous,
        "the outcome reports what the write replaced, and nothing else"
    );

    let workspace = std::env::temp_dir();
    let touched = workspace.join("omp-desktop-allow-next-session.txt");
    let _ = std::fs::remove_file(&touched);

    let sink = RecordingSink::default();
    let spec = SidecarSpec::omp(&workspace).ephemeral();
    let live = open(
        &spec,
        ClientOptions {
            request_timeout: Duration::from_secs(30),
            ..Default::default()
        },
        sink.clone(),
        registry(),
    )
    .await
    .expect("the session opens");

    run_unanswered(&live, &bash_prompt(&touched), &touched).await;

    assert!(
        touched.exists(),
        "the call really ran, or the assertion below proves nothing: {} does not exist",
        touched.display()
    );
    assert!(
        sink.dialogs().is_empty(),
        "the session after the write must ask nothing, saw {:?}",
        sink.dialogs()
    );

    let status = live.status().expect("status");
    let pid = status.sidecar_pid.expect("a pid to watch");
    live.shutdown(Duration::from_secs(10)).await;
    wait_for("the agent to exit", || (!process_exists(pid)).then_some(())).await;

    let _ = std::fs::remove_file(&touched);
    drop(restore);
}

#[tokio::test]
#[ignore = "requires a real `omp` binary, provider credentials, and network"]
async fn an_allow_write_leaves_the_other_policies_alone() {
    // Pins the merge. The CLI rejects per-tool subkeys, so an allow-write sends the
    // whole record back — a write that replaced instead of merged would silently
    // drop every other tool's policy the user had set.
    let _guard = ENGINE_CONFIG.lock().await;
    let restore = RestoreGlobalConfig::new();

    // Install a second tool's entry through the same CLI path, merged in so even
    // this step cannot drop a policy the user really had.
    let mut record = recorded_policies();
    record.insert("probe_other".to_string(), Value::String("deny".to_string()));
    set_policies(&record);

    let outcome = omp_desktop::policy::allow_tool("bash")
        .await
        .expect("the policy is recorded");
    assert_eq!(outcome.tool, "bash");

    let after = recorded_policies();
    assert_eq!(
        after.get("probe_other"),
        Some(&Value::String("deny".to_string())),
        "another tool's policy must survive the write: {after:?}"
    );
    assert_eq!(
        after.get("bash"),
        Some(&Value::String("allow".to_string())),
        "and this one must have landed: {after:?}"
    );

    // The user's config goes back before the lock is released, so no other test can
    // construct a session while this one's records are still in it.
    drop(restore);
}
