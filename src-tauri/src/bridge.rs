//! Tauri commands: thin adapters over the session actor.
//!
//! Rule for this module: no business rules. Each command opens a session, asks it
//! something, or forwards an error string the UI can display. Anything that
//! decides *what* happens belongs in `session.rs` or below.
//!
//! Every session-scoped command takes the thread it is addressed to as its first
//! argument: several sessions are live at once (one sidecar each, `docs/11` D5), so
//! "the session" is not a thing this layer can assume. The id is the engine's own
//! (`get_state.sessionId`), which is what the sidebar lists and what a resume resolves
//! back to a session file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use omp_store::Store;
use omp_transport::protocol::ui::UiResponse;
use omp_transport::protocol::{commands, ImageContent};
use omp_transport::{ClientOptions, SidecarSpec};
use tauri::{AppHandle, State};

use crate::agents;
use crate::dto::{
    AgentSnapshot, AgentTranscript, ArtifactSnapshot, BranchTarget, BrokerScope, CommandSnapshot,
    ImageIn, IndexStatus, LaunchContext, ModelCatalogue, ParkedAgent, ProjectSnapshot, RowSnapshot,
    SearchHit, SessionStatus, SessionSummaryDto, TerminalSnapshot, ThreadAgents, ThreadSnapshot,
    TodoPhaseInput, TodoPhaseSnapshot, UiAnswer, UiRequestSnapshot, WorkspaceEntry,
};
use crate::favourites::Favourites;
use crate::flows;
use crate::models::Catalogue;
use crate::panel;
use crate::policy::{self, PolicyOutcome};
use crate::pty::{TerminalSink, Terminals};
use crate::search::{self, Index};
use crate::session::{self, LiveSession, Roster};
use crate::session::{agent_snapshot, row_snapshot};
use crate::threads::Threads;

/// How long the engine gets to finish cleanly before it is killed.
///
/// The value lives in [`crate::session`] because the pump needs it too, and a close and a
/// reaping that disagreed would be two answers to "how long does the engine get".
const SHUTDOWN_GRACE: Duration = session::SHUTDOWN_GRACE;

/// Tauri's managed state: every live thread, and the app's own stores.
pub struct AppState {
    /// The live threads, keyed by the engine's session id.
    ///
    /// An `Arc` rather than a plain field because each thread's pump holds the same
    /// registry as its roster: the pump is what notices a turn starting, and the roster
    /// it republishes is this map (`crate::session::Roster`).
    pub threads: Arc<Threads>,
    /// The model catalogue the picker reads (`docs/12` §7.1). App-wide rather than
    /// per-session: every session in the app is the same engine build with the same
    /// provider configuration, so they answer with the same rows.
    pub models: Catalogue,
    /// The starred models. The engine has no favourite concept, so this is ours
    /// (`docs/12` §14.3).
    pub favourites: Favourites,
    /// The engine's on-disk store, for the catalogue the sidebar browses.
    ///
    /// Resolved once at startup, because it is a property of the installation rather
    /// than of a request. `None` when the platform has no agent directory at all (no
    /// `$HOME` and no `PI_CODING_AGENT_DIR`): the browser then lists nothing, which is a
    /// smaller failure than refusing to launch.
    pub store: Option<Store>,
    /// The app's own search index (`docs/12` §7.4, decision D7).
    ///
    /// An `Arc` because a scan runs on a worker of its own and outlives the command that
    /// started it: the index is handed to `spawn_blocking`, not borrowed from the state
    /// that asked for the scan.
    pub index: Arc<Index>,
    /// The app's own terminals: one shell per tab, in a workspace (`docs/12` §11, D6).
    ///
    /// App-wide rather than per-thread, and deliberately so: a terminal is where a person
    /// goes *while* the agent works, and switching threads must not close it. What it has to
    /// do with a thread is only the directory it started in.
    ///
    /// An `Arc` because a terminal's own readers outlive the call that opened it: the thread
    /// that notices a shell exiting needs the registry the tab lives in.
    pub terminals: Arc<Terminals>,
    /// The directory the app's own files live in, or `None` when the platform has none.
    ///
    /// Kept beside the two stores it was also handed to because two flows are about *app*
    /// paths rather than store rows: an export goes under `<config dir>/exports`, and
    /// `open_path` refuses everything that is not a path the app itself owns or a
    /// directory it is already showing.
    pub config_dir: Option<PathBuf>,
}

impl AppState {
    /// Managed once at startup with the directory the app's own files live in.
    ///
    /// The stores take the directory here rather than resolving it per call: it is a
    /// property of the installation, not of a request, and both stores cache what they
    /// read from it for the life of the process.
    pub fn new(config_dir: Option<PathBuf>) -> Self {
        Self {
            threads: Arc::new(Threads::default()),
            models: Catalogue::new(config_dir.clone()),
            favourites: Favourites::new(config_dir.clone()),
            store: Store::discover(),
            // The index file goes under the same directory, and no scan is started here: the
            // first pass belongs to the app's setup, which is the first moment there is a
            // handle to announce its progress through.
            index: Arc::new(Index::new(config_dir.clone())),
            terminals: Arc::new(Terminals::new()),
            config_dir,
        }
    }
}

/// The directory the app's own files live in, or `None` when the platform has none.
///
/// Not the engine's config directory: the catalogue cache and the favourites list are
/// the app's surfaces, and reading or writing the engine's own store would tie us to a
/// schema that is not ours to hold stable. A location we cannot resolve leaves both
/// stores in memory for the process, which is a smaller failure than refusing to launch.
pub fn config_dir(app: &AppHandle) -> Option<PathBuf> {
    use tauri::Manager;

    match app.path().app_config_dir() {
        Ok(dir) => Some(dir),
        Err(error) => {
            eprintln!("[omp-desktop] no app config directory, so nothing is persisted: {error}");
            None
        }
    }
}

