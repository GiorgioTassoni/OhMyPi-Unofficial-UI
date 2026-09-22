//! Sidecar process supervision.
//!
//! One `omp --mode rpc-ui` child per session (`docs/11-v1-scope.md` §3.1). This
//! module owns the process and its pipes and nothing else — it does not know
//! about frames, sessions or UI.
//!
//! Two behaviours here are correctness requirements rather than conveniences:
//!
//! * **stderr is drained continuously.** A child whose stderr pipe fills up
//!   blocks on write, and a blocked agent produces no frames. We keep a bounded
//!   tail for diagnostics and discard the rest.
//! * **the child is killed on drop.** The app must not orphan sidecars; each is
//!   a ~200 MB process.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;

use crate::error::ClientError;

/// Bytes of stderr retained for diagnostics.
const STDERR_TAIL_BYTES: usize = 64 * 1024;

/// How to launch a sidecar.
#[derive(Debug, Clone)]
pub struct SidecarSpec {
    /// Executable. Defaults to the resolved `omp` binary.
    pub program: PathBuf,
    /// Arguments, e.g. `--mode rpc-ui --approval-mode write`.
    pub args: Vec<String>,
    /// Working directory — the workspace the agent operates on.
    pub cwd: Option<PathBuf>,
    /// Extra environment variables layered on the inherited environment.
    pub env: Vec<(String, String)>,
}

impl SidecarSpec {
    /// Build a spec for `omp` with the v1 launch shape.
    ///
    /// `docs/11-v1-scope.md` §3.1 / D3: protocol mode with a live UI context
    /// (`rpc-ui` — plain `rpc` cannot prompt for approval and fails closed),
    /// plus an explicit approval mode because the engine's own default is
    /// `yolo`.
    pub fn omp(cwd: impl Into<PathBuf>) -> Self {
        Self {
            program: resolve_omp_binary(),
            args: vec![
                "--mode".into(),
                "rpc-ui".into(),
                "--approval-mode".into(),
                "write".into(),
            ],
            cwd: Some(cwd.into()),
            // RPC suppresses extension `setTitle` chrome unless the host opts in. Session
            // title generation is separate; the host starts that through OMP's own generator.
            env: vec![("PI_RPC_EMIT_TITLE".into(), "1".into())],
        }
    }

    /// Append a launch flag, for example `("--model", "anthropic/claude-sonnet-4-5")`.
    pub fn with_arg(mut self, flag: &str, value: &str) -> Self {
        self.args.push(flag.into());
        self.args.push(value.into());
        self
    }

    /// Append a valueless launch flag.
    pub fn with_flag(mut self, flag: &str) -> Self {
        self.args.push(flag.into());
        self
    }

    /// Resume a session file instead of starting empty.
    pub fn resuming(self, session_file: &str) -> Self {
        self.with_arg("--resume", session_file)
    }

    /// Launch with this approval mode, replacing the one already in the args.
    ///
    /// Replacing rather than appending is the whole point: the app's spec always
    /// carries a mode (`omp`), and passing a second `--approval-mode` would leave the
    /// engine to decide which one it means. The mode switch (`docs/12` §7.2) is the
    /// caller — it restarts a live session with a different ladder, so "the last flag
    /// wins" is not a bet worth taking.
    pub fn with_approval_mode(mut self, mode: &str) -> Self {
        let mut args: Vec<String> = Vec::with_capacity(self.args.len() + 2);
        let mut index = 0;
        while index < self.args.len() {
            if self.args[index] == "--approval-mode" {
                // Drop the flag and its value.
                index += 2;
                continue;
            }
            args.push(self.args[index].clone());
            index += 1;
        }
        args.push("--approval-mode".into());
        args.push(mode.into());
        self.args = args;
        self
    }

    /// The approval mode this spec launches with, when it names one.
    ///
    /// Read from the args for the same reason `resumed_session_file` is: the flag is
    /// what the engine acts on. A spec without one leaves the decision to the engine's
    /// own default — `yolo` — which is why `None` is reported as "no mode" rather than
    /// as a guess (`docs/12` §7.2).
    pub fn approval_mode(&self) -> Option<&str> {
        let index = self.args.iter().position(|arg| arg == "--approval-mode")?;
        self.args.get(index + 1).map(String::as_str)
    }

    /// The session file this spec resumes, when it resumes one.
    ///
    /// Read from the args rather than from a field of its own, because the flag is
    /// what the engine acts on: a spec whose args say `--resume` is resuming, whatever
    /// else a caller believes about it. `sesion::open` uses this to decide whether the
    /// conversation has to be hydrated (`docs/12` §6.2).
    pub fn resumed_session_file(&self) -> Option<&str> {
        let index = self.args.iter().position(|arg| arg == "--resume")?;
        self.args.get(index + 1).map(String::as_str)
    }

