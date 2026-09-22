//! The app's own terminals (`docs/12` §11, decision D6).
//!
//! A real PTY per tab, running a real shell, starting in the workspace the user was looking
//! at. This is the one part of the app the engine has no part in: `--mode rpc-ui` sets
//! `PI_NO_PTY=1`, so the agent's `bash` runs without a terminal and can neither see nor drive
//! what runs here. The panel says so in as many words, because "the agent's shell" is the
//! assumption a user would otherwise make.
//!
//! # Shape
//!
//! [`Terminals`] owns the sessions, each one a `portable-pty` master/slave pair plus the child
//! the slave spawned, keyed by an id this host mints. Two threads per terminal: one blocks
//! reading the master, one posts what it read to the frontend in batches. The `sync_channel`
//! between them is the whole flow-control story, and it works the way a terminal should — when
//! the webview falls behind, the reader blocks, the kernel's pty buffer fills, and the *shell*
//! blocks. `yes` stops scrolling instead of the host growing without bound.
//!
//! # Lifecycle
//!
//! A terminal is not thread-scoped. It is started in a workspace and outlives the session it
//! was opened beside, because the user's reason for having one is usually that they are about
//! to switch threads. It ends when its shell ends, when the user closes the tab, or when the
//! app exits ([`Terminals::shutdown`], from the host's own exit hook) — and that last one is
//! the one that must not leak a process, so it ends the shell's whole session rather
//! than only the shell.
//!
//! # Why input is written on the calling thread
//!
//! `write` puts the bytes straight into the master and returns. A pty's input queue is drained
//! by the shell as it reads, so the only way to block here is a paste larger than that queue
//! aimed at a shell that has stopped reading — and the fix for it (a writer thread per terminal
//! behind a channel) would cost a third thread and make "that shell has exited" unobservable at
//! the call site. Input ordering is a correctness property and the queue is a performance one,
//! so the ordering wins: one call, one write, in the order the user typed.
//!
//! # What a closed tab does not do
//!
//! Nothing is replayed. A tab that is closed loses its scrollback, and output written while no
//! window was listening is gone: there is no ring buffer here to re-read, because the frontend
//! holds the scrollback (that is what a terminal emulator is for) and the host's job is to
//! deliver bytes once, in order.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use portable_pty::{native_pty_system, Child, CommandBuilder, ExitStatus, MasterPty, PtySize};
use tauri::{AppHandle, Emitter};

use crate::dto::{
    TerminalExit, TerminalOutput, TerminalSnapshot, TERMINALS_EVENT, TERMINAL_OUTPUT_EVENT,
};

/// The smallest and largest window a terminal may be told it has.
///
/// Clamped rather than refused: a webview that has not been laid out yet reports `0`, and a
/// shell told it has no columns prints its prompt one character per line. At the top end this
/// is the terminal default of a very large screen, so nothing real is being denied.
const MIN_COLS: u16 = 20;
const MAX_COLS: u16 = 1_000;
const MIN_ROWS: u16 = 5;
const MAX_ROWS: u16 = 1_000;

/// The largest batch one event carries.
///
/// A full-screen repaint arrives as hundreds of small writes; one event each would put
/// hundreds of IPC round trips through the webview for one keystroke. 8 KiB is also what the
/// reader asks for at a time, so the common case is one read, one event.
const MAX_CHUNK: usize = 8 * 1024;

/// How much output may sit between the reader and the pump.
///
/// This is the flow control (see the module docs): eight batches of unposted output, after
/// which the reader blocks.
const QUEUE_DEPTH: usize = 8;

/// How long a shell gets to honour `SIGHUP` before the host stops asking.
const KILL_GRACE: Duration = Duration::from_millis(250);

/// How long a killed shell gets to be reaped before the host moves on.
const REAP_GRACE: Duration = Duration::from_secs(5);

/// How long a terminal whose output ended gets to produce an exit status before the host stops
/// waiting for one.
///
/// Output ending does not always mean the shell is gone — a shell can close its own terminal —
/// so this bounds the wait rather than assuming the good case. When it expires the row stays
/// as it was: a shell that is still running is reported as running, not as exited with a code
/// nobody observed.
const EXIT_GRACE: Duration = Duration::from_secs(5);

/// What the shell is told it is talking to.
const TERM: &str = "xterm-256color";

