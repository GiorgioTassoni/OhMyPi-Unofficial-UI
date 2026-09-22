//! What a settings write costs, proved as the restart it is (`docs/12` §12).
//!
//! Every row of the settings screen says what it takes for the change to be in effect, and the
//! claim behind `restart: "sidecar"` is the one users will notice if it is wrong: *the file is
//! read when a session is constructed, not while it runs*. `docs/13` classes keys by where a
//! value is consumed, which is a different question from whether a write from another process
//! is visible — and the app writes from another process.
//!
//! This pins both halves on a real engine:
//!
//! 1. **At construction the file is read**: a session started in a workspace whose
//!    `.omp/config.yml` says `steeringMode: all` reports `all`, which also proves a project
//!    file really is honoured (`docs/13` leans on that for its provenance display).
//! 2. **A running session never sees the write**, even given time to: the file is rewritten
//!    behind it and `get_state` still answers `all`.
//! 3. **The restart the screen offers is what applies it**: the same session file, resumed in a
//!    new process, answers `one-at-a-time` — the `--resume` restart the Mode popover already
//!    performs (`tests/mode_switch.rs`), which is what `settings_restart_sessions` reuses.
//!
//! The workspace is a temp directory with its own project config, so nothing here touches the
//! user's global config and no test needs to serialize against another.
//!
//! Ignored by default: it spawns a real `omp` sidecar. It needs no credentials and no network —
//! construction and `get_state` are local, and no turn is ever run.
//!
//! ```text
//! cargo test -p omp-desktop --test settings_live -- --ignored --nocapture
//! ```

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use omp_desktop::session::{open, LiveSession};
use omp_transport::protocol::commands;
use omp_transport::{ClientOptions, SidecarSpec};
use support::{registry, RecordingSink};

fn options() -> ClientOptions {
    ClientOptions {
        request_timeout: Duration::from_secs(60),
        ..Default::default()
    }
}

/// A workspace named for this test, so a run that dies before cleanup cannot make the next one
/// pass on a stale config file.
fn workspace() -> PathBuf {
    let path = std::env::temp_dir().join("omp-desktop-settings-live");
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("the workspace");
    path
}

/// Write the project's config file, the way a user's own editor would.
fn set_project_setting(workspace: &Path, body: &str) {
    let directory = workspace.join(".omp");
    std::fs::create_dir_all(&directory).expect("the project config directory");
    std::fs::write(directory.join("config.yml"), body).expect("the project config");
}

/// One key, straight from `get_state`.
async fn setting(live: &LiveSession, key: &str) -> String {
    let client = live.client().expect("the session is open");
    let state = client
        .request(commands::get_state(), None)
        .await
        .expect("get_state answers");

    state["data"][key]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| format!("<the state did not report {key}: {}>", state["data"]))
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

/// Open a session in `workspace`, resuming `file` when one is given.
async fn live_session(workspace: &Path, file: Option<&str>) -> Arc<LiveSession> {
    let spec = match file {
        Some(file) => SidecarSpec::omp(workspace).resuming(file),
        None => SidecarSpec::omp(workspace),
    };

    open(&spec, options(), RecordingSink::default(), registry())
        .await
        .expect("the session opens")
}

#[tokio::test]
#[ignore = "spawns a real `omp` sidecar (no network, no credentials needed)"]
async fn a_setting_is_read_at_construction_and_only_a_restart_applies_a_write() {
    let workspace = workspace();
    set_project_setting(&workspace, "steeringMode: all\n");

    // ---- 1. The file is read when the session is constructed ----------------
    println!("\n=========== A: a session started in a workspace with a project config ===========");
    let live = live_session(&workspace, None).await;
    let file = session_file(&live).await;
    let before = setting(&live, "steeringMode").await;
    println!("  the project file says `steeringMode: all`; get_state says `{before}`");
    assert_eq!(
        before, "all",
        "a project config file is honoured at construction — the provenance display depends on it"
    );

    // ---- 2. A write behind it is not seen, even given time ------------------
    println!("\n=========== B: the same key rewritten while the session runs ===========");
    set_project_setting(&workspace, "steeringMode: one-at-a-time\n");
    // Every chance to notice: if a reload is happening on a timer, this is where it would
    // show up. Measured, there is no reload path at all — `Settings.reloadFromDisk()` has one
    // caller in the whole engine, a sub-agent re-scoping.
    tokio::time::sleep(Duration::from_millis(750)).await;

    let during = setting(&live, "steeringMode").await;
    println!("  the file now says `one-at-a-time`; get_state still says `{during}`");
    assert_eq!(
        during, "all",
        "a running session must not pick up another process's write — this is exactly why \
         every affected row says a restart is needed"
    );

    // ---- 3. The restart is what applies it ---------------------------------
    println!("\n=========== C: --resume the same file, the restart the screen offers ===========");
    live.shutdown(Duration::from_secs(10)).await;

    let restarted = live_session(&workspace, Some(&file)).await;
    assert_eq!(
        session_file(&restarted).await,
        file,
        "the restart must stay on the same session file"
    );

    let after = setting(&restarted, "steeringMode").await;
    println!("  after the restart: get_state says `{after}`");
    assert_eq!(
        after, "one-at-a-time",
        "the restart is what makes a written setting real"
    );

    restarted.shutdown(Duration::from_secs(10)).await;
    let _ = std::fs::remove_dir_all(&workspace);
}
