//! The mode switch, proved as the restart it is (`docs/12` §7.2).
//!
//! The engine has no runtime setter for `tools.approvalMode` (measured in 8b:
//! `--approval-mode` decides it at launch, and the config record is read when a
//! session is constructed). So the popover's change is: record the choice, then
//! restart the sidecar on the **same session file**. Two claims make that defensible
//! and both are asserted here:
//!
//! 1. **The thread survives it.** `--resume` keeps the session file, and the
//!    conversation comes back through the pager (`get_messages_page` →
//!    `Transcript::restore`) — the event stream starts at connect, so without that
//!    hydration a resumed thread would render as empty.
//! 2. **The mode really changes**, in the new process: `yolo` runs a `bash` call with
//!    no approval, where the app's `write` launch prompts for one.
//!
//! Ignored by default: it spawns real agents, needs credentials and network.
//!
//! ```text
//! cargo test -p omp-desktop --test mode_switch -- --ignored --nocapture
//! ```

mod support;

use std::path::{Path, PathBuf};
use std::time::Duration;

use omp_desktop::session::{open, LiveSession};
use omp_transport::protocol::{self, commands};
use omp_transport::{ClientOptions, SidecarSpec};
use support::{registry, wait_for, RecordingSink};

fn options() -> ClientOptions {
    ClientOptions {
        request_timeout: Duration::from_secs(60),
        ..Default::default()
    }
}

/// A workspace named for this test, so a run that dies before cleanup cannot make
/// the next one pass on stale state.
fn workspace() -> PathBuf {
    let path = std::env::temp_dir().join("omp-desktop-mode-switch");
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("the workspace");
    path
}

/// Send one prompt and wait for the turn to *finish*.
///
/// Waiting on `!is_streaming` alone is a race, and it cost this file its first version:
/// the flag is false before the turn starts, so the wait returned immediately, the
/// transcript still held only the user's own message, and the assertion that followed
/// was satisfied by the prompt rather than by the answer. The engine's own
/// `messageCount` is what actually moves — twice per turn — so the wait is for it to
/// move *and* for the turn to be over.
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
    // The last rows are patched on `message_end`; give the pump its turn to publish.
    tokio::time::sleep(Duration::from_millis(300)).await;
}

/// The conversation as plain text, for comparing a thread with its resumed self.
fn lines(live: &LiveSession) -> Vec<String> {
    live.rows()
        .expect("the transcript is readable")
        .iter()
        .map(|row| format!("{}: {}", row.role, row.text))
        .collect()
}

/// The session file the engine is writing, straight from `get_state`.
async fn session_file(live: &LiveSession) -> String {
    let client = live.client().expect("the session is open");
    let state = client
        .request(commands::get_state(), None)
        .await
        .expect("get_state answers");
    state["data"]["sessionFile"]
        .as_str()
        .expect("the state names a session file")
        .to_string()
}

/// The user's real engine config, put back however the test ends.
///
/// A guard rather than a call at the end, so a failed assertion still restores it.
struct RestoreConfig(PathBuf, String);

impl Drop for RestoreConfig {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.0, &self.1);
    }
}

fn config_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME")).join(".omp/agent/config.yml")
}

/// Serializes the two tests below: one writes the engine's config, and the other starts
/// sessions that read it at construction. Running them together would make a restart's
/// outcome depend on a file another test is editing — the hazard `tests/approval.rs`
/// guards against too, for the same reason.
static ENGINE_CONFIG: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
#[ignore = "requires a real `omp` binary, provider credentials, and network"]
async fn a_restart_changes_the_mode_and_keeps_the_thread() {
    let _turn = ENGINE_CONFIG.lock().await;
    let workspace = workspace();
    let touched = workspace.join("mode-switch-bash.txt");

    // ---- 1. A persistent session with one turn in it ----------------------
    println!("\n================ A: the session before the switch ================");
    let sink = RecordingSink::default();
    let live = open(
        &SidecarSpec::omp(&workspace),
        options(),
        sink.clone(),
        registry(),
    )
    .await
    .expect("the session opens");
    let file = session_file(&live).await;
    say(&live, "Reply with exactly: alpha").await;
    let before = lines(&live);
    println!("  before the restart: {} rows", before.len());
    for line in &before {
        println!("    {line}");
    }
    assert!(
        before
            .iter()
            .any(|line| line.starts_with("assistant:") && line.contains("alpha")),
        "the answer must be in the transcript before the restart, or the comparison below \
         would compare two empty threads: {before:?}"
    );

    // ---- 2. The restart the Mode popover performs -------------------------
    println!("\n================ B: --resume the same file, --approval-mode yolo ================");
    live.shutdown(Duration::from_secs(10)).await;

    let resumed_spec = SidecarSpec::omp(&workspace)
        .with_approval_mode("yolo")
        .resuming(&file);
    assert_eq!(
        resumed_spec.resumed_session_file(),
        Some(file.as_str()),
        "the restart must resume the file it came from"
    );

    let sink = RecordingSink::default();
    let next = open(&resumed_spec, options(), sink.clone(), registry())
        .await
        .expect("the resumed session opens");
    let again = session_file(&next).await;
    assert_eq!(
        again, file,
        "the restart must stay on the same session file"
    );

    // ---- 3. The thread is intact, through the pager -----------------------
    let after = lines(&next);
    println!("  after the restart: {} rows", after.len());
    for line in &after {
        println!("    {line}");
    }
    assert_eq!(
        after, before,
        "the resumed thread must show exactly what the live one did — the event stream \
         starts at connect, so this is the hydration (`get_messages_page`) and nothing else"
    );

    // ---- 4. The new mode is in force --------------------------------------
    // `yolo` approves everything, so the call runs with no dialog. Under the app's
    // `write` launch the same prompt raises `select` + `["Approve","Deny"]` (8b).
    say(&next, &bash_prompt(&touched)).await;
    let dialogs = sink.dialogs();
    assert!(
        dialogs.is_empty(),
        "`yolo` must not prompt, but the run raised {dialogs:?}"
    );
    wait_for("the approved call to run", || {
        touched.exists().then_some(())
    })
    .await;
    println!(
        "  yolo ran the command with no dialog: {}",
        touched.exists()
    );

    next.shutdown(Duration::from_secs(10)).await;
    let _ = std::fs::remove_dir_all(&workspace);
}

fn bash_prompt(file: &Path) -> String {
    format!(
        "Use the bash tool to run exactly `touch {}`, then reply with exactly: done",
        file.display()
    )
}

/// The recorded mode is what the *next* session reads, which is the half that
/// outlives the process.
#[tokio::test]
#[ignore = "requires a real `omp` binary (the config CLI), no network"]
async fn the_mode_is_recorded_for_the_sessions_that_follow() {
    let _turn = ENGINE_CONFIG.lock().await;
    let config = config_path();
    let original = std::fs::read_to_string(&config).expect("the config is readable");
    let _restore = RestoreConfig(config.clone(), original.clone());

    omp_desktop::policy::record_mode("always-ask")
        .await
        .expect("the mode is written");
    let recorded = omp_transport::protocol::commands::get_state();
    assert!(
        recorded.is_object(),
        "sanity: the protocol still builds commands"
    );

    // Read it back through the engine's own CLI, not through the code under test.
    let output = std::process::Command::new(omp_transport::resolve_omp_binary())
        .args(["config", "get", "tools.approvalMode"])
        .output()
        .expect("the CLI runs");
    let value = String::from_utf8_lossy(&output.stdout);
    assert!(
        value.contains("always-ask"),
        "the engine must report the mode that was written, got {value:?}"
    );
    println!("  recorded: {}", value.trim());
}