/// Spawn a sidecar for `workspace`, handshake, and register it as a live thread.
///
/// Several threads are live at once, one sidecar each (`docs/11` D5), so this replaces
/// nothing: the registry is keyed by the engine's session id, which only exists after
/// the handshake — and a *fresh* session's id is by construction one that is not in it.
///
/// `resume` is a session **id**, as the sidebar lists them, never a path. Measured at
/// v18.2.6: `omp --resume <path>` opens the path verbatim with no existence check, so a
/// typo silently starts an empty session, while an unknown *id* exits(1) before the
/// `ready` frame and never reaches the JSONL stream at all. Resolving the id through the
/// catalogue first is what keeps both failure modes out of the host, and it is why a
/// resume here is a catalogue lookup followed by a path.
#[tauri::command]
pub async fn open_thread(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace: String,
    resume: Option<String>,
) -> Result<ThreadSnapshot, String> {
    // A thread that is already live is answered with what it already is: clicking the
    // row of a running session must not start a second sidecar on the same conversation.
    if let Some(id) = &resume {
        if let Some(live) = state.threads.get(id) {
            return Ok(live.snapshot());
        }
    }

    let mut spec = SidecarSpec::omp(&workspace);
    if let Some(id) = &resume {
        let store = state.store.clone();
        let id = id.clone();
        // Resolved on a blocking task: the store scan opens two windows of every session
        // file, and the async worker doing it is a worker not answering commands.
        let path = scan(move || session_path(store.as_ref(), &id)).await?;
        spec = spec.resuming(&path);
    }

    // Captured before launching: when a packaged app cannot find its sidecar, the
    // path it tried is the whole diagnosis.
    let tried = spec.program.display().to_string();
    let roster: Arc<dyn Roster> = state.threads.clone();

    let live = session::open(&spec, ClientOptions::default(), app.clone(), roster)
        .await
        .map_err(|error| format!("{error}\n  tried: {tried}"))?;

    let snapshot = live.snapshot();

    // What came back has to be the session that was asked for. `--resume <path>` opens the
    // path verbatim with **no existence check** (measured at v18.2.6), so a session file that
    // was deleted, moved or replaced underneath the window comes back as a brand-new empty
    // session — and showing that as the thread the user clicked is how a lost conversation
    // looks exactly like an empty one. The sidecar is stopped before the error is returned:
    // the caller gets nothing to address it with, so leaving it running would be an orphan.
    if let Some(id) = &resume {
        let answered = live.thread.current();
        if &answered != id {
            live.shutdown(SHUTDOWN_GRACE).await;

            return Err(format!(
                "the agent opened the file for session {id} as a different session ({answered}), \
                 so the conversation it holds is not that one. Its session file is gone or was \
                 replaced."
            ));
        }
    }

    // An id that is somehow already live means the engine handed back a session this
    // registry is holding — the guard above cannot see a fresh session's id, so this is
    // the one place it can still happen. The displaced sidecar is *stopped*: dropping the
    // handle would leave a ~200 MB agent running with nothing left to address it.
    if let Some(previous) = state.threads.remove(&snapshot.id) {
        previous.shutdown(SHUTDOWN_GRACE).await;
    }

    // Old RPC sessions may predate automatic naming. If one already has a real user
    // message but no title, opening it is enough to let OMP's own generator repair it.
    let should_generate_title = resume.is_some()
        && snapshot.title.is_none()
        && live.rows().is_ok_and(|rows| {
            rows.iter()
                .any(|row| row.role == "user" && !row.text.trim().is_empty())
        });

    state.threads.insert(Arc::clone(&live));
    // A thread that is live again is not suspended: `insert` clears the id the engine
    // answered with, and this clears the one the window asked for — the two are the same id
    // (the check above refuses anything else), and a resume is the only path that removes it.
    if let Some(id) = &resume {
        state.threads.clear_suspension(id);
    }
    state.threads.publish(&app);

    if should_generate_title {
        let roster: Arc<dyn Roster> = state.threads.clone();
        session::generate_title_if_unnamed(live, app.clone(), roster);
    }

    if let Some(client) = state.threads.get(&snapshot.id).and_then(|t| t.client()) {
        let app_handle = app.clone();
        let models = state.models.clone();
        tauri::async_runtime::spawn(async move {
            let _ = models
                .refresh(&app_handle, crate::models::fetch(&client))
                .await;
        });
    }

    Ok(snapshot)
}

/// The session file an id names, through the engine's own catalogue.
///
/// The two facts this exists for are both measured at v18.2.6: `--resume` takes a path
/// with no existence check (so a wrong path is a silently empty session), and an unknown
/// id is reported on stderr with `exit(1)` **before** the `ready` frame — i.e. as a
/// dead sidecar rather than as a JSONL error the host could read.
///
/// Public for the same reason [`session_catalogue`] is: it is the whole of the decision
/// `open_thread` makes about a resume, and a live test drives it without a window.
pub fn session_path(store: Option<&Store>, id: &str) -> Result<String, String> {
    let store = store.ok_or_else(|| {
        "the engine's session store is not available, so a session cannot be resumed".to_string()
    })?;

    let session = store
        .find(id)
        .ok_or_else(|| format!("no session with id {id}"))?;

    Ok(session.path.display().to_string())
}

/// Run a blocking store scan off the async workers.
///
/// The engine's store is read by opening two windows of every session file
/// (`crates/omp-store`), on files that can be megabytes. An async worker parked in that
/// scan is a worker not answering commands, so the scan gets a thread of its own.
async fn scan<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| format!("the store scan did not finish: {error}"))?
}

/// Change the session's approval mode (`docs/12` §7.2).
///
/// Two halves, in this order: record the choice for the sessions that follow, then
/// restart this thread's sidecar so it takes effect now. The restart is a *resume* — same
/// session file, new `--approval-mode` — and it is the only way: measured in 8b, the engine
/// has no runtime setter, and the config record is read when a session is constructed.
///
/// What it costs is stated rather than hidden: the conversation comes back (the host
/// hydrates it through the same path any resume takes — `session::open`, which restores
/// history whenever the spec resumes a file), a turn in flight does not.
#[tauri::command]
pub async fn set_approval_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    thread: String,
    mode: String,
) -> Result<SessionStatus, String> {
    // Validated before anything is written or stopped, so a typo cannot leave the
    // config changed and the session restarted for nothing.
    policy::validate_mode(&mode)?;
    policy::record_mode(&mode).await?;

    let live = thread_of(&state, &thread)?;

    let workspace = live.workspace.clone();
    let session_file = live.session_file().ok_or_else(|| {
        "the engine did not name a session file, so this session cannot be resumed".to_string()
    })?;

    // The registry loses the thread before its sidecar is stopped: a command arriving
    // mid-restart is refused rather than written to a pipe that is going away.
    state.threads.remove(&thread);
    live.shutdown(SHUTDOWN_GRACE).await;

    let spec = SidecarSpec::omp(&workspace)
        .with_approval_mode(&mode)
        .resuming(&session_file);
    let tried = spec.program.display().to_string();
    let roster: Arc<dyn Roster> = state.threads.clone();
    let next = session::open(&spec, ClientOptions::default(), app.clone(), roster)
        .await
        .map_err(|error| format!("{error}\n  tried: {tried}"))?;

    let status = next.status()?;
    // Keyed by the new session's own id, which a resume of the same file keeps — and if
    // an engine ever handed back a different one, the registry would be right and the
    // returned status would say so (`control.sessionId`).
    state.threads.insert(next);
    state.threads.publish(&app);

    Ok(status)
}

/// The current status of one thread, for a pane that polls.
#[tauri::command]
pub async fn thread_status(
    state: State<'_, AppState>,
    thread: String,
) -> Result<SessionStatus, String> {
    thread_of(&state, &thread)?.status()
}

/// The conversation so far, as rows.
#[tauri::command]
pub async fn transcript(
    state: State<'_, AppState>,
    thread: String,
) -> Result<Vec<RowSnapshot>, String> {
    thread_of(&state, &thread)?.rows()
}

/// Send a prompt.
///
/// A text-only prompt needs no approval round-trip. A prompt that makes the agent
/// reach for a write-class tool blocks until the approval dialog is answered
/// (`docs/12` §10) — the composer stays usable meanwhile, because steering is a
/// separate command.
#[tauri::command]
pub async fn prompt(
    state: State<'_, AppState>,
    thread: String,
    message: String,
    images: Option<Vec<ImageIn>>,
) -> Result<(), String> {
    session::prompt(thread_of(&state, &thread)?, message, &wire_images(images)).await
}

/// Inject a message into the turn that is already running.
///
/// `docs/12` §5.2: while streaming, `Enter` steers. Measured at v18.2.6: the engine
/// accepts a steering message with **no** turn running and starts one, so the
/// composer never has to second-guess its own (slightly stale) idea of whether a
/// turn is in flight.
#[tauri::command]
pub async fn steer(
    state: State<'_, AppState>,
    thread: String,
    message: String,
    images: Option<Vec<ImageIn>>,
) -> Result<(), String> {
    ask(
        &state,
        &thread,
        commands::steer(message, &wire_images(images)),
        "steering message",
    )
    .await
}