/// Where terminal events go.
///
/// A trait rather than an `AppHandle`, for the reason `crate::search::ProgressSink` is one: a
/// unit test drives the same code with no window, and nothing here has an opinion about the
/// event bus. Passed to the calls that announce rather than stored, so [`Terminals`] stays
/// what it is — the sessions.
pub trait TerminalSink: Send + Sync + 'static {
    /// One batch of output, already base64.
    fn output(&self, output: &TerminalOutput);
    /// The whole tab set, whenever it changes.
    fn changed(&self, terminals: &[TerminalSnapshot]);
}

impl TerminalSink for AppHandle {
    fn output(&self, output: &TerminalOutput) {
        let _ = self.emit(TERMINAL_OUTPUT_EVENT, output.clone());
    }

    fn changed(&self, terminals: &[TerminalSnapshot]) {
        let _ = self.emit(TERMINALS_EVENT, terminals.to_vec());
    }
}

/// A sink that drops what it is told, for a caller with no window.
#[derive(Debug, Default)]
pub struct NullSink;

impl TerminalSink for NullSink {
    fn output(&self, _output: &TerminalOutput) {}
    fn changed(&self, _terminals: &[TerminalSnapshot]) {}
}

/// What a terminal runs.
///
/// Production runs the user's own shell ([`Program::shell`]). A test names its own, so a check
/// of "the shell sees the size it was given" does not depend on what `$SHELL` happens to be in
/// whoever's environment runs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub file: String,
    pub args: Vec<String>,
}

impl Program {
    /// The user's shell: `$SHELL` when it names something, the platform's default otherwise.
    pub fn shell() -> Self {
        #[cfg(unix)]
        let (fallback, variable) = ("/bin/sh", "SHELL");
        #[cfg(windows)]
        let (fallback, variable) = ("cmd.exe", "COMSPEC");

        let file = std::env::var(variable)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| fallback.to_string());

        Self {
            file,
            args: Vec::new(),
        }
    }

    /// A specific program.
    pub fn new(file: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            args: Vec::new(),
        }
    }

    /// With one more argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }
}

/// Every terminal the app has open.
#[derive(Default)]
pub struct Terminals {
    /// The tabs, in the order they were opened.
    ///
    /// A `Vec` rather than a map: a person has a handful of terminals, the panel draws them in
    /// the order they were opened, and a map would not preserve that without a second key.
    sessions: Mutex<Vec<Session>>,
    /// The next id, so two tabs opened in the same millisecond cannot collide.
    next: AtomicU64,
}

/// One terminal: the pty pair, the process in it, and how that process ended.
struct Session {
    id: String,
    cwd: PathBuf,
    /// The master's writer half. Kept apart from the master because writing needs `&mut` and
    /// resizing does not.
    writer: Box<dyn Write + Send>,
    /// The master, for resizes and for the window size the kernel reports to the shell.
    pty: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    /// The id the child was spawned with, kept because `process_id` is not meaningful once the
    /// child is reaped.
    pid: Option<u32>,
    /// `None` while it runs: set once, by whoever notices the shell is gone.
    exit: Option<TerminalExit>,
}

impl Session {
    fn snapshot(&self) -> TerminalSnapshot {
        TerminalSnapshot {
            id: self.id.clone(),
            cwd: self.cwd.display().to_string(),
            running: self.exit.is_none(),
            exit: self.exit.clone(),
            pid: self.pid,
        }
    }
}

impl Terminals {
    pub fn new() -> Self {
        Self::default()
    }

    /// The tabs, in the order they were opened.
    pub fn list(&self) -> Vec<TerminalSnapshot> {
        self.lock()
            .iter()
            .map(Session::snapshot)
            .collect::<Vec<_>>()
    }

    /// Open a terminal in `cwd`, running the user's shell.
    pub fn open(
        self: &Arc<Self>,
        sink: Arc<dyn TerminalSink>,
        cwd: &Path,
        cols: u16,
        rows: u16,
    ) -> Result<TerminalSnapshot, String> {
        self.open_program(sink, cwd, cols, rows, Program::shell())
    }

