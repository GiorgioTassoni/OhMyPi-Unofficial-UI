//! The Oh My Pi desktop shell.
//!
//! A Tauri host for the `omp` coding agent: it owns the window and the agent
//! process, and nothing else — the agent itself stays upstream (`docs/11` §2.2).
//!
//! Layering, outermost first:
//!
//! * [`bridge`] — Tauri commands, thin adapters with no business rules.
//! * [`flows`] — the session flows behind a thread's context menu: the eight commands
//!   that decide something rather than pass it through (`docs/12` §2.3).
//! * [`external`] — handing a URL to the user's browser, under an allowlist.
//! * [`policy`] — the engine's own config record, for "always allow <tool>".
//! * [`panel`] — the right panel's data: the plan, the workspace tree, and the
//!   artifacts the engine spilled (`docs/12` §8).
//! * [`models`] — the model catalogue: the engine's rows, cached and refreshed out of
//!   band, plus the chip row's four session commands.
//! * [`favourites`] — the starred models, which the engine has no concept of
//!   (`docs/12` §14.3).
//! * [`search`] — the app's own cross-thread search index, in SQLite FTS5, because the
//!   engine has none (`docs/12` §7.4, decision D7).
//! * [`pty`] — the app's own terminals: a real shell in a real pty, which the engine
//!   has no part in (`docs/12` §11, decision D6).
//! * [`settings`] — the settings screen's host side: the composed screen, the two write
//!   commands, and the raw-config escape hatch (`docs/12` §12, `docs/13`).
//! * [`session`] — one live agent: its reducers and the pump that feeds them.
//! * [`threads`] — the registry of live agents, keyed by the engine's session id.
//! * [`dialogs`] — the pending-dialog store, which announces every change to it.
//! * [`dto`] — the frontend's data contract, hand-written so the UI does not move
//!   when the wire does.
//! * `omp_session`, `omp_transport`, `omp_store` — the crates under test, unchanged
//!   here.
//!
//! This is a library so the session actor can be exercised without a window
//! (`tests/m0.rs`); `main.rs` only starts the app.

pub mod agents;
pub mod bridge;
pub mod dialogs;
pub mod dto;
pub mod external;
pub mod favourites;
pub mod flows;
pub mod idle;
pub mod models;
pub mod notify;
pub mod panel;
pub mod policy;
pub mod pty;
pub mod search;
pub mod session;
pub mod settings;
pub mod threads;

use std::sync::Arc;

use tauri::Manager;

use bridge::{AppState, LaunchState};

/// Read the workspace the app was launched with, if any.
///
/// Only the first positional argument is considered, and only when it is an
/// existing directory: a typo should open an idle window rather than an agent in
/// the wrong project.
pub fn launch_context() -> dto::LaunchContext {
    let argument = std::env::args().nth(1);
    let workspace = argument
        .clone()
        .filter(|candidate| std::path::Path::new(candidate).is_dir());

    if workspace.is_none() && argument.is_some() {
        eprintln!("[omp-desktop] ignoring the launch argument: not a directory");
    }

    dto::LaunchContext {
        workspace,
        user: local_user(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// The OS user, for the sidebar's identity row.
///
/// `$USER` first because it is what a shell would say, `$LOGNAME` as the fallback the platform
/// sets for login sessions, and `None` rather than a guessed name when neither exists: an
/// identity row that invents "user" would be worse than one that shows nothing.
fn local_user() -> Option<String> {
    ["USER", "LOGNAME", "USERNAME"]
        .iter()
        .find_map(|name| std::env::var(name).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Start the app. Returns when the last window closes.
pub fn run() {
    let context = launch_context();

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(move |app| {
            // Managed here rather than with `Builder::manage` because the two app-owned
            // stores are constructed with the config directory the app will write to,
            // and that only exists once there is an app.
            let state = AppState::new(bridge::config_dir(app.handle()));

            // The first index pass starts with the window and runs on a worker of its own:
            // a cold pass reads every session file (`crate::search`), so it cannot be
            // anything the window waits for — `docs/12` §7.4 asked for exactly that.
            let index = Arc::clone(&state.index);
            let store = state.store.clone();
            // Two app-level tasks outlive the command that started them, and both publish
            // through the window's own sink: the idle supervisor (which releases sidecars a
            // window will not notice until it resumes one) and the broker watcher (which
            // reports a background job finishing while nobody is asking).
            let sink: Arc<dyn session::ActivitySink> = Arc::new(app.handle().clone());
            idle::spawn(
                Arc::clone(&state.threads),
                Arc::clone(&sink),
                idle::Config::from_env(),
            );
            notify::spawn_job_watcher(Arc::clone(&state.threads), sink);
            app.manage(state);
            app.manage(LaunchState { context });
            if let Err(error) =
                search::scan_in_background(&index, store, Arc::new(app.handle().clone()))
            {
                eprintln!("[omp-desktop] the search index was not started: {error}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bridge::open_thread,
            bridge::close_thread,
            bridge::threads,
            bridge::thread_status,
            bridge::transcript,
            bridge::prompt,
            bridge::steer,
            bridge::follow_up,
            bridge::stop_turn,
            bridge::stop_turn_and_send,
            bridge::ui_requests,
            bridge::respond_ui_request,
            bridge::sessions,
            bridge::projects,
            bridge::set_focused_thread,
            bridge::notify_os,
            bridge::search,
            bridge::index_status,
            bridge::reindex,
            bridge::allow_tool,
            bridge::open_external,
            bridge::launch_context,
            bridge::models,
            bridge::thread_todos,
            bridge::set_todos,
            bridge::workspace_tree,
            bridge::read_artifact,
            bridge::available_commands,
            bridge::refresh_models,
            bridge::set_model,
            bridge::set_thinking_level,
            bridge::set_auto_compaction,
            bridge::compact,
            bridge::favourites,
            bridge::set_favourites,
            bridge::set_approval_mode,
            bridge::rename_thread,
            bridge::branch_targets,
            bridge::branch_thread,
            bridge::handoff_thread,
            bridge::export_html,
            bridge::open_path,
            bridge::pick_directory,
            bridge::delete_session,
            bridge::pin_session,
            bridge::agents,
            bridge::parked_agents,
            bridge::agent_messages,
            bridge::broker_processes,
            bridge::stop_broker_process,
            bridge::terminals,
            bridge::terminal_open,
            bridge::terminal_write,
            bridge::terminal_resize,
            bridge::terminal_close,
            settings::settings_screen,
            settings::settings_write,
            settings::settings_reset,
            settings::settings_hatch,
            settings::settings_hatch_plan,
            settings::settings_hatch_apply,
            settings::settings_backup,
            settings::settings_restart_sessions,
        ])
        .build(tauri::generate_context!())
        .expect("the Tauri application builds")
        .run(|app, event| {
            // Closing the last window exits the app; make sure the agent goes with
            // it rather than being orphaned.
            if let tauri::RunEvent::Exit = event {
                bridge::terminate(app);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The footer's two facts.
    ///
    /// The version is asserted against the crate's own, because the way this breaks is somebody
    /// typing a version into a string. The user is asserted as a *shape* rather than a value: it
    /// comes from the environment, and a test that read the same variable to check it would only
    /// be proving that `std::env::var` works.
    #[test]
    fn the_launch_context_reports_this_build() {
        assert_eq!(launch_context().version, env!("CARGO_PKG_VERSION"));

        if let Some(user) = local_user() {
            assert_eq!(user.trim(), user, "a user name with padding is not one");
            assert!(!user.is_empty(), "an empty name is not an identity");
        }
    }
}