/// Queue a message to run after the current turn finishes.
#[tauri::command]
pub async fn follow_up(
    state: State<'_, AppState>,
    thread: String,
    message: String,
    images: Option<Vec<ImageIn>>,
) -> Result<(), String> {
    ask(
        &state,
        &thread,
        commands::follow_up(message, &wire_images(images)),
        "queued message",
    )
    .await
}

/// Stop the turn in flight.
///
/// Goes through the session rather than straight to `abort` because a run parked on
/// an approval has to be refused before it can be stopped — `LiveSession::stop_turn`
/// carries the measurement behind that.
#[tauri::command]
pub async fn stop_turn(state: State<'_, AppState>, thread: String) -> Result<(), String> {
    thread_of(&state, &thread)?.stop_turn().await
}

/// Stop the turn in flight and send a message in its place.
#[tauri::command]
pub async fn stop_turn_and_send(
    state: State<'_, AppState>,
    thread: String,
    message: String,
    images: Option<Vec<ImageIn>>,
) -> Result<(), String> {
    thread_of(&state, &thread)?
        .stop_turn_and_send(message, &wire_images(images))
        .await
}

/// The wire images for a command, in the order the composer attached them.
///
/// Order is the engine's: a message's attachments are numbered `[Image #N]` in the
/// order they arrive, so reordering here would attach a picture to the wrong sentence.
fn wire_images(images: Option<Vec<ImageIn>>) -> Vec<ImageContent> {
    images
        .unwrap_or_default()
        .into_iter()
        .map(|image| ImageContent::new(image.mime_type, image.data))
        .collect()
}

/// The live thread a command was addressed to, or the reason it cannot be acted on.
///
/// The error names the id: with several threads live, "no thread …" from a sidebar row
/// the host has already closed is a different problem from a command aimed at the wrong
/// window, and only the id tells them apart.
fn thread_of(state: &AppState, thread: &str) -> Result<Arc<LiveSession>, String> {
    state
        .threads
        .get(thread)
        .ok_or_else(|| format!("no live thread {thread}"))
}

/// Any live thread, for a command whose answer is about the engine rather than about one
/// conversation — the model catalogue (`docs/12` §7.1). With none live there is nothing
/// to ask, which is the error.
fn any_thread(state: &AppState) -> Result<Arc<LiveSession>, String> {
    state
        .threads
        .any()
        .ok_or_else(|| "no session is open".to_string())
}

/// Send one command whose whole outcome is its `response` frame.
///
/// Every composer action has this shape; the body itself lives in [`session::send`],
/// because the chip row's commands need the same treatment and one of them
/// (`set_auto_compaction`) is not a composer action at all.
async fn ask(
    state: &AppState,
    thread: &str,
    command: serde_json::Value,
    what: &str,
) -> Result<(), String> {
    let live = thread_of(state, thread)?;
    let client = live
        .client()
        .ok_or_else(|| "the session is shutting down".to_string())?;

    session::send(&client, command, what).await
}

/// The commands the palette offers (`docs/12` §7.3).
///
/// Engine answers, so this needs a session — and they are *session* answers rather than
/// app-wide ones, because two of the four sources (`file`, `custom`) come from the
/// workspace. The session caches them after the first fetch; a later
/// `available_commands_update` refreshes that cache and announces it on
/// `commands-updated`.
#[tauri::command]
pub async fn available_commands(
    state: State<'_, AppState>,
    thread: String,
) -> Result<Vec<CommandSnapshot>, String> {
    thread_of(&state, &thread)?.commands().await
}

/// The model catalogue as the picker reads it: the cache, never the network.
///
/// `docs/12` §7.1: the picker opens against this immediately, so it must not be able to
/// wait on a fetch. The first call in a process reads the cache file once; after that it
/// is the rows in memory.
#[tauri::command]
pub fn models(state: State<'_, AppState>) -> Result<ModelCatalogue, String> {
    state.models.snapshot()
}

/// Fetch the catalogue out of band, then announce it on `models-updated`.
///
/// Resolves when *this* fetch lands — or immediately when a fetch is already in flight,
/// in which case the in-flight one announces the result. Either way the event, not this
/// call, is what tells a picker its rows changed (`crate::models::Catalogue::refresh`).
///
/// The catalogue is an engine answer, so there is nothing to fetch without a session:
/// with none open this refuses with `no session is open` and announces nothing, leaving
/// the picker on the cached rows [`models`] already gave it. Any live thread will do, and
/// deliberately so — the rows are the engine build's and its provider configuration's,
/// not one conversation's, which is why this stays app-wide while everything else here
/// takes a thread.
#[tauri::command]
pub async fn refresh_models(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let live = any_thread(&state)?;
    let client = live
        .client()
        .ok_or_else(|| "the session is shutting down".to_string())?;

    state
        .models
        .refresh(&app, crate::models::fetch(&client))
        .await
}

/// Make `provider/id` the thread's model (`docs/12` §7.1).
#[tauri::command]
pub async fn set_model(
    state: State<'_, AppState>,
    thread: String,
    provider: String,
    model_id: String,
) -> Result<(), String> {
    let live = thread_of(&state, &thread)?;

    crate::models::set_model(&live, &provider, &model_id).await
}

/// Set the thread's thinking level (`docs/12` §7.6).
#[tauri::command]
pub async fn set_thinking_level(
    state: State<'_, AppState>,
    thread: String,
    level: String,
) -> Result<(), String> {
    let live = thread_of(&state, &thread)?;

    crate::models::set_thinking_level(&live, &level).await
}

/// Turn the engine's automatic compaction on or off (`docs/12` §7.5).
#[tauri::command]
pub async fn set_auto_compaction(
    state: State<'_, AppState>,
    thread: String,
    enabled: bool,
) -> Result<(), String> {
    let live = thread_of(&state, &thread)?;

    crate::models::set_auto_compaction(&live, enabled).await
}

/// Compact now, with the context popover's optional instructions (`docs/12` §7.5).
#[tauri::command]
pub async fn compact(
    state: State<'_, AppState>,
    thread: String,
    instructions: Option<String>,
) -> Result<(), String> {
    let live = thread_of(&state, &thread)?;

    crate::models::compact(&live, instructions.as_deref()).await
}

/// Rename a session (`docs/12` §2.3).
///
/// The engine answers a bare ack and emits nothing, so nothing arrives to correct a stale
/// name — `flows::rename` re-reads the state the chrome renders, and the caller refreshes
/// the catalogue, which is what reads the title slot the engine rewrote in place. An empty
/// name is refused in the host with the engine's own sentence, so the UI never makes that
/// round trip.
#[tauri::command]
pub async fn rename_thread(
    state: State<'_, AppState>,
    thread: String,
    name: String,
) -> Result<(), String> {
    let live = thread_of(&state, &thread)?;

    flows::rename(&live, &name).await
}

/// The user messages a fork can start from (`docs/12` §6.3).
///
/// Engine answers, and the only valid entry ids there are: a picker built from the
/// transcript would have nothing valid to send.
#[tauri::command]
pub async fn branch_targets(
    state: State<'_, AppState>,
    thread: String,
) -> Result<Vec<BranchTarget>, String> {
    let live = thread_of(&state, &thread)?;

    flows::branch_targets(&live).await
}

