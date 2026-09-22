//! Settings: the screen's host side, its writes, and the raw-config escape hatch.
//!
//! `crates/omp-settings` owns the data — the generated catalog, the write flow, the screen
//! model — and knows nothing about this app: no window, no sidecar, no paths of its own. This
//! module is the seam, and three things meet in it that meet nowhere else:
//!
//! * **The files.** The engine answers with *effective* values — every layer merged — and the
//!   screen draws where each one comes from (`docs/12` §12), a question only the files answer.
//!   So the host reads the agent directory, the global file, the thread's project file and the
//!   read-only `PI_CONFIG_FILES` overlays, and hands the crate that set.
//! * **The live sessions.** A write from outside the process is not seen by a session that is
//!   already running (`crates/omp-settings/src/lib.rs`, rule 2), so the six keys the catalog
//!   calls `live` are pushed through the same RPC setters the chip row uses, and
//!   [`settings_restart_sessions`] is what every other row points at instead.
//! * **The window's own paths.** The project file is named by whichever thread the screen is
//!   looking at, and the overlays are the host process's environment rather than the engine's.
//!
//! The commands are thin where the crate does the work, and deliberate where it does not: the
//! restart of a live session is `bridge::set_approval_mode`'s mechanism, reused rather than
//! invented, because two ways to bring a session back is two ways for one of them to lose its
//! conversation.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use omp_settings::apply;
use omp_settings::catalog::{Catalog, Restart};
use omp_settings::engine::Cli;
use omp_settings::hatch;
use omp_settings::screen::{self, FileState, Files};
use omp_transport::protocol::commands;
use omp_transport::{ClientOptions, SidecarSpec};
use serde_json::Value;
use tauri::{AppHandle, State};

use crate::bridge::AppState;
use crate::dto::{
    SettingsApplyReport, SettingsHatch, SettingsHatchPlan, SettingsRestartReport,
    SettingsRestartSkip, SettingsScreen, SettingsWriteOutcome,
};
use crate::models;
use crate::session::{self, LiveSession, Roster};

/// How long a sidecar gets to exit cleanly before it is killed.
///
/// The same five seconds `bridge::set_approval_mode` gives the thread it restarts: this is a
/// restart of the same kind, and one number for "stop a session" is one thing to change.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// The settings screen, as the host composes it (`docs/12` §12).
///
/// One round trip, because the host sends everything the screen draws: the catalog is not in
/// the frontend, so a row's label, group, order, options and gate all arrive with it and cannot
/// drift from the engine's.
///
/// `thread` is the session whose workspace the app treats as the one in view; without one the
/// screen reports no project file at all, which is the honest answer for a window with nothing
/// open.
#[tauri::command]
pub async fn settings_screen(
    state: State<'_, AppState>,
    thread: Option<String>,
) -> Result<SettingsScreen, String> {
    let catalog = Catalog::embedded()?;
    let cli = Cli::new();

    let agent_dir = cli.agent_dir().await?;
    let listing = cli.list().await?;
    let files = Files {
        global: file_state(&global_config_file(&agent_dir), catalog),
        project: project_file(&state, thread.as_deref(), catalog).await,
        overlays: overlays(catalog),
        agent_dir,
    };

    Ok(screen::build(
        catalog,
        &listing,
        &files,
        env!("CARGO_PKG_VERSION"),
    ))
}

/// Record a value for one settings key, then make a `live` key true of the sessions that are
/// already running.
#[tauri::command]
pub async fn settings_write(
    state: State<'_, AppState>,
    key: String,
    value: Value,
    confirmation: Option<String>,
) -> Result<SettingsWriteOutcome, String> {
    let catalog = Catalog::embedded()?;
    let cli = Cli::new();

    let mut outcome = apply::set(&cli, catalog, &key, value, confirmation.as_deref()).await?;
    effect_now(&state, &cli, &mut outcome).await;

    Ok(outcome)
}

/// Return one key to the engine's default, with the same follow-through.
#[tauri::command]
pub async fn settings_reset(
    state: State<'_, AppState>,
    key: String,
    confirmation: Option<String>,
) -> Result<SettingsWriteOutcome, String> {
    let catalog = Catalog::embedded()?;
    let cli = Cli::new();

    let mut outcome = apply::reset(&cli, catalog, &key, confirmation.as_deref()).await?;
    effect_now(&state, &cli, &mut outcome).await;

    Ok(outcome)
}