    /// Keep the session out of the on-disk store.
    pub fn ephemeral(self) -> Self {
        self.with_flag("--no-session")
    }

    /// Add an environment variable for the child.
    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

/// Resolve the `omp` executable.
///
/// First hit wins:
///
/// 1. `OMP_BIN` — the override tests and development builds use.
/// 2. **Beside the running executable** — where Tauri's `externalBin` staging
///    puts it (the target-triple suffix is stripped at bundle time), which is how
///    a packaged app finds the sidecar it shipped with.
/// 3. `omp` on `PATH` — a developer who already has the CLI installed.
///
/// The middle rule is the one that matters for distribution: it is what makes a
/// packaged app self-contained rather than dependent on the user's `PATH`.
pub fn resolve_omp_binary() -> PathBuf {
    if let Ok(path) = std::env::var("OMP_BIN") {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }

    if let Ok(executable) = std::env::current_exe() {
        if let Some(bundled) = bundled_sibling(&executable) {
            return bundled;
        }
    }

    PathBuf::from("omp")
}

/// The sidecar staged beside `executable`, if it is there.
///
/// Split from [`resolve_omp_binary`] so the rule can be tested without moving the
/// test binary.
fn bundled_sibling(executable: &Path) -> Option<PathBuf> {
    let candidate = executable.parent()?.join(SIDECAR_FILE_NAME);
    candidate.is_file().then_some(candidate)
}

/// The filename Tauri's `externalBin` staging produces, once the triple suffix
/// has been stripped.
pub const SIDECAR_FILE_NAME: &str = if cfg!(windows) { "omp.exe" } else { "omp" };

/// State shared with the stderr drain task.
#[derive(Debug, Default)]
struct StderrTail {
    lines: VecDeque<String>,
    bytes: usize,
}

impl StderrTail {
    fn push_line(&mut self, line: String) {
        self.bytes += line.len();
        self.lines.push_back(line);
        while self.bytes > STDERR_TAIL_BYTES && self.lines.len() > 1 {
            if let Some(dropped) = self.lines.pop_front() {
                self.bytes -= dropped.len();
            }
        }
    }

    fn snapshot(&self) -> String {
        self.lines.iter().cloned().collect::<Vec<_>>().join("\n")
    }
}

/// A running sidecar process.
///
/// Every lifecycle operation works through `&self`, including shutdown. That is a
/// hard requirement, not a style choice: a host that shares the client (an `Arc`,
/// as the desktop app does) must still be able to *end* the child, and a
/// consuming `shutdown(self)` cannot be called on a shared handle. The
/// alternative — dropping instead of stopping — leaks a ~200 MB agent process,
/// which is exactly what this type exists to prevent.
#[derive(Debug)]
pub struct Sidecar {
    /// `Option` so the child can be taken for waiting, and reaped exactly once.
    child: Mutex<Option<Child>>,
    /// A single async mutex rather than a sync one wrapping an async one: the
    /// writer must hold it *across* an await, so a `std::sync` guard would make
    /// the future non-`Send`.
    stdin: AsyncMutex<Option<ChildStdin>>,
    stderr_tail: Arc<Mutex<StderrTail>>,
    stderr_task: Option<JoinHandle<()>>,
}

impl Sidecar {
    /// Spawn the child and return it with its stdout reader.
    ///
    /// stdout is handed to the caller because the frame reader must own it
    /// exclusively; stderr is drained internally.
    pub fn spawn(spec: &SidecarSpec) -> Result<(Self, BufReader<ChildStdout>), ClientError> {
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }
        for (key, value) in &spec.env {
            command.env(key, value);
        }

        let mut child = command.spawn().map_err(|source| ClientError::Spawn {
            program: spec.program.display().to_string(),
            source,
        })?;

        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");