/// Fork the session at one of its messages, returning the thread that replaced it.
///
/// The engine forks **in place** — a new session in the same sidecar, the old id gone —
/// so what comes back is a different thread's row and the window switches to it. The
/// command needs the app because the registry has to move with the id and the sidebar has
/// to hear about it (`flows::branch`).
#[tauri::command]
pub async fn branch_thread(
    app: AppHandle,
    state: State<'_, AppState>,
    thread: String,
    entry_id: String,
) -> Result<ThreadSnapshot, String> {
    let live = thread_of(&state, &thread)?;

    flows::branch(&state.threads, &app, &live, &entry_id).await
}

/// Hand the session over, optionally with instructions (`docs/12` §6.4).
///
/// Not an export: the document is committed as a compaction entry on **this** session and
/// no file is written (measured — the RPC path never sets `savedPath`). The caller re-reads
/// the transcript, which is where the maintenance appears; this re-reads the control state,
/// which is where the context usage moves.
#[tauri::command]
pub async fn handoff_thread(
    state: State<'_, AppState>,
    thread: String,
    instructions: Option<String>,
) -> Result<(), String> {
    let live = thread_of(&state, &thread)?;

    flows::handoff(&live, instructions.as_deref()).await
}

/// Export the session to HTML, returning the absolute path the app wrote it to.
///
/// The path is the host's, not the engine's: left to itself, `export_html` resolves
/// `omp-session-<stem>.html` against the **sidecar's** working directory, which is the
/// user's project. It is also the path `open_path` will accept afterwards, which is why
/// the app chooses it rather than trusting the answer (`flows::export`).
#[tauri::command]
pub async fn export_html(state: State<'_, AppState>, thread: String) -> Result<String, String> {
    let live = thread_of(&state, &thread)?;

    flows::export(&live, state.config_dir.as_deref()).await
}

/// Pin or unpin a session (`docs/12` §2.3).
///
/// Dispatched as the engine's own `/pin <id>` through a live thread, which is why `thread`
/// is required: the pin set is written under the engine's cross-process lock, so OMP's
/// `session-pins.json` stays the single source of truth and the app only mirrors it by
/// re-reading (`omp-store::pins`).
#[tauri::command]
pub async fn pin_session(
    state: State<'_, AppState>,
    thread: String,
    id: String,
) -> Result<(), String> {
    let live = thread_of(&state, &thread)?;

    flows::pin(&live, &id).await
}

/// The starred models, in the user's order (`docs/12` §7.1).
#[tauri::command]
pub fn favourites(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state.favourites.list()
}

/// Replace the starred models, returning what was stored.
///
/// The return is not decoration: a repeated key is dropped, so the stored list can be
/// shorter than the one sent, and the window should render what actually landed.
#[tauri::command]
pub fn set_favourites(
    state: State<'_, AppState>,
    keys: Vec<String>,
) -> Result<Vec<String>, String> {
    state.favourites.set(keys)
}

/// Record "always allow" for one tool, so the next session stops asking about it.
///
/// The engine reads its policy record when it builds a session, so this cannot
/// quieten the dialog that is open right now — [`crate::policy`] carries the
/// measurement. What comes back is the write itself, and the dialog tells the user
/// that it applies from the next session on rather than pretending otherwise.
#[tauri::command]
pub async fn allow_tool(tool: String) -> Result<PolicyOutcome, String> {
    crate::policy::allow_tool(&tool).await
}

/// Open a URL in the user's default application.
///
/// The conversation's markdown hands model-authored URLs here, which is why the
/// allowlist lives in [`crate::external`] rather than in the webview: the frontend
/// asks, the host decides, and a refusal comes back as a message the window can
/// show.
#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    crate::external::open(&url)
}

/// Open a file or directory in the user's default application.
///
/// The other door out of the app, and deliberately not `open_external`'s: that one takes a
/// URL and refuses `file:` (the allowlist for what a *model-authored link* may launch),
/// while this one takes a path from the window and admits only the app's own export
/// directory and directories it is already showing. Which paths those are is decided here
/// (`flows::Allowlist`), from the live threads and the catalogue rather than from anything
/// the caller sent.
///
/// On a blocking worker because building the allowlist reads the store, which opens two
/// windows of every session file.
#[tauri::command]
pub async fn open_path(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let store = state.store.clone();
    let threads = Arc::clone(&state.threads);
    let exports = flows::exports_dir(state.config_dir.as_deref());

    scan(move || {
        let allowlist = flows::Allowlist::collect(store.as_ref(), &threads, exports);

        allowlist.open(&path)
    })
    .await
}

// ------------------------------------------------- the right panel (`docs/12` §8)

/// The thread's plan, as the Todos tab renders it.
///
/// Re-read from `get_state` for a live thread — the panel opens at any moment, so the
/// cached control may predate the engine's last mutation — and answered from the control the
/// thread was last read at when it is not live any more, because there is nothing left to
/// ask. The plan has no event of its own (measured: the engine's stream carries
/// `todo_reminder` and `todo_auto_clear` and nothing else todo-shaped), so this command and
/// the `todo` tool's stale trigger ([`omp_session::SessionControl::apply`]) are the two ways
/// the panel hears that it moved.
#[tauri::command]
pub async fn thread_todos(
    state: State<'_, AppState>,
    thread: String,
) -> Result<Vec<TodoPhaseSnapshot>, String> {
    let phases = match state.threads.get(&thread) {
        Some(live) => live.todo_phases().await,
        None => {
            state
                .threads
                .cached_control(&thread)
                .ok_or_else(|| {
                    format!(
                        "{thread} is not open, and its plan was never read while it was: \
                         reopen the session to see its todos"
                    )
                })?
                .todo_phases
        }
    };

    Ok(panel::phases(&phases))
}

/// Replace the thread's plan, answering with the engine's own list.
///
/// The answer is rendered, never the request: the engine stores what it is given and echoes
/// its own projection of it, so the window can never show a phase or a task the engine did
/// not keep. That is not normalisation — nothing is corrected on the way through, with the
/// single exception of an empty list, which [`panel::outgoing`] refuses to send.
///
/// Editing needs a live session, which is why this is the one panel command that does not
/// fall back to a remembered control (`docs/12` §8.1's read-only suspended mode).
#[tauri::command]
pub async fn set_todos(
    state: State<'_, AppState>,
    thread: String,
    phases: Vec<TodoPhaseInput>,
) -> Result<Vec<TodoPhaseSnapshot>, String> {
    let live = thread_of(&state, &thread)?;
    let stored = live.set_todo_phases(panel::outgoing(&phases)?).await?;

    Ok(panel::phases(&stored))
}

/// One level of the thread's workspace, measured rather than guessed.
///
/// `path` is absolute and inside the thread's own working directory, or absent for that
/// directory itself; [`panel::tree`] is where both of those rules live, symlinks and all.
#[tauri::command]
pub async fn workspace_tree(
    state: State<'_, AppState>,
    thread: String,
    path: Option<String>,
) -> Result<Vec<WorkspaceEntry>, String> {
    let root = thread_root(&state, &thread).await?;

    // On a blocking worker: a directory can be large, or on a network filesystem, and an
    // async worker parked in `read_dir` is a worker not answering the rest of the window.
    scan(move || panel::tree(&root, path.as_deref())).await
}

/// Read one tool result the engine spilled to `artifact://<id>` (`docs/12` §8.2).
///
/// The file is resolved the way the engine resolves it — the session's `.jsonl` path minus
/// that suffix is the artifacts directory, and `<id>.` names the file inside it — and the id
/// must be purely digits, so what arrives from a tool card cannot name a path outside it.
#[tauri::command]
pub async fn read_artifact(
    state: State<'_, AppState>,
    thread: String,
    id: String,
) -> Result<ArtifactSnapshot, String> {
    let session_file = thread_session_file(&state, &thread).await?;
    let directory = panel::artifacts_dir(&session_file)?;

    scan(move || panel::artifact(&directory, &id)).await
}