/// Read a config file the way the escape hatch draws it.
///
/// `scope` picks which file: `global` is the file under the agent directory, `project` is
/// `<thread's workspace>/.omp/config.yml`. Overlays are shown in the sources banner and are
/// never edited, so they are not a scope.
#[tauri::command]
pub async fn settings_hatch(
    state: State<'_, AppState>,
    thread: Option<String>,
    scope: Option<String>,
) -> Result<SettingsHatch, String> {
    let catalog = Catalog::embedded()?;
    let cli = Cli::new();
    let path = hatch_path(&state, &cli, thread.as_deref(), scope.as_deref()).await?;

    hatch::open(&path, catalog)
}

/// What an edited copy of the file would change — validation and diff, no writing.
///
/// The baseline is the file as the hatch shows it *now*: the pane the user is editing was drawn
/// from that text, so a diff against anything else would report changes nobody made. It is the
/// redacted text, which is why an untouched credential line does not read as a change.
#[tauri::command]
pub async fn settings_hatch_plan(text: String) -> Result<SettingsHatchPlan, String> {
    let catalog = Catalog::embedded()?;
    let cli = Cli::new();
    let path = global_config_file(&cli.agent_dir().await?);

    plan_against(&path, &text, catalog)
}

/// Apply an edited copy of the file, after copying the file aside.
///
/// The write itself is the crate's (`omp config set|reset` per change); what the host adds is
/// the copy that makes it reversible, and the rule that it is taken once per distinct content
/// rather than once per click — the pane is where a user edits many keys in a row, and a
/// revert list that is three identical copies deep is one nobody can use.
///
/// The file is read again here and handed to the crate as it is *now*: the engine writes this
/// file too — an "always allow" recording `tools.approval`, a model-role write — and an edit
/// previewed before one of those must not put the file back the way the user last saw it
/// (`docs/13` §"Protections that apply to every write", 3).
#[tauri::command]
pub async fn settings_hatch_apply(
    text: String,
    confirmation: Option<String>,
) -> Result<SettingsApplyReport, String> {
    let catalog = Catalog::embedded()?;
    let cli = Cli::new();
    let path = global_config_file(&cli.agent_dir().await?);

    let plan = plan_against(&path, &text, catalog)?;
    if !plan.changes.is_empty() {
        copy_aside_once(&path)?;
    }
    let current = read_current(&path, catalog)?;

    apply::apply_plan(&cli, catalog, &plan, &current, confirmation.as_deref()).await
}

/// Copy the engine's global config aside, and answer with the copy's path.
#[tauri::command]
pub async fn settings_backup() -> Result<String, String> {
    let cli = Cli::new();
    let path = global_config_file(&cli.agent_dir().await?);

    hatch::backup(&path).map(|copy| copy.display().to_string())
}