        let stderr_tail = Arc::new(Mutex::new(StderrTail::default()));
        let sink = Arc::clone(&stderr_tail);
        let stderr_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            // Draining is the point; the content is only for diagnostics.
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(mut tail) = sink.lock() {
                    tail.push_line(line);
                }
            }
        });

        Ok((
            Self {
                child: Mutex::new(Some(child)),
                stdin: AsyncMutex::new(stdin),
                stderr_tail,
                stderr_task: Some(stderr_task),
            },
            BufReader::new(stdout),
        ))
    }

    /// The child's process id, when it is still running.
    pub fn id(&self) -> Option<u32> {
        self.child
            .lock()
            .ok()
            .and_then(|child| child.as_ref().and_then(Child::id))
    }

    /// Write one pre-encoded line followed by a newline and flush it.
    pub async fn write_raw_line(&self, line: &str) -> Result<(), ClientError> {
        let mut guard = self.stdin.lock().await;
        let stdin = guard.as_mut().ok_or(ClientError::StdinClosed)?;
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(ClientError::Write)?;
        stdin.write_all(b"\n").await.map_err(ClientError::Write)?;
        stdin.flush().await.map_err(ClientError::Write)
    }

    /// Close stdin. The engine then drains accepted commands and exits 0; keep
    /// reading stdout until it does.
    pub async fn close_stdin(&self) {
        let mut guard = self.stdin.lock().await;
        if let Some(mut stdin) = guard.take() {
            let _ = stdin.shutdown().await;
        }
    }

    /// The most recent stderr output, for error surfaces.
    pub fn stderr_tail(&self) -> String {
        self.stderr_tail
            .lock()
            .map(|tail| tail.snapshot())
            .unwrap_or_default()
    }

    /// Wait for exit, or kill the child if it overruns the timeout.
    ///
    /// Returns `None` when the child was already reaped, so a second call cannot
    /// report a status it did not observe.
    pub async fn wait_or_kill(
        &self,
        timeout: Duration,
    ) -> std::io::Result<Option<std::process::ExitStatus>> {
        let child = self.child.lock().ok().and_then(|mut slot| slot.take());
        let Some(mut child) = child else {
            return Ok(None);
        };

        match tokio::time::timeout(timeout, child.wait()).await {
            Ok(result) => result.map(Some),
            Err(_) => {
                let _ = child.start_kill();
                child.wait().await.map(Some)
            }
        }
    }

    /// Terminate the child immediately.
    pub fn kill(&self) {
        if let Ok(mut child) = self.child.lock() {
            if let Some(child) = child.as_mut() {
                let _ = child.start_kill();
            }
        }
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        // Belt and braces alongside `kill_on_drop`: never leave a 200 MB agent
        // process behind when a session view goes away.
        self.kill();
        if let Some(task) = self.stderr_task.take() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_the_approval_mode_replaces_the_flag() {
        // Two `--approval-mode` flags in one argv would leave the engine to pick, and
        // the mode switch is exactly the caller that would produce them.
        let spec = SidecarSpec::omp(".").with_approval_mode("yolo");
        let modes: Vec<&String> = spec
            .args
            .windows(2)
            .filter(|pair| pair[0] == "--approval-mode")
            .map(|pair| &pair[1])
            .collect();
        assert_eq!(modes, vec![&"yolo".to_string()]);

        let again = spec.with_approval_mode("always-ask");
        let modes: Vec<&String> = again
            .args
            .windows(2)
            .filter(|pair| pair[0] == "--approval-mode")
            .map(|pair| &pair[1])
            .collect();
        assert_eq!(modes, vec![&"always-ask".to_string()]);
    }

    #[test]
    fn the_launch_mode_is_read_from_the_args_it_launches_with() {
        assert_eq!(SidecarSpec::omp(".").approval_mode(), Some("write"));
        assert_eq!(
            SidecarSpec::omp(".")
                .with_approval_mode("yolo")
                .approval_mode(),
            Some("yolo")
        );
        // A spec that names no mode is reported as naming none: the engine's own
        // default (`yolo`) is not ours to assume.
        let bare = SidecarSpec {
            args: vec!["--mode".into(), "rpc-ui".into()],
            ..SidecarSpec::omp(".")
        };
        assert_eq!(bare.approval_mode(), None);
    }

    #[test]
    fn resuming_is_read_from_the_args_it_launches_with() {
        assert_eq!(SidecarSpec::omp(".").resumed_session_file(), None);
        let resumed = SidecarSpec::omp(".").resuming("/tmp/thread.jsonl");
        assert_eq!(resumed.resumed_session_file(), Some("/tmp/thread.jsonl"));
        // The mode switch must not disturb it.
        assert_eq!(
            resumed.with_approval_mode("yolo").resumed_session_file(),
            Some("/tmp/thread.jsonl")
        );
    }

    #[test]
    fn the_app_launch_declares_its_approval_posture() {
        // `write` — read and write tiers run, exec asks. It is passed explicitly
        // rather than inherited because the engine's own default is `yolo`: with no
        // flag the approval dialog never appears, and every test of it would fail
        // for a reason nothing in the UI could explain. Measured on v18.2.6: with
        // this flag a `bash` call raises `select` + `["Approve","Deny"]`, without it
        // nothing asks at all.
        let spec = SidecarSpec::omp(".");
        let mode = spec
            .args
            .windows(2)
            .find(|pair| pair[0] == "--approval-mode")
            .map(|pair| pair[1].clone());
        assert_eq!(mode.as_deref(), Some("write"));
    }

    #[test]
    fn the_app_launch_opts_into_extension_title_chrome() {
        let spec = SidecarSpec::omp(".");
        assert!(spec
            .env
            .iter()
            .any(|(key, value)| key == "PI_RPC_EMIT_TITLE" && value == "1"));
    }
}
