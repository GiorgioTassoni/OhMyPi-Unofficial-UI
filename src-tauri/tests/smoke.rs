//! The app actually starts.
//!
//! This is the one thing every other test in this repository cannot see, and it is here because
//! that cost a broken app: the tests drive modules through harnesses that *have* a Tokio runtime,
//! while the app's own `setup` runs **without** one. A bare `tokio::spawn` there panics — "there
//! is no reactor running" — before a single window is drawn, and every test still passes, because
//! a test has the runtime the app does not.
//!
//! That is exactly what happened. Step 14 started two background loops from `setup` with a bare
//! `tokio::spawn` (`idle::spawn` and `notify::spawn_job_watcher`); the supervisor's own live suite
//! passed, the 301 unit tests passed, the 37 live suites passed and the 62 window checks passed,
//! and the app died on launch with a panic nobody had run it to see. Both now spawn on
//! `tauri::async_runtime`, the runtime the rest of the app runs on, and this test is the guard:
//! it launches the real binary the way a person does and asserts it is still alive afterwards.
//!
//! Ignored by default: it opens a real window, so it needs a display — `DISPLAY` or
//! `WAYLAND_DISPLAY`, and `xvfb-run` on a headless machine. It passes **no** workspace argument,
//! so it starts no sidecar and writes nothing to the session store: what it exercises is the
//! startup path itself, which is where the panic lived.
//!
//! ```text
//! cargo test -p omp-desktop --test smoke -- --ignored --nocapture
//! ```

use std::process::{Command, Stdio};
use std::time::Duration;

/// How long the app is given to get through `setup` and open its window.
///
/// Eight seconds is generous for a path that either panics immediately or does not: the point is
/// to fail loudly on a process that has *already* gone, not to measure how fast it starts.
const SETTLE: Duration = Duration::from_secs(8);

#[test]
#[ignore = "opens a real window: needs a display (xvfb-run when headless)"]
fn the_app_starts_and_stays_up() {
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        eprintln!("no display, so there is no window to prove: run this under `xvfb-run`");
        return;
    }

    let mut child = Command::new(env!("CARGO_BIN_EXE_omp-desktop"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the app binary must be launchable");

    std::thread::sleep(SETTLE);

    if let Some(status) = child.try_wait().expect("waiting must not fail") {
        let output = child.wait_with_output().expect("output");
        panic!(
            "the app exited during startup with {status}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    let _ = child.kill();
    let output = child.wait_with_output().expect("output");
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Named separately from the exit code because a panic *can* be survived by an app with a
    // running event loop — the window would stay up around a dead host, which is worse than
    // exiting, since nothing on screen would say so.
    assert!(
        !stderr.contains("panicked"),
        "the app panicked while it was running:\n{stderr}"
    );
    assert!(
        !stderr.contains("there is no reactor running"),
        "a background task was spawned outside the app's runtime:\n{stderr}"
    );
}