/// Restart every live session, so a written setting is in effect now.
///
/// The action behind every `sidecar` row's "restart to use it now" (`docs/12` §12). A session
/// mid-turn is left alone and named in the report: killing a run the user is watching to apply
/// a setting they may not have finished reading is not a trade this app makes (`dto` §settings).
#[tauri::command]
pub async fn settings_restart_sessions(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SettingsRestartReport, String> {
    let mut report = SettingsRestartReport {
        restarted: Vec::new(),
        skipped: Vec::new(),
    };
    // Whether the roster moved at all: a session that was stopped and did not come back has
    // left the registry, and the sidebar has to hear about it.
    let mut touched = false;

    // Sequential on purpose: each restart is a fresh sidecar with its own handshake, and a
    // dozen of them racing for the same engine locks is not faster in any way a person waits on.
    for live in state.threads.live() {
        let thread = live.thread.current();

        if let Some(reason) = mid_turn(&live) {
            report.skipped.push(SettingsRestartSkip { thread, reason });
            continue;
        }

        match resume_now(&app, &state, &live).await {
            Ok(next) => {
                touched = true;
                report.restarted.push(next);
            }
            Err(reason) => {
                touched = true;
                report.skipped.push(SettingsRestartSkip { thread, reason });
            }
        }
    }

    if touched {
        state.threads.publish(&app);
    }

    Ok(report)
}

/// Make a `live` write true of the sessions that are already running.
///
/// `docs/13` §"The measurement that fixes `restart`" is the reason this exists: the engine
/// reads its settings when a session is constructed, so only the six keys with an RPC setter
/// can honestly be called `live` — and only if the host pushes them after the file write. The
/// sentence the row shows (`recorded_message`) is the promise this has to keep; a session that
/// did not take the value is named in it rather than left to look applied.
async fn effect_now(state: &AppState, cli: &Cli, outcome: &mut SettingsWriteOutcome) {
    if outcome.restart != Restart::Live || state.threads.live().is_empty() {
        return;
    }

    let Some(value) = effective_value(cli, outcome).await else {
        // A reset has no value of its own, and the engine can report no value for a key whose
        // default is "nothing" — there is then nothing to push, and saying so is better than
        // letting the row's promise stand unearned.
        outcome.message.push_str(
            " The engine reports no value for it now, so a running session keeps the one it has.",
        );
        return;
    };

    let failures = push_to_live(state, &outcome.key, &value).await;
    if !failures.is_empty() {
        outcome.message.push_str(&format!(
            " Not every live session took it: {}.",
            failures.join("; ")
        ));
    }
}

/// The value a `live` write should push: what was written, or, after a reset, what the engine
/// reports now.
///
/// The default is deliberately not guessed at here: a reset means "the engine's own choice",
/// and the six keys' choices are the engine's to make.
async fn effective_value(cli: &Cli, outcome: &SettingsWriteOutcome) -> Option<Value> {
    if let Some(value) = &outcome.value {
        return Some(value.clone());
    }

    cli.list().await.ok()?.value_of(&outcome.key)
}

/// Push one key's new value into every live session, and name the ones that refused.
///
/// Per session rather than all-or-nothing: the file did change, so a session that died on its
/// own must not turn a recorded write into an error — but it must not be counted as applied
/// either, which is what returning the failures rather than counting them leaves possible.
async fn push_to_live(state: &AppState, key: &str, value: &Value) -> Vec<String> {
    let mut failures = Vec::new();

    for live in state.threads.live() {
        if let Err(error) = push_one(&live, key, value).await {
            failures.push(format!("{}: {error}", live.thread.current()));
        }
    }

    failures
}

/// One key's value, through the setter that key already has.
///
/// The thinking level and auto-compaction go through [`crate::models`] — the two functions
/// `bridge::set_thinking_level` and `bridge::set_auto_compaction` call — and the other four
/// through the same command builders those two use, so a value the settings screen writes and a
/// value the chip row writes reach the engine identically.
async fn push_one(live: &LiveSession, key: &str, value: &Value) -> Result<(), String> {
    match key {
        "defaultThinkingLevel" => models::set_thinking_level(live, &text(value)?).await,
        "compaction.enabled" => models::set_auto_compaction(live, flag(value)?).await,
        "steeringMode" => {
            push(
                live,
                commands::set_steering_mode(&text(value)?),
                "steering mode",
            )
            .await
        }
        "followUpMode" => {
            push(
                live,
                commands::set_follow_up_mode(&text(value)?),
                "queued-message mode",
            )
            .await
        }
        "interruptMode" => {
            push(
                live,
                commands::set_interrupt_mode(&text(value)?),
                "interrupt mode",
            )
            .await
        }
        "retry.enabled" => {
            push(
                live,
                commands::set_auto_retry(flag(value)?),
                "auto-retry change",
            )
            .await
        }
        // Unreachable through this module: `effect_now` only runs for a `live` write, and the
        // catalog holds that class to exactly the six keys above
        // (`crates/omp-settings/src/apply.rs`). A poisoned registry entry or a catalog from a
        // newer engine is still answered rather than panicked on.
        other => Err(format!("this build has no live setter for `{other}`")),
    }
}

/// Send one command into a live session, then re-read the state the chips render.
///
/// The two steps [`crate::models`] takes for the same reason: the command is what changes the
/// engine, and the re-read is what stops a control the engine just moved from being shown
/// stale. A missed re-read is logged and dropped — the value *did* reach the session, and the
/// pump's own frames will correct the snapshot.
async fn push(live: &LiveSession, command: Value, what: &str) -> Result<(), String> {
    let client = live
        .client()
        .ok_or_else(|| "the session is shutting down".to_string())?;

    session::send(&client, command, what).await?;

    if let Err(error) = live.reread_control().await {
        eprintln!("[omp-desktop] could not refresh session state: {error}");
    }

    Ok(())
}

/// A name from a value the catalog already validated the shape of.
///
/// `apply` checks a written value against the key's type, and a reset's value comes from the
/// engine's own listing, so a mismatch here is an engine that changed the key under this build.
/// Reported like any other per-session failure rather than turned into a panic in a command
/// handler.
fn text(value: &Value) -> Result<String, String> {
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("`{value}` is not one of this key's values"))
}