// ------------------------------------------------- the agents panel (`docs/12` §9)

/// Every agent the app can account for, one entry per thread.
///
/// Two sources, joined here rather than in the window: the live threads' rosters, which the
/// engine is still reporting on, and the rosters of threads that have left the registry,
/// which only the cache remembers. A thread with no agents is left out — the sidebar's
/// `Active agents N` is the count, and the panel is the list of who they *are*, so a row
/// saying "nothing" is noise the count already answered.
///
/// Polled rather than pushed for the thread it names one at a time: the frames push the
/// roster the moment it changes ([`AGENTS_EVENT`]), and this is what the panel reads when it
/// opens, before any change has happened.
#[tauri::command]
pub async fn agents(state: State<'_, AppState>) -> Result<Vec<ThreadAgents>, String> {
    let mut threads: Vec<ThreadAgents> = Vec::new();

    for live in state.threads.live() {
        let agents: Vec<AgentSnapshot> = live.agents().await.iter().map(agent_snapshot).collect();
        if agents.is_empty() && live.agents_error.is_none() {
            continue;
        }

        threads.push(ThreadAgents {
            thread: live.thread.current(),
            agents,
            error: live.agents_error.clone(),
        });
    }

    for id in state.threads.closed_ids() {
        let Some(agents) = state.threads.cached_agents(&id) else {
            continue;
        };
        if agents.is_empty() {
            continue;
        }

        threads.push(ThreadAgents {
            thread: id,
            agents: agents.iter().map(agent_snapshot).collect(),
            // A closed thread's roster was read while it had an engine, so whatever it
            // said about being unable to read one no longer applies to it.
            error: None,
        });
    }

    Ok(threads)
}

/// The subagents a thread's session file shows on disk (`docs/12` §9's parked rows).
///
/// The engine's roster is live-only, so everything a session spawned and settled is found
/// here instead — the transcripts a subagent wrote itself, beside the parent's artifacts.
/// Read-only and engine-free: it works for a session this window has never opened.
#[tauri::command]
pub async fn parked_agents(
    state: State<'_, AppState>,
    thread: String,
) -> Result<Vec<ParkedAgent>, String> {
    let session_file = thread_session_file(&state, &thread).await?;

    // On a blocking worker: this reads a directory and the head of every file in it.
    scan(move || agents::parked(&session_file)).await
}

/// One page of a subagent's transcript, by byte cursor (`docs/12` §9).
///
/// The engine answers while it still owns the id, and stops the moment its registry is
/// cleared — a fork, a switch or a restart. So the engine is asked first and the file it
/// would have read is read directly when it will not: same JSONL, same page rules
/// (`omp_session::read_transcript`), and `source` says which of the two answered, because a
/// page from a file the engine has forgotten is not the same claim as one it served.
///
/// Rows come back as the app's own [`RowSnapshot`], so a subagent's transcript renders
/// through the same conversation rows as the thread that spawned it.
#[tauri::command]
pub async fn agent_messages(
    state: State<'_, AppState>,
    thread: String,
    agent: String,
    from_byte: Option<u64>,
) -> Result<AgentTranscript, String> {
    let from = from_byte.unwrap_or_default();
    let session_file = thread_session_file(&state, &thread).await?;
    let live = state.threads.get(&thread);

    if let Some(live) = &live {
        if let Some(page) = live.agent_messages(&agent, from).await {
            return Ok(AgentTranscript {
                agent,
                path: page.session_file,
                next_byte: page.next_byte,
                reset: page.reset,
                source: "engine".to_string(),
                rows: page.rows,
            });
        }
    }

    let path = agents::parked_by_id(&session_file, &agent)?.path;

    let reading = path.clone();
    let page = scan(move || {
        omp_session::read_transcript(std::path::Path::new(&reading), from)
            .map_err(|error| format!("`{reading}` could not be read: {error}"))
    })
    .await?;

    Ok(AgentTranscript {
        agent,
        path,
        next_byte: page.next_byte,
        reset: page.reset,
        source: "file".to_string(),
        rows: page
            .messages
            .iter()
            .map(|message| row_snapshot(&omp_session::Row::Message(message.clone())))
            .collect(),
    })
}

/// Every broker-owned process the engine is supervising (`docs/12` §9)
///
/// `omp ps` is the only supported way to see or stop these, and it runs with the app's own
/// pinned binary so a second engine version's daemons are not reported as this app's. The
/// directory it runs in is the thread's when one is named — that is the scope the engine
/// reports first — and the app's own config directory otherwise.
#[tauri::command]
pub async fn broker_processes(
    state: State<'_, AppState>,
    thread: Option<String>,
) -> Result<Vec<BrokerScope>, String> {
    let cwd = match &thread {
        Some(thread) => thread_root(&state, thread).await?,
        None => state
            .config_dir
            .as_ref()
            .map(|directory| directory.display().to_string())
            .unwrap_or_default(),
    };

    agents::broker_scopes(&cwd).await
}

/// Stop one broker-owned process (`docs/12` §9).
///
/// The only supported path, and therefore the one the panel takes: a daemon the broker owns
/// is restarted by that broker if it just dies, so killing its pid would be undone and would
/// also leave the broker believing it is still there. `omp ps` is what tells the broker.
#[tauri::command]
pub async fn stop_broker_process(
    state: State<'_, AppState>,
    thread: Option<String>,
    name: String,
) -> Result<String, String> {
    let cwd = match &thread {
        Some(thread) => thread_root(&state, thread).await?,
        None => state
            .config_dir
            .as_ref()
            .map(|directory| directory.display().to_string())
            .unwrap_or_default(),
    };

    agents::stop_broker(&cwd, &name).await
}

/// The directory a thread's tree is rooted at.
///
/// The live session's own workspace while one is running, and the `cwd` the engine recorded
/// in the session's header once it is not — so a suspended thread still browses the project
/// it was working in. Both come from the engine's side of the house rather than from the
/// caller: a root the window could name would make this a browser for the whole disk.
async fn thread_root(state: &AppState, thread: &str) -> Result<String, String> {
    if let Some(live) = state.threads.get(thread) {
        return Ok(live.workspace.clone());
    }

    let store = state.store.clone();
    let id = thread.to_string();

    scan(move || session_cwd(store.as_ref(), &id)).await
}

/// The session file a thread's artifacts live beside.
///
/// Three sources, in the order that avoids work: the live session's own `get_state`, the
/// control the registry remembers for a thread that has closed, and the catalogue — which is
/// the only source for a session nothing in this window has opened.
async fn thread_session_file(state: &AppState, thread: &str) -> Result<String, String> {
    if let Some(live) = state.threads.get(thread) {
        return live.session_file().ok_or_else(|| {
            "this session runs without a session store, so it has no artifacts".to_string()
        });
    }

    if let Some(file) = state
        .threads
        .cached_control(thread)
        .and_then(|control| control.session_file)
    {
        return Ok(file);
    }

    let store = state.store.clone();
    let id = thread.to_string();

    scan(move || session_path(store.as_ref(), &id)).await
}

/// The working directory a session was created in, through the engine's own catalogue.
///
/// Read out of the session file's header (`omp_store` parses it, the engine wrote it), which
/// is the only record of it that outlives the sidecar.
fn session_cwd(store: Option<&Store>, id: &str) -> Result<String, String> {
    let store = store.ok_or_else(|| {
        "the engine's session store is not available, so this session's workspace cannot be \
         found"
            .to_string()
    })?;

    let session = store
        .find(id)
        .ok_or_else(|| format!("no session with id {id}"))?;

    if session.cwd.is_empty() {
        return Err(format!(
            "the engine recorded no working directory for the session {id}"
        ));
    }

    Ok(session.cwd)
}