    /// Open a terminal in `cwd`, running `program`.
    ///
    /// The seam a test drives: everything about the terminal is the same either way, and the
    /// only difference is which program the pty is told to run.
    pub fn open_program(
        self: &Arc<Self>,
        sink: Arc<dyn TerminalSink>,
        cwd: &Path,
        cols: u16,
        rows: u16,
        program: Program,
    ) -> Result<TerminalSnapshot, String> {
        // Refused rather than substituted: a shell that starts in the wrong directory is worse
        // than one that does not start, because nothing about it looks wrong.
        if !cwd.is_dir() {
            return Err(format!("{} is not a directory", cwd.display()));
        }

        let size = PtySize {
            rows: clamp(rows, MIN_ROWS, MAX_ROWS),
            cols: clamp(cols, MIN_COLS, MAX_COLS),
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = native_pty_system()
            .openpty(size)
            .map_err(|error| format!("no terminal was allocated: {error}"))?;

        let mut command = CommandBuilder::new(&program.file);
        for arg in &program.args {
            command.arg(arg);
        }
        command.cwd(cwd);
        command.env("TERM", TERM);
        // Not "256 colors": programs ask for this to decide whether to emit 24-bit SGR, and
        // xterm.js renders it. Without it a gradient is quantised into the 256-colour palette.
        command.env("COLORTERM", "truecolor");

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| format!("{} did not start: {error}", program.file))?;

        // The host must not hold the slave: the master sees EOF when the last slave handle
        // closes, so keeping this one would leave a closed tab looking like a live shell
        // forever.
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("the terminal cannot be read: {error}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("the terminal cannot be written: {error}"))?;

        let id = format!("pty-{}", self.next.fetch_add(1, Ordering::Relaxed));
        let session = Session {
            id: id.clone(),
            cwd: cwd.to_path_buf(),
            writer,
            pty: pair.master,
            pid: child.process_id(),
            exit: None,
            child,
        };

        let snapshot = session.snapshot();
        self.lock().push(session);
        spawn_pump(id, reader, Arc::clone(self), Arc::clone(&sink));
        sink.changed(&self.list());

        Ok(snapshot)
    }

    /// Send what the user typed.
    pub fn write(&self, id: &str, data: &str) -> Result<(), String> {
        let mut sessions = self.lock();
        let session = sessions
            .iter_mut()
            .find(|session| session.id == id)
            .ok_or_else(|| format!("there is no terminal {id}"))?;

        // Answering this here rather than letting the kernel say `EIO`: a write to a shell that
        // has exited is not a failure to report as one, it is a keystroke with nowhere to go.
        if session.exit.is_some() {
            return Err("that shell has exited".to_string());
        }

        session
            .writer
            .write_all(data.as_bytes())
            .and_then(|()| session.writer.flush())
            .map_err(|error| format!("the shell is not reading: {error}"))
    }

    /// Tell the shell its window changed size.
    ///
    /// The kernel passes this on as `SIGWINCH`, which is why a full-screen program redraws: the
    /// resize is not a host-side convenience, it is the terminal protocol.
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let mut sessions = self.lock();
        let session = sessions
            .iter_mut()
            .find(|session| session.id == id)
            .ok_or_else(|| format!("there is no terminal {id}"))?;

        let size = PtySize {
            rows: clamp(rows, MIN_ROWS, MAX_ROWS),
            cols: clamp(cols, MIN_COLS, MAX_COLS),
            pixel_width: 0,
            pixel_height: 0,
        };

        session
            .pty
            .resize(size)
            .map_err(|error| format!("the terminal did not resize: {error}"))
    }

    /// Close a tab, ending its shell. Returns the remaining tabs.
    pub fn close(
        &self,
        sink: &dyn TerminalSink,
        id: &str,
    ) -> Result<Vec<TerminalSnapshot>, String> {
        let session = {
            let mut sessions = self.lock();
            let at = sessions
                .iter()
                .position(|session| session.id == id)
                .ok_or_else(|| format!("there is no terminal {id}"))?;
            sessions.remove(at)
        };

        // Outside the lock: terminating waits on a process, and holding the registry across
        // that wait would stall every other tab's keystroke behind it.
        let mut session = session;
        terminate(&mut session);
        drop(session);

        let terminals = self.list();
        sink.changed(&terminals);
        Ok(terminals)
    }

    /// End every shell. Called from the host's exit hook, and the only path that must not
    /// leave a process behind.
    pub fn shutdown(&self) {
        let sessions = {
            let mut guard = self.lock();
            std::mem::take(&mut *guard)
        };

        for mut session in sessions {
            terminate(&mut session);
        }
    }