/// A switch from a value, on the same terms as [`text`].
fn flag(value: &Value) -> Result<bool, String> {
    value
        .as_bool()
        .ok_or_else(|| format!("`{value}` is not on or off"))
}

/// Why a session may not be restarted right now, when it may not.
///
/// `is_streaming` and `is_compacting` are the reducer's own flags, kept current by the pump
/// (`crates/omp-session/src/control.rs`): the first is the one the sidebar's dot draws
/// (`docs/12` §2.2), and the second is the same situation for the compaction the engine is in
/// the middle of. A state that cannot be read is treated as a reason not to stop the session —
/// the fallback has to be "leave it running".
fn mid_turn(live: &LiveSession) -> Option<String> {
    let Ok(control) = live.control.lock() else {
        return Some("its state could not be read, so it is not safe to stop".to_string());
    };

    if control.is_streaming {
        return Some("a turn is in flight".to_string());
    }
    if control.is_compacting {
        return Some("a compaction is running".to_string());
    }

    None
}

/// Stop one live session and bring it straight back, on the same session file.
///
/// `bridge::set_approval_mode`'s restart-and-resume, reused rather than rewritten: the registry
/// loses the thread before its sidecar is stopped, so a command arriving mid-restart is refused
/// instead of written to a pipe that is going away; the session comes back through
/// `session::open`, which is also what restores the conversation (`docs/12` §6.2); and the
/// launch mode it had is carried over, because a session switched to another approval ladder
/// must not silently return to the one in the config.
///
/// Answers with the id the session came back under — a resume keeps it, and if an engine ever
/// handed back a different one the report would be right and the registry would say so.
async fn resume_now(
    app: &AppHandle,
    state: &AppState,
    live: &Arc<LiveSession>,
) -> Result<String, String> {
    let session_file = live.session_file().ok_or_else(|| {
        "the engine did not name a session file, so this session cannot be resumed".to_string()
    })?;

    let mut spec = SidecarSpec::omp(&live.workspace).resuming(&session_file);
    if let Some(mode) = &live.approval_mode {
        spec = spec.with_approval_mode(mode);
    }
    let tried = spec.program.display().to_string();

    if let Some(stopped) = state.threads.remove(&live.thread.current()) {
        stopped.shutdown(SHUTDOWN_GRACE).await;
    }

    let roster: Arc<dyn Roster> = state.threads.clone();
    let next = session::open(&spec, ClientOptions::default(), app.clone(), roster)
        .await
        .map_err(|error| {
            format!("the session was stopped to apply the setting and could not be resumed: {error}\n  tried: {tried}")
        })?;

    let thread = next.thread.current();
    state.threads.insert(next);

    Ok(thread)
}

/// Diff edited text against the file as it is right now.
///
/// One baseline for both the preview and the apply — [`read_current`] is the single read of
/// "the file", so the plan the user approved is a plan against the text the pane drew.
fn plan_against(path: &Path, edited: &str, catalog: &Catalog) -> Result<SettingsHatchPlan, String> {
    let baseline = read_current(path, catalog)?;

    Ok(hatch::plan(&baseline, edited, catalog))
}