/// The dialogs one thread is waiting on, for a pane that re-reads on change.
#[tauri::command]
pub async fn ui_requests(
    state: State<'_, AppState>,
    thread: String,
) -> Result<Vec<UiRequestSnapshot>, String> {
    thread_of(&state, &thread)?.pending_ui_requests()
}

/// Answer a pending dialog.
///
/// The answer is a shape the frontend can produce in one of three ways, and this
/// resolves it to the one the engine is waiting for. Refusals come back as errors
/// the UI can act on: an unknown dialog means the engine has already moved on
/// (`docs/12` §10), so the dialog should close rather than retry.
#[tauri::command]
pub async fn respond_ui_request(
    state: State<'_, AppState>,
    thread: String,
    request_id: String,
    answer: UiAnswer,
) -> Result<(), String> {
    let live = thread_of(&state, &thread)?;

    live.respond(&request_id, resolve_answer(&answer)?).await
}

/// The one response the answer's shape can mean.
///
/// An answer carrying none of the three fields is refused rather than defaulted:
/// a `select` answered with an empty choice, or a `confirm` answered with a
/// silence read as "yes", is worse than an error the UI can show.
fn resolve_answer(answer: &UiAnswer) -> Result<UiResponse, String> {
    if answer.cancelled == Some(true) {
        return Ok(UiResponse::Cancelled {
            timed_out: answer.timed_out.unwrap_or(false),
        });
    }
    if let Some(confirmed) = answer.confirmed {
        return Ok(UiResponse::Confirmed(confirmed));
    }
    if let Some(value) = &answer.value {
        return Ok(UiResponse::Value(value.clone()));
    }

    Err("an answer must carry one of `value`, `confirmed` or `cancelled`".to_string())
}

/// What the app was launched with, so the window can open a project directly.
#[tauri::command]
pub async fn launch_context(state: State<'_, LaunchState>) -> Result<LaunchContext, String> {
    Ok(state.context.clone())
}

/// Managed alongside [`AppState`]: the arguments the process started with.
#[derive(Default)]
pub struct LaunchState {
    pub context: LaunchContext,
}

/// Close one thread, letting its engine finish cleanly.
///
/// The registry loses the thread before its sidecar is stopped, so a command that races
/// the close is refused instead of being written to a pipe that is going away — and so is
/// a second close of the same row, which is the correct answer for a stale click.
#[tauri::command]
pub async fn close_thread(
    app: AppHandle,
    state: State<'_, AppState>,
    thread: String,
) -> Result<(), String> {
    let live = state
        .threads
        .remove(&thread)
        .ok_or_else(|| format!("no live thread {thread}"))?;

    live.shutdown(SHUTDOWN_GRACE).await;
    state.threads.publish(&app);

    Ok(())
}

/// Every thread with a live sidecar right now.
#[tauri::command]
pub async fn threads(state: State<'_, AppState>) -> Result<Vec<ThreadSnapshot>, String> {
    Ok(state.threads.snapshots())
}

/// The engine's session catalogue, as the sidebar browses it.
///
/// Needs no live session, so it answers with nothing open — which is the state the app
/// starts in, and the state a user reads the catalogue from before opening anything.
/// A store this process could not find answers with an empty list rather than an error:
/// no agent directory means no sessions, and "nothing here yet" is a window that still
/// works.
#[tauri::command]
pub async fn sessions(state: State<'_, AppState>) -> Result<Vec<SessionSummaryDto>, String> {
    let store = state.store.clone();
    let threads = Arc::clone(&state.threads);

    scan(move || Ok(session_catalogue(store.as_ref(), &threads))).await
}

/// Tell the host which thread the window is showing (`docs/11` D5).
///
/// The idle policy's one guard this process cannot derive: *which* thread a person is looking
/// at is a fact about the window, so the window says it and the registry keeps it. `None` is
/// "no thread in particular" — a closed column, a window with nothing selected — and it
/// excludes nothing, because the other three guards still hold.
///
/// Nothing else reads it: it is not a preference (`docs/12` §13: the host decides nothing about
/// interrupting a person) and not a setting. It exists so that a thread someone is reading is
/// never released from under them.
#[tauri::command]
pub async fn set_focused_thread(
    state: State<'_, AppState>,
    thread: Option<String>,
) -> Result<(), String> {
    state.threads.focus(thread);

    Ok(())
}

/// The project groups, derived from the sessions' own working directories.
#[tauri::command]
pub async fn projects(state: State<'_, AppState>) -> Result<Vec<ProjectSnapshot>, String> {
    let store = state.store.clone();

    scan(move || Ok(project_groups(store.as_ref()))).await
}

/// Search every session the app has indexed (`docs/12` §7.4).
///
/// The app's own index, not the engine's: the engine has no cross-thread search at v18.2.6, and
/// a cold session — one nobody has opened — has no sidecar to ask at all. This reads the FTS5
/// table in the app's own config directory, so it is a query rather than a store scan: no
/// blocking worker, and the clamp on `limit` is what keeps one broad query from becoming a
/// megabyte of JSON over the IPC boundary (`MAX_LIMIT`).
///
/// An index that cannot be read answers with an empty list, which is also what a query matching
/// nothing looks like: the caller is a search box being typed into, where an error is a message
/// with no action behind it.
#[tauri::command]
pub async fn search(
    state: State<'_, AppState>,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<SearchHit>, String> {
    Ok(state.index.search(&query, limit))
}

/// How far the app's own index has got, for the overlay's footer (`docs/12` §7.4).
#[tauri::command]
pub async fn index_status(state: State<'_, AppState>) -> Result<IndexStatus, String> {
    Ok(state.index.status())
}

/// Index the store again, on a worker of its own (`docs/12` §7.4).
///
/// Returns as soon as the pass has been *started*: a full pass reads every session file, and a
/// command that waited for it would be the window that stops answering while it runs — the one
/// thing §7.4 rules out. Progress arrives on `index-progress` instead. A pass already in flight
/// makes this a no-op rather than a second one, so a repeated "rebuild" is cheap.
#[tauri::command]
pub async fn reindex(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    search::scan_in_background(&state.index, state.store.clone(), Arc::new(app))
}

/// Delete a session: its file and its artifacts directory (`docs/12` §2.3).
///
/// App-owned, because the engine advertises no delete command at all (measured: `delete`
/// is not among the 45 `get_available_commands` returns; its own `/delete` is TUI-only), so
/// there is nothing to dispatch. The id is resolved through the catalogue rather than
/// accepted as a path — a command that took a path would be a general-purpose delete — and
/// a session whose thread is live is refused, since a sidecar that keeps appending to a
/// file it still holds recreates it on its next write (`flows::delete`).
///
/// Blocking worker for the same reason `open_path` uses one: the id is a store scan.
///
/// The index loses the session's rows here rather than at the next pass, and that is the whole
/// point of doing it in this command: `docs/12` §7.4's index must not return hits for a thread
/// that cannot be opened, and the user who just deleted a session is the person most likely to
/// be searching for it. A failure to do so is reported as the *index* still listing the session,
/// because the file itself is already gone.
#[tauri::command]
pub async fn delete_session(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let store = state.store.clone();
    let threads = Arc::clone(&state.threads);
    let index = Arc::clone(&state.index);

    scan(move || {
        flows::delete(store.as_ref(), &threads, &id)?;
        index.forget(&id).map_err(|error| {
            format!("the session was deleted, but the search index still lists it: {error}")
        })
    })
    .await
}