    /// Notice that a terminal's shell exited, and record how.
    ///
    /// Called by the pump once output has ended. The child is usually already gone by then, so
    /// the common case is one `try_wait`; the wait exists for the case where the shell closed
    /// its own terminal and is still running, and it is bounded for the case where it never
    /// exits at all.
    fn reap(&self, id: &str) -> Option<TerminalSnapshot> {
        let deadline = Instant::now() + EXIT_GRACE;
        loop {
            {
                let mut sessions = self.lock();
                let session = sessions.iter_mut().find(|session| session.id == id)?;
                if session.exit.is_some() {
                    return Some(session.snapshot());
                }
                match session.child.try_wait() {
                    Ok(Some(status)) => {
                        session.exit = Some(signal_or_code(status));
                        return Some(session.snapshot());
                    }
                    // Still running, or the platform cannot say: either way there is nothing to
                    // record yet, and the row keeps reporting what is true — it is running.
                    Ok(None) => {}
                    Err(_) => return None,
                }
            }

            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// The sessions, with a poisoned lock treated as a lock that was held when a thread
    /// panicked: the data behind it is a list of handles, which is not made invalid by a panic
    /// elsewhere.
    fn lock(&self) -> MutexGuard<'_, Vec<Session>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Read the master until it ends, posting what arrives.
fn spawn_pump(
    id: String,
    mut reader: Box<dyn Read + Send>,
    registry: Arc<Terminals>,
    sink: Arc<dyn TerminalSink>,
) {
    let (sender, receiver) = sync_channel::<Vec<u8>>(QUEUE_DEPTH);

    let reader_name = format!("pty-reader-{id}");
    let _ = thread::Builder::new().name(reader_name).spawn(move || {
        let mut buffer = vec![0u8; MAX_CHUNK];
        loop {
            match reader.read(&mut buffer) {
                // Zero is EOF and an error is a hung-up pty; both mean no more output is
                // coming, and the pump tells the difference that matters (`reap`).
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if sender.send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
        // Dropping the sender here is how the pump learns the output is over.
    });

    let pump_name = format!("pty-pump-{id}");
    let _ = thread::Builder::new().name(pump_name).spawn(move || {
        pump(&receiver, &id, sink.as_ref());

        // The output ended. Say how the shell finished, then publish the set either way: a
        // tab whose shell exited is a tab whose row changed.
        registry.reap(&id);
        sink.changed(&registry.list());
    });
}

/// Batch reads into events.
///
/// Blocking on the first batch and draining whatever else is *already* queued, rather than
/// waiting on a timer: a keystroke's echo goes out immediately — a 16 ms tick would be felt —
/// while a program printing a screenful arrives as one event instead of hundreds.
///
/// One event carries at most [`MAX_CHUNK`]: a read that would take the batch past it waits to
/// be the next event's first batch, rather than being merged into this one. Measured, that is
/// the difference between a batch of exactly the size the reader asks for and one of 8 KiB plus
/// whatever was queued behind it.
fn pump(receiver: &Receiver<Vec<u8>>, id: &str, sink: &dyn TerminalSink) {
    // A read that did not fit in the event just sent. It goes out with the next one, so holding
    // it back costs a batch and not a delay: the loop below does not block while this is set.
    let mut held: Option<Vec<u8>> = None;

    loop {
        let mut batch = match held.take() {
            Some(bytes) => bytes,
            // Disconnected: the writer is gone, which is how the output ends.
            None => match receiver.recv() {
                Ok(bytes) => bytes,
                Err(_) => break,
            },
        };

        while batch.len() < MAX_CHUNK {
            match receiver.try_recv() {
                Ok(next) => {
                    if batch.len() + next.len() > MAX_CHUNK {
                        held = Some(next);
                        break;
                    }
                    batch.extend_from_slice(&next);
                }
                // Empty: nothing else is queued, so this batch goes now. Disconnected: this is
                // the last one, and the next `recv` ends the loop.
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }

        sink.output(&TerminalOutput {
            id: id.to_string(),
            data: BASE64.encode(&batch),
        });
    }
}

/// End a shell, and everything it started.
///
/// `SIGHUP` first, because that is what a terminal sends when it goes away and what a shell is
/// written to expect. What follows exists because the jobs outlive the shell: an interactive
/// shell — which is what a pty makes of one — gives every job a process *group* of its own
/// (`setsid` puts the shell alone in its group), so killing the shell's group reaches the shell
/// and nothing it started. The *session* is the boundary that covers them, and it is what the
/// host sweeps.
///
/// The shell's own death is not where this stops. `dash` — the `/bin/sh` of every Debian- and
/// Ubuntu-derived machine — does not pass `SIGHUP` on to its jobs, so a host that read the
/// polite death as "done" would leave them running where `bash` happened to end them.
fn terminate(session: &mut Session) {
    let _ = session.child.kill();

    // The polite signal gets its moment to work on its own: a shell that forwards it is doing
    // the cleanest version of this, and this is what tells the two cases apart.
    let shell_gone = reaped(session, KILL_GRACE);

    escalate(session);

    if !shell_gone {
        let _ = reaped(session, REAP_GRACE);
    }
}

/// Poll a child until it has exited, or the deadline passes.
fn reaped(session: &mut Session, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    loop {
        match session.child.try_wait() {
            Ok(Some(status)) => {
                session.exit = Some(signal_or_code(status));
                return true;
            }
            // A child that cannot be polled is not a child to keep waiting on.
            Err(_) => return false,
            Ok(None) => {}
        }

        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// The part a polite signal does not reach.
///
/// Three passes, smallest first: the shell, the shell's process group (its jobs, for a shell
/// with no job control), then every process still in the shell's *session*. That last one is
/// the pass this exists for: the jobs of an interactive shell are one group each, so neither
/// of the first two reaches them. It is the Linux pass: `/proc` is what lists a session's
/// members, and the two kills above are the whole of this on a platform without it.
#[cfg(unix)]
fn escalate(session: &mut Session) {
    // The id the child was spawned with: `process_id` answers only while the child is
    // unreaped, and this runs after the shell has been reaped.
    let Some(leader) = session.pid else {
        return;
    };

    // SAFETY: `kill` with a pid the host spawned and a signal number that takes no argument.
    // The calls are allowed to fail — the process may have exited since it was counted — and a
    // failure here is not something the caller can act on, since the alternative is a process
    // the user cannot see.
    unsafe {
        libc::kill(leader as i32, libc::SIGKILL);
        libc::kill(-(leader as i32), libc::SIGKILL);
    }

    #[cfg(target_os = "linux")]
    for member in session_members(leader) {
        // SAFETY: as above.
        unsafe {
            libc::kill(member as i32, libc::SIGKILL);
        }
    }
}

/// On Windows a child is a process *handle* and portable-pty's kill is `TerminateProcess`,
/// which is already decisive: there is nothing to escalate to.
#[cfg(not(unix))]
fn escalate(session: &mut Session) {
    let _ = session.child.kill();
}

/// Every process still in the session the shell led.
///
/// `setsid` makes the shell a session leader, so its pid *is* the session id — no reading of
/// `/proc` is needed to learn what to look for, only to find who is in it. A job leaves the
/// session by asking to (`setsid`, the second fork of a daemon), and one that did is left
/// alone, which is the point of the boundary.
///
/// Read-then-kill cannot name a process that has since died and been replaced: Linux hands out
/// pid numbers in a cycle, so a number comes round again only after `pid_max` — four million —
/// further spawns, and this window is the microseconds between two reads.
#[cfg(target_os = "linux")]
fn session_members(leader: u32) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        // No `/proc` to read: the two kills in `escalate` are what is left.
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| *pid != std::process::id() && session_of(*pid) == Some(leader))
        .collect()
}

/// The session a process belongs to, out of `/proc/<pid>/stat`.
///
/// The second field is the process's own name and may contain spaces and parentheses, so the
/// fields after it are found from the *last* `)`: state, ppid, pgrp, session.
#[cfg(target_os = "linux")]
fn session_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;

    stat[stat.rfind(')')? + 1..]
        .split_whitespace()
        .nth(3)?
        .parse()
        .ok()
}

/// What the platform said happened, without inventing a number.
///
/// portable-pty reports a placeholder code for a signal death (measured: `ExitStatus` carries
/// `code: 1` alongside the signal), so a death by signal is reported as a death by signal and
/// `code` stays empty. The signal's name is the platform's own text — `strsignal`: `Terminated`,
/// `Killed`, `Hangup` — which is what the panel shows, in the shell's own language.
fn signal_or_code(status: ExitStatus) -> TerminalExit {
    match status.signal() {
        Some(signal) => TerminalExit {
            code: None,
            signal: Some(signal.to_string()),
        },
        None => TerminalExit {
            code: Some(status.exit_code() as i32),
            signal: None,
        },
    }
}

/// A count clamped into a sane range.
fn clamp(value: u16, lowest: u16, highest: u16) -> u16 {
    value.max(lowest).min(highest)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sink that keeps what it was told, the way the window would have seen it.
    #[derive(Default)]
    struct Recorder {
        output: Mutex<Vec<(String, Vec<u8>)>>,
        sets: Mutex<Vec<Vec<TerminalSnapshot>>>,
    }

    impl TerminalSink for Recorder {
        fn output(&self, output: &TerminalOutput) {
            let bytes = BASE64.decode(&output.data).expect("the host posts base64");
            held(&self.output).push((output.id.clone(), bytes));
        }

        fn changed(&self, terminals: &[TerminalSnapshot]) {
            held(&self.sets).push(terminals.to_vec());
        }
    }

    /// A lock held by a test, with a poisoned lock treated as one held when a thread
    /// panicked: what is behind it is a list of records, which no panic can invalidate.
    fn held<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

    impl Recorder {
        /// Everything one terminal has produced, decoded.
        fn text(&self, id: &str) -> String {
            let chunks = held(&self.output);
            let bytes: Vec<u8> = chunks
                .iter()
                .filter(|(chunk_id, _)| chunk_id == id)
                .flat_map(|(_, bytes)| bytes.clone())
                .collect();
            String::from_utf8_lossy(&bytes).to_string()
        }

        /// Every chunk one terminal produced, in order.
        fn chunks(&self, id: &str) -> Vec<Vec<u8>> {
            held(&self.output)
                .iter()
                .filter(|(chunk_id, _)| chunk_id == id)
                .map(|(_, bytes)| bytes.clone())
                .collect()
        }
    }

    fn recorder() -> Arc<Recorder> {
        Arc::new(Recorder::default())
    }

    /// A shell that is not the developer's own: `$SHELL` can be anything, and these checks are
    /// about the pty rather than about whichever shell is configured.
    fn sh() -> Program {
        Program::new("/bin/sh")
    }

    /// Send a line and wait for `expected` to come back.
    ///
    /// Not for the line's own echo: an interactive tty echoes what is typed *before* the shell
    /// has run it, so waiting on the echo passes whatever the command did — which is how the
    /// first version of the resize check managed to pass a resize that never happened.
    fn say(terminals: &Terminals, sink: &Recorder, id: &str, line: &str, expected: &str) -> String {
        terminals.write(id, line).expect("the shell is reading");
        terminal_text(terminals, sink, id, |text| text.contains(expected))
    }

    /// Poll one terminal's output until `done` accepts it.
    fn terminal_text(
        terminals: &Terminals,
        sink: &Recorder,
        id: &str,
        mut done: impl FnMut(&str) -> bool,
    ) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let text = sink.text(id);
            if done(&text) {
                return text;
            }
            assert!(
                Instant::now() < deadline,
                "the terminal never produced what the check waited for; last output: {text:?}"
            );
            assert!(
                terminals.list().iter().any(|terminal| terminal.id == id),
                "the terminal disappeared while waiting"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn workspace() -> PathBuf {
        // The crate's own directory: it exists, it is not the directory the test binary runs
        // in, and it needs no cleanup.
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    #[cfg(target_os = "linux")]
    fn process_exists(pid: u32) -> bool {
        Path::new(&format!("/proc/{pid}")).exists()
    }

    /// Whether anything in the process table mentions `needle` in its command line.
    #[cfg(target_os = "linux")]
    fn something_runs(needle: &str) -> bool {
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return false;
        };
        for entry in entries.flatten() {
            let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else {
                continue;
            };
            let text = String::from_utf8_lossy(&cmdline).replace('\0', " ");
            if text.contains(needle) {
                return true;
            }
        }
        false
    }

    #[test]
    fn a_shell_starts_in_the_directory_it_was_given() {
        let terminals = Arc::new(Terminals::new());
        let sink = recorder();
        let opened = terminals
            .open(sink.clone(), &workspace(), 80, 24)
            .expect("a terminal opens");

        assert_eq!(opened.cwd, workspace().display().to_string());
        assert!(opened.running);
        assert!(opened.pid.is_some(), "a pty child has a pid");

        // The prompt proves a pty, not a pipe: a shell with no terminal prints nothing first.
        let text = terminal_text(&terminals, &sink, &opened.id, |text| {
            text.contains("$") || text.contains("#") || text.contains(">")
        });
        assert!(
            !text.is_empty(),
            "an interactive shell greeted the terminal: {text:?}"
        );

        // The shell's own answer, not the host's idea of where it started.
        let pwd = say(
            &terminals,
            &sink,
            &opened.id,
            "pwd\r",
            &workspace().display().to_string(),
        );
        assert!(
            pwd.contains(&workspace().display().to_string()),
            "the shell's own `pwd` is the directory it was opened in: {pwd:?}"
        );

        let list = terminals.close(&*sink, &opened.id).expect("the tab closes");
        assert!(list.is_empty(), "the closed tab is gone: {list:?}");
    }

    #[test]
    fn a_window_resize_reaches_the_shell() {
        let terminals = Arc::new(Terminals::new());
        let sink = recorder();
        let opened = terminals
            .open_program(sink.clone(), &workspace(), 80, 24, sh())
            .expect("a terminal opens");

        let first = say(&terminals, &sink, &opened.id, "stty size\r", "24 80");
        assert!(
            first.contains("24 80"),
            "the shell was told the size it was opened with: {first:?}"
        );

        terminals
            .resize(&opened.id, 100, 40)
            .expect("the terminal resizes");

        // Asked of the shell, not of the host: `stty` reads the kernel's window size for the
        // terminal it is talking to, so this fails unless the ioctl reached the pty.
        let second = say(&terminals, &sink, &opened.id, "stty size\r", "40 100");
        assert!(
            second.contains("40 100"),
            "the resize reached the shell: {second:?}"
        );

        terminals.close(&*sink, &opened.id).expect("the tab closes");
    }

    #[test]
    fn a_size_that_was_never_laid_out_is_clamped() {
        let terminals = Arc::new(Terminals::new());
        let sink = recorder();
        // A webview that has not been laid out reports zero, and a shell told it has no columns
        // wraps its prompt one character per line.
        let opened = terminals
            .open_program(sink.clone(), &workspace(), 0, 0, sh())
            .expect("a terminal opens");

        let text = say(
            &terminals,
            &sink,
            &opened.id,
            "stty size\r",
            &format!("{MIN_ROWS} {MIN_COLS}"),
        );
        assert!(
            text.contains(&format!("{MIN_ROWS} {MIN_COLS}")),
            "a zero size became the smallest usable window: {text:?}"
        );

        terminals.close(&*sink, &opened.id).expect("the tab closes");
    }

    #[test]
    fn an_exit_code_is_reported_and_the_tab_stays() {
        let terminals = Arc::new(Terminals::new());
        let sink = recorder();
        let opened = terminals
            .open_program(
                sink.clone(),
                &workspace(),
                80,
                24,
                Program::new("/bin/sh").arg("-c").arg("exit 7"),
            )
            .expect("a terminal opens");

        wait_until("the shell to exit", || {
            terminals
                .list()
                .first()
                .is_some_and(|terminal| !terminal.running)
        });

        let after = terminals.list().remove(0);
        assert_eq!(after.id, opened.id);
        assert_eq!(
            after.exit,
            Some(TerminalExit {
                code: Some(7),
                signal: None
            })
        );

        // The row is still there: a tab outlives its shell so the user can read the last
        // screenful, which is exactly what `exit` in a terminal is for.
        assert_eq!(terminals.list().len(), 1);

        // And it cannot be typed into any more, with a message rather than a refused write.
        let refused = terminals.write(&opened.id, "echo hi\r");
        assert_eq!(refused, Err("that shell has exited".to_string()));

        terminals.close(&*sink, &opened.id).expect("the tab closes");
    }

    #[test]
    fn a_death_by_signal_is_not_reported_as_an_exit_code() {
        let terminals = Arc::new(Terminals::new());
        let sink = recorder();
        terminals
            .open_program(
                sink.clone(),
                &workspace(),
                80,
                24,
                Program::new("/bin/sh").arg("-c").arg("kill -TERM $$"),
            )
            .expect("a terminal opens");

        wait_until("the shell to die of the signal", || {
            terminals
                .list()
                .first()
                .is_some_and(|terminal| !terminal.running)
        });

        let exit = terminals.list().remove(0).exit.expect("it ended");
        assert!(
            exit.signal.is_some(),
            "the platform names the signal: {exit:?}"
        );
        assert_eq!(
            exit.code, None,
            "portable-pty carries a placeholder code for a signal death, and the panel must \
             not show it as a real one: {exit:?}"
        );
    }

    #[test]
    fn a_directory_that_is_not_one_is_refused() {
        let terminals = Arc::new(Terminals::new());
        let refused = terminals.open(
            Arc::new(NullSink),
            &workspace().join("no-such-directory"),
            80,
            24,
        );

        assert!(
            refused.is_err_and(|message| message.contains("is not a directory")),
            "a shell in the wrong directory is worse than no shell"
        );
        assert!(terminals.list().is_empty(), "and nothing was left behind");
    }

    #[test]
    fn a_missing_terminal_is_an_error_and_not_a_panic() {
        let terminals = Terminals::new();

        assert!(terminals.write("pty-404", "hi").is_err());
        assert!(terminals.resize("pty-404", 80, 24).is_err());
        assert!(terminals.close(&NullSink, "pty-404").is_err());
    }

    #[test]
    fn long_output_is_batched_and_kept_whole() {
        let terminals = Arc::new(Terminals::new());
        let sink = recorder();
        let opened = terminals
            .open_program(sink.clone(), &workspace(), 80, 24, sh())
            .expect("a terminal opens");

        // 64 KiB in one command: eight times the batch cap, so this is the boundary and not
        // the common case.
        let line = "seq 1 8000\r";
        terminals
            .write(&opened.id, line)
            .expect("the shell is reading");

        let marker = "8000";
        let text = terminal_text(&terminals, &sink, &opened.id, |text| text.contains(marker));
        assert_eq!(
            text.matches("7999").count(),
            1,
            "every line arrived exactly once: {text:?}"
        );

        let chunks = sink.chunks(&opened.id);
        assert!(
            chunks.iter().all(|chunk| chunk.len() <= MAX_CHUNK),
            "no batch exceeds the cap: {:?}",
            chunks.iter().map(Vec::len).collect::<Vec<_>>()
        );
        assert!(
            chunks.len() < 8000,
            "a screenful is batches, not one event per line: {} events",
            chunks.len()
        );

        terminals.close(&*sink, &opened.id).expect("the tab closes");
    }

    #[test]
    fn closing_a_tab_ends_the_shell() {
        let terminals = Arc::new(Terminals::new());
        let sink = recorder();
        let opened = terminals
            .open(sink.clone(), &workspace(), 80, 24)
            .expect("a terminal opens");
        let pid = opened.pid.expect("a pty child has a pid");

        terminals.close(&*sink, &opened.id).expect("the tab closes");

        wait_until("the shell to be gone", || !process_exists(pid));
        assert!(terminals.list().is_empty());
        // The close announced itself: the panel must not be able to keep drawing a tab the
        // host has forgotten.
        let announced = held(&sink.sets).last().cloned().unwrap_or_default();
        assert!(
            announced.is_empty(),
            "the last set said the tabs are gone: {announced:?}"
        );
    }

    #[test]
    fn closing_a_tab_ends_what_the_shell_started() {
        let terminals = Arc::new(Terminals::new());
        let sink = recorder();
        let opened = terminals
            .open_program(sink.clone(), &workspace(), 80, 24, sh())
            .expect("a terminal opens");

        // A background job. An interactive shell gives it a process group of its own, so
        // killing the shell's group misses it, and `dash` (the `/bin/sh` of Debian and Ubuntu)
        // does not pass `SIGHUP` on to it either: the session sweep is what reaches it, on a
        // `bash` machine as much as on a `dash` one.
        let marker = "sleep 271828";
        terminals
            .write(&opened.id, &format!("{marker} &\r"))
            .expect("the shell is reading");
        wait_until("the job to start", || something_runs(marker));

        terminals.close(&*sink, &opened.id).expect("the tab closes");

        wait_until("the job to go with its terminal", || {
            !something_runs(marker)
        });
    }

    #[test]
    fn closing_a_tab_leaves_the_other_tabs_alone() {
        let terminals = Arc::new(Terminals::new());
        let sink = recorder();
        let kept = terminals
            .open_program(sink.clone(), &workspace(), 80, 24, sh())
            .expect("the first terminal opens");
        let closed = terminals
            .open_program(sink.clone(), &workspace(), 80, 24, sh())
            .expect("the second terminal opens");

        // A job in the tab that stays open: ending a tab sweeps a session, and this is the
        // process that would notice if the sweep were not bounded by it.
        let marker = "sleep 161803";
        terminals
            .write(&kept.id, &format!("{marker} &\r"))
            .expect("the shell is reading");
        wait_until("the job to start", || something_runs(marker));

        terminals.close(&*sink, &closed.id).expect("the tab closes");

        assert!(
            something_runs(marker),
            "the other tab's job is still running"
        );
        assert!(
            terminals
                .list()
                .iter()
                .any(|terminal| terminal.id == kept.id && terminal.running),
            "and its tab is still open"
        );

        terminals.close(&*sink, &kept.id).expect("the tab closes");
        wait_until("the job to go with the tab that kept it", || {
            !something_runs(marker)
        });
    }

    #[test]
    fn shutdown_ends_every_terminal() {
        let terminals = Arc::new(Terminals::new());
        let sink = recorder();
        let first = terminals
            .open(sink.clone(), &workspace(), 80, 24)
            .expect("the first terminal opens");
        let second = terminals
            .open(sink.clone(), &workspace(), 80, 24)
            .expect("the second terminal opens");
        let pids: Vec<u32> = [&first, &second]
            .iter()
            .filter_map(|terminal| terminal.pid)
            .collect();
        assert_eq!(pids.len(), 2, "both have pids");

        terminals.shutdown();

        assert!(terminals.list().is_empty(), "no tabs survive the app");
        for pid in pids {
            wait_until("the shell to be gone", || !process_exists(pid));
        }
    }
}