/// The file's keys, read the way the hatch draws it: redacted first, then flattened.
///
/// Redacted because the baseline a hand-edit is diffed against has to be the text the pane
/// showed — a key holding a credential reads as the mask on both sides, so an untouched
/// credential line is not a change. Flattened from that text rather than from the file so the
/// two agree about what a key holds.
///
/// A file that cannot be parsed has no keys to plan or apply against: the hatch reports the
/// parser's own sentence instead of letting an empty baseline read as "nothing is set", which
/// would have the crate refuse every change with a reason that blamed the wrong thing.
fn read_current(path: &Path, catalog: &Catalog) -> Result<hatch::Flattened, String> {
    let current = hatch::open(path, catalog)?;
    if let Some(error) = current.error {
        return Err(error);
    }

    hatch::read(&current.text, catalog).map_err(|error| error.sentence())
}

/// Copy the file aside, unless the newest copy already holds exactly what is on disk.
///
/// The hatch's promise is that one hand-edit can be undone (`docs/13` §"Protections that apply
/// to every write", 2), and the *first* change of a session is the one that has to be
/// reversible. Taking another copy of content the newest backup already holds would only bury
/// that one. A file that does not exist yet has nothing to lose, so it is not a failure.
fn copy_aside_once(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }

    let current = std::fs::read(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    if let Some(newest) = hatch::backups(path).first() {
        if std::fs::read(newest).is_ok_and(|copy| copy == current) {
            return Ok(());
        }
    }

    hatch::backup(path).map(|_| ())
}

/// Which file the escape hatch edits.
///
/// An unknown scope is refused by name, and `project` needs both halves of the path it is made
/// of: a thread to name a workspace, and a workspace for that thread. Both sentences say what
/// was missing, because "the hatch did not open" is not something a user can act on.
async fn hatch_path(
    state: &AppState,
    cli: &Cli,
    thread: Option<&str>,
    scope: Option<&str>,
) -> Result<PathBuf, String> {
    match scope.unwrap_or("global") {
        "global" => Ok(global_config_file(&cli.agent_dir().await?)),
        "project" => {
            let thread = thread.ok_or_else(|| {
                "the project config file is <workspace>/.omp/config.yml, and no session is open to name a workspace"
                    .to_string()
            })?;
            let workspace = workspace_of(state, thread).await.ok_or_else(|| {
                format!("no workspace is known for session `{thread}`, so its `.omp/config.yml` cannot be found")
            })?;

            Ok(project_config_file(&workspace))
        }
        other => Err(format!(
            "`{other}` is not a config scope — the escape hatch edits `global` or `project`"
        )),
    }
}

/// The global config file under the agent directory.
///
/// `config.yml` is the name the engine writes, and `config.yaml` is one it reads: a user who
/// has only the latter is being read from it, so that is the file the screen describes and the
/// hatch edits. Neither existing is the fresh install, where the canonical name is the file the
/// first write will create.
fn global_config_file(agent_dir: &str) -> PathBuf {
    let canonical = Path::new(agent_dir).join("config.yml");
    if canonical.exists() {
        return canonical;
    }

    let compatibility = Path::new(agent_dir).join("config.yaml");
    if compatibility.exists() {
        return compatibility;
    }

    canonical
}

/// `<workspace>/.omp/config.yml`, the project layer's one file.
fn project_config_file(workspace: &str) -> PathBuf {
    Path::new(workspace).join(".omp").join("config.yml")
}

/// The thread's project file, when the thread's workspace is known.
///
/// The workspace comes from the live sidecar, and for a thread whose sidecar has gone from the
/// session's own header through the engine's catalogue — the same lookup a resume makes, for
/// the same reason (`bridge::session_path`). A window looking at a closed thread is still
/// looking at a project, and reporting "no project file" for it would be a claim about the
/// disk rather than about the lookup.
async fn project_file(state: &AppState, thread: Option<&str>, catalog: &Catalog) -> FileState {
    let Some(thread) = thread else {
        return FileState::missing(String::new());
    };
    let Some(workspace) = workspace_of(state, thread).await else {
        return FileState::missing(String::new());
    };

    file_state(&project_config_file(&workspace), catalog)
}