/// Open the OS native folder picker dialog and return the selected path, or None if cancelled.
///
/// On KDE Plasma, delegates directly to `kdialog` to show the user's default Dolphin folder
/// navigator with their native bookmarks and Plasma theme.
#[tauri::command]
pub async fn pick_directory(title: Option<String>) -> Result<Option<String>, String> {
    scan(move || {
        let title_str = title.unwrap_or_else(|| "Open Project Folder".to_string());

        #[cfg(target_os = "linux")]
        {
            let is_kde = std::env::var("XDG_CURRENT_DESKTOP")
                .map(|d| d.to_uppercase().contains("KDE"))
                .unwrap_or(false);

            if is_kde {
                if let Ok(output) = std::process::Command::new("kdialog")
                    .arg("--title")
                    .arg(&title_str)
                    .arg("--getexistingdirectory")
                    .output()
                {
                    if output.status.success() {
                        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        if !path.is_empty() {
                            return Ok(Some(path));
                        }
                    }
                    return Ok(None);
                }
            }

            if let Ok(output) = std::process::Command::new("zenity")
                .args(["--file-selection", "--directory"])
                .arg(format!("--title={title_str}"))
                .output()
            {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        return Ok(Some(path));
                    }
                }
                return Ok(None);
            }
        }

        #[cfg(target_os = "macos")]
        {
            let script = format!("POSIX path of (choose folder with prompt \"{}\")", title_str);
            if let Ok(output) = std::process::Command::new("osascript")
                .args(["-e", &script])
                .output()
            {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        return Ok(Some(path));
                    }
                }
                return Ok(None);
            }
        }

        #[cfg(target_os = "windows")]
        {
            let script = format!(
                "Add-Type -AssemblyName System.Windows.Forms; $f = New-Object System.Windows.Forms.FolderBrowserDialog; $f.Description = '{}'; if ($f.ShowDialog() -eq 'OK') {{ $f.SelectedPath }}",
                title_str
            );
            if let Ok(output) = std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &script])
                .output()
            {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        return Ok(Some(path));
                    }
                }
                return Ok(None);
            }
        }

        Ok(None)
    })
    .await
}

/// The catalogue rows the `sessions` command answers with.
///
/// Split out from the command so it can be exercised without a window: the body here is
/// the whole answer, and the command adds only the thread the blocking scan runs on.
///
/// The registry is read for one field: whether the app has *released* this thread's sidecar
/// for being idle. That is app-owned state rather than anything the file says, and it is the
/// only thing that separates a session nobody has opened from one whose process this app
/// stopped — which the sidebar has to draw differently (`docs/12` §16).
pub fn session_catalogue(store: Option<&Store>, threads: &Threads) -> Vec<SessionSummaryDto> {
    let Some(store) = store else {
        return Vec::new();
    };

    let pinned = store.pinned_ids();

    store
        .list()
        .into_iter()
        .map(|session| {
            let is_pinned = pinned.contains(&session.id);
            let parent_id = session.parent_id();
            let suspended = threads.is_suspended(&session.id);

            SessionSummaryDto {
                id: session.id,
                path: session.path.display().to_string(),
                bucket: session.bucket,
                cwd: session.cwd,
                title: session.title,
                title_source: session.title_source,
                parent_id,
                created_at: session.created,
                modified_at: session.modified_ms,
                message_count: session.message_count,
                size: session.size,
                first_message: session.first_message,
                status: session.lifecycle.as_str().to_string(),
                pinned: is_pinned,
                suspended,
            }
        })
        .collect()
}

/// The project groups the `projects` command answers with.
///
/// Grouped by the cwd the sessions record rather than by bucket name, which is the same
/// rule `docs/12` §2.1 uses for the sidebar: a bucket name is a lossy encoding of a path
/// and cannot be turned back into one. The order is the catalogue's — newest session
/// first — so the project most recently worked in leads, which is what a sidebar wants.
/// A session that recorded no cwd cannot be attributed to a project and is left out of
/// this list; it is still in `sessions`.
pub fn project_groups(store: Option<&Store>) -> Vec<ProjectSnapshot> {
    let Some(store) = store else {
        return Vec::new();
    };

    let hidden = store.hidden_projects();
    let mut order: Vec<String> = Vec::new();
    let mut counts: HashMap<String, usize> = HashMap::new();

    for session in store.list() {
        if session.cwd.is_empty() {
            continue;
        }

        match counts.get_mut(&session.cwd) {
            Some(count) => *count += 1,
            None => {
                counts.insert(session.cwd.clone(), 1);
                order.push(session.cwd);
            }
        }
    }

    order
        .into_iter()
        .map(|path| ProjectSnapshot {
            name: project_name(&path),
            session_count: counts.get(&path).copied().unwrap_or_default(),
            hidden: hidden.contains(&path),
            path,
        })
        .collect()
}

/// A project's display name: its last path segment, or the path itself when it has none
/// (a root directory, which has no name to take).
fn project_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// End every thread, and every terminal, from Tauri's exit hook.
///
/// A process that exits while holding an `OmpClient` would leave the agent
/// running — a ~200 MB orphan per closed window, and with several threads open it would
/// be one per thread. Tauri's managed state is not dropped on every exit path, so the
/// children are stopped explicitly.
///
/// The terminals are children of the same kind: a shell left behind is a process the user
/// cannot see and did not ask to keep, and it may be running something expensive.
pub fn terminate(app: &AppHandle) {
    use tauri::Manager;

    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    for live in state.threads.drain() {
        tauri::async_runtime::block_on(live.shutdown(SHUTDOWN_GRACE));
    }

    state.terminals.shutdown();
}

/// Where a terminal's events go: the window, through the handle every other event uses.
fn terminal_sink(app: &AppHandle) -> Arc<dyn TerminalSink> {
    Arc::new(app.clone())
}

/// Show an OS notification.
///
/// The host sends it and the **window** decides whether to: focus, and whether the thread is
/// the one on screen, are facts about the window, and the host has no business holding them
/// (`docs/12` §13 — the host reports facts, the surface decides whether to interrupt someone).
/// What this adds is a path that does not depend on a web `Notification` global existing:
/// measured against `@tauri-apps/plugin-notification` 2.4.0, the JavaScript API is
/// `new window.Notification(title, options)` and never invokes the plugin's own command, so a
/// banner sent from the frontend rests on a global none of the three webviews promise. The
/// plugin's Rust side is `notify-rust` over D-Bus on Linux, with its own implementation per
/// platform and no permission dance in front of it.
///
/// The plugin's own error is returned rather than swallowed: the window may drop it, but the
/// host must not report a banner that never appeared as success.
#[tauri::command]
pub async fn notify_os(app: AppHandle, title: String, body: String) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;

    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|error| format!("the notification was not shown: {error}"))
}

/// The app's terminals, in the order their tabs were opened (`docs/12` §11).
#[tauri::command]
pub fn terminals(state: State<'_, AppState>) -> Vec<TerminalSnapshot> {
    state.terminals.list()
}