/// The directory a session is working in, when the app can still answer.
async fn workspace_of(state: &AppState, thread: &str) -> Option<String> {
    if let Some(live) = state.threads.get(thread) {
        return Some(live.workspace.clone());
    }

    // On a worker of its own: the catalogue opens two windows of every session file, which is
    // why `bridge` reads it the same way.
    let store = state.store.clone()?;
    let id = thread.to_string();
    let summary = tauri::async_runtime::spawn_blocking(move || store.find(&id))
        .await
        .ok()
        .flatten()?;

    (!summary.cwd.is_empty()).then_some(summary.cwd)
}

/// One config file, as the screen needs it: its path, whether it parses, and what it sets.
///
/// A file that is not there is not an error — it is the state a fresh install is in — and a
/// file that cannot be parsed is reported with the parser's own line, because a partially
/// readable config is the case the banner exists for.
fn file_state(path: &Path, catalog: &Catalog) -> FileState {
    let display = path.display().to_string();

    if !path.exists() {
        return FileState::missing(display);
    }

    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            return FileState::unreadable(display, format!("could not read it: {error}"));
        }
    };

    match hatch::flatten(&text, catalog) {
        Ok(flattened) => FileState::read(display, flattened),
        Err(error) => FileState::unreadable(display, error.sentence()),
    }
}

/// The read-only overlays the process was pointed at (`PI_CONFIG_FILES`).
///
/// A platform path-list, read with the platform's own separator rule: the engine splits it the
/// same way, and a host that guessed at `:` would mis-split a Windows path. The app never edits
/// one — they are shown so a value that comes from an overlay is not reported as coming from a
/// file the user can open.
fn overlays(catalog: &Catalog) -> Vec<FileState> {
    let Some(list) = std::env::var_os("PI_CONFIG_FILES") else {
        return Vec::new();
    };

    std::env::split_paths(&list)
        .map(|path| file_state(&path, catalog))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of this test's own, removed and recreated per test.
    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "omp-desktop-settings-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a test directory");

        path
    }

    /// The revert list has to stay usable: one hand-edit, one copy to go back to.
    ///
    /// The hatch applies change by change, so a session of editing is a run of applies; a copy
    /// per click would leave the user scrolling through identical files to find the one that
    /// predates the mistake.
    #[test]
    fn a_backup_is_not_taken_twice_for_the_same_content() {
        let dir = scratch("backup");
        let config = dir.join("config.yml");
        std::fs::write(&config, "steeringMode: all\n").expect("a config");

        copy_aside_once(&config).expect("the first copy");
        copy_aside_once(&config).expect("a second copy of the same content is skipped, not failed");
        assert_eq!(
            hatch::backups(&config).len(),
            1,
            "an unchanged file is already backed up"
        );

        std::fs::write(&config, "steeringMode: one-at-a-time\n").expect("the edit");
        copy_aside_once(&config).expect("a copy of the new content");
        assert_eq!(
            hatch::backups(&config).len(),
            2,
            "content the newest copy does not hold is worth keeping"
        );
    }

    /// A file that is not there yet is a fresh install: nothing to lose, so nothing to copy —
    /// and not an error either, because the first write of a fresh install is the normal case.
    #[test]
    fn a_missing_file_is_not_a_failed_backup() {
        let dir = scratch("missing");
        let config = dir.join("config.yml");

        assert!(copy_aside_once(&config).is_ok());
        assert!(hatch::backups(&config).is_empty());
    }

    /// The engine reads `config.yaml` when that is the file in use (`docs/05` §1.2), so that is
    /// the file the screen must describe and the hatch edit — not the name a fresh install gets.
    #[test]
    fn the_compatibility_name_wins_when_it_is_the_file_in_use() {
        let dir = scratch("naming");
        let agent_dir = dir.display().to_string();
        let canonical = dir.join("config.yml");
        let compatibility = dir.join("config.yaml");

        assert_eq!(
            global_config_file(&agent_dir),
            canonical,
            "a fresh install has neither, and the canonical name is what a write creates"
        );

        std::fs::write(&compatibility, "steeringMode: all\n").expect("the only config");
        assert_eq!(
            global_config_file(&agent_dir),
            compatibility,
            "a file the engine reads is the file to edit"
        );

        std::fs::write(&canonical, "steeringMode: all\n").expect("the canonical config");
        assert_eq!(
            global_config_file(&agent_dir),
            canonical,
            "the canonical name wins once it exists"
        );
    }
}