/// Open a terminal in `cwd`, running the user's own shell.
///
/// The directory comes from the caller because only the window knows which thread the user
/// is looking at; the *shell* does not, which is what keeps a terminal app-scoped.
///
/// Synchronous, and it is the one place here where that is a decision: allocating a pty and
/// forking a shell is a millisecond, and the window has nothing to draw until it answers.
#[tauri::command]
pub fn terminal_open(
    app: AppHandle,
    state: State<'_, AppState>,
    cwd: String,
    cols: u16,
    rows: u16,
) -> Result<TerminalSnapshot, String> {
    state
        .terminals
        .open(terminal_sink(&app), Path::new(&cwd), cols, rows)
}

/// Send what the user typed, or pasted.
#[tauri::command]
pub fn terminal_write(state: State<'_, AppState>, id: String, data: String) -> Result<(), String> {
    state.terminals.write(&id, &data)
}

/// Tell a terminal's shell its window changed size.
#[tauri::command]
pub fn terminal_resize(
    state: State<'_, AppState>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state.terminals.resize(&id, cols, rows)
}

/// Close a tab, ending its shell and everything it started.
///
/// The only terminal command that is not on the main thread: ending a shell waits for it (a
/// graceful signal, a grace period, then a kill), and a window that froze for that would
/// freeze on the one action a user takes *because* something is stuck.
#[tauri::command]
pub async fn terminal_close(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<TerminalSnapshot>, String> {
    let terminals = Arc::clone(&state.terminals);
    let sink = terminal_sink(&app);

    tauri::async_runtime::spawn_blocking(move || terminals.close(sink.as_ref(), &id))
        .await
        .map_err(|error| format!("the terminal was not closed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::ControlSnapshot;

    /// The window's own shape, asserted where it can break.
    ///
    /// `ImageIn` is the frontend→host contract and `ImageContent` is the host→engine
    /// one; a field renamed on one side only would send an attachment the engine
    /// ignores **silently**, because an unknown JSON field is not an error.
    #[test]
    fn an_image_from_the_window_becomes_the_engines_wire_object() {
        let from_window = serde_json::json!({
            "data": "iVBORw0KGgo=",
            "mimeType": "image/png",
        });
        let inbound: ImageIn = serde_json::from_value(from_window).expect("the window's shape");

        assert_eq!(
            commands::prompt("look", &wire_images(Some(vec![inbound])), None),
            serde_json::json!({
                "type": "prompt",
                "message": "look",
                "images": [{ "type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png" }],
            })
        );
    }

    /// Nothing attached is the common case, and it must not put an empty array on
    /// every message the app sends.
    #[test]
    fn a_message_without_attachments_carries_no_images_field() {
        assert!(wire_images(None).is_empty());
        assert!(wire_images(Some(Vec::new())).is_empty());
        assert_eq!(
            commands::steer("hi", &wire_images(None)),
            serde_json::json!({ "type": "steer", "message": "hi" })
        );
    }

    /// The search host's two payloads, as the overlay reads them.
    ///
    /// `completedAt` is the name the footer renders and `thread`/`kind`/`ordinal` are what a hit
    /// is picked apart by (`frontend/src/lib/search.ts`), so a field renamed here would arrive as
    /// `undefined` there — silently, because an object with an extra field is exactly what the
    /// frontend expects. Same class of break as the image shape above.
    #[test]
    fn the_search_payloads_are_the_shape_the_overlay_reads() {
        let status = IndexStatus {
            indexed: 12,
            total: 34,
            running: true,
            completed_at: Some(1_700_000_000_000),
            // Null here, because a running pass has nothing to report yet — but the key has to
            // be there, or the footer reads a missing field as `undefined` and says nothing.
            error: None,
        };
        assert_eq!(
            serde_json::to_value(&status).expect("the status serializes"),
            serde_json::json!({
                "indexed": 12,
                "total": 34,
                "running": true,
                "completedAt": 1_700_000_000_000_u64,
                "error": null,
            })
        );

        let hit = SearchHit {
            thread: "01alpha".to_string(),
            kind: "answer".to_string(),
            ordinal: 3,
            text: "the tokenizer re-reads the buffer".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&hit).expect("a hit serializes"),
            serde_json::json!({
                "thread": "01alpha",
                "kind": "answer",
                "ordinal": 3,
                "text": "the tokenizer re-reads the buffer",
            })
        );
    }

    /// The right panel's three payloads, as the panel reads them (`docs/12` §8).
    ///
    /// `frontend/src/bridge.ts` names every key asserted here — `todoPhases`, `tasks`,
    /// `blocker`, `isDir`, `modifiedAt`, `bytes`, `truncated` — and the panel draws them
    /// without a runtime check: a field renamed on this side arrives as `undefined` there,
    /// silently, because an object with the extra keys the frontend ignores is exactly what
    /// it expects. Same class of break as the shapes above. `todoPhases` replaced a
    /// `todoPhaseCount`, which the panel could not draw a task from.
    #[test]
    fn the_panels_payloads_are_the_shape_it_reads() {
        let control = ControlSnapshot {
            session_id: "01alpha".to_string(),
            session_name: None,
            model: None,
            thinking_level: None,
            is_streaming: false,
            is_compacting: false,
            message_count: 4,
            queued_message_count: 0,
            context: None,
            todo_phases: panel::phases(&omp_session::decode_phases(&serde_json::json!([
                {
                    "name": "Implementation",
                    "tasks": [
                        { "content": "write it", "status": "in_progress" },
                        { "content": "ship it", "status": "blocked", "blocker": "the review" },
                    ],
                }
            ]))),
            transcript_rows: 4,
            session_file: Some("/tmp/session.jsonl".to_string()),
            auto_compaction_enabled: Some(true),
        };

        let wire = serde_json::to_value(&control).expect("the control snapshot serializes");

        assert_eq!(
            wire["todoPhases"],
            serde_json::json!([
                {
                    "name": "Implementation",
                    "tasks": [
                        { "content": "write it", "status": "in_progress", "blocker": null },
                        { "content": "ship it", "status": "blocked", "blocker": "the review" },
                    ],
                }
            ]),
            // `null` rather than absent for a task with no blocker: Tauri's JSON is typed
            // by the frontend's interface, which declares `blocker: string | null`.
        );
        assert!(
            wire.get("todoPhaseCount").is_none(),
            "the count is gone, not kept beside the list"
        );

        // The tree's rows: `isDir` and `modifiedAt` are what the panel branches and dates
        // on, and `path` is what it passes straight back for the next level.
        assert_eq!(
            serde_json::to_value(&WorkspaceEntry {
                name: "src".to_string(),
                path: "/work/src".to_string(),
                is_dir: true,
                size: 0,
                modified_at: 1_700_000_000_000,
            })
            .expect("an entry serializes"),
            serde_json::json!({
                "name": "src",
                "path": "/work/src",
                "isDir": true,
                "size": 0,
                "modifiedAt": 1_700_000_000_000_u64,
            })
        );

        // And an artifact read: `bytes` is the whole result's size, `text` what the host
        // could hand over, and `truncated` the flag that says the two differ.
        assert_eq!(
            serde_json::to_value(&ArtifactSnapshot {
                id: "7".to_string(),
                path: "/store/sessions/b/7.bash.log".to_string(),
                bytes: 1_288_895,
                text: "1\n2\n".to_string(),
                truncated: true,
            })
            .expect("an artifact read serializes"),
            serde_json::json!({
                "id": "7",
                "path": "/store/sessions/b/7.bash.log",
                "bytes": 1_288_895_u64,
                "text": "1\n2\n",
                "truncated": true,
            })
        );
    }

    #[test]
    fn threads_returns_empty_when_no_sessions_are_live() {
        let registry = Arc::new(Threads::default());
        assert!(registry.snapshots().is_empty());
    }
}
