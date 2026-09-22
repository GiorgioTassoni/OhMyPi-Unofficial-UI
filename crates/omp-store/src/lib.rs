//! The engine's on-disk store, read the way the engine reads it.
//!
//! Everything in this crate is a **read**: sessions, pins and the project registry are the
//! engine's files, and the app's job is to show them without becoming a second writer that
//! can corrupt them. (The one place the app acts on them — deleting a session, pinning one —
//! goes through the engine's own commands; `docs/12` §2.3.)
//!
//! Why this exists at all: at v18.2.6 the RPC surface has no way to list sessions. The
//! engine's own listing is disk-only (`session-listing.ts`, SDK-only `SessionManager.listAll`),
//! so the app has to read the files — and it has to read them with the *same* rules, or the
//! sidebar and the engine's resume picker would disagree about the same session.
//!
//! ```no_run
//! let Some(store) = omp_store::Store::discover() else {
//!     return;
//! };
//! for session in store.list() {
//!     println!("{} {:?} {}", session.id, session.lifecycle, session.first_message);
//! }
//! ```
//!
//! Blocking, deliberately: a scan reads two windows of every session file, and the caller —
//! the Tauri host — owns the decision of which thread that happens on.

pub mod listing;
pub mod messages;
pub mod paths;
pub mod pins;
pub mod projects;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub use listing::{Lifecycle, SessionSummary};
pub use messages::{Record, RecordKind};

/// The engine's store, with the agent directory resolved once.
#[derive(Debug, Clone)]
pub struct Store {
    agent_dir: PathBuf,
}

impl Store {
    /// The store this process should read: `PI_CODING_AGENT_DIR`, else `~/.omp/agent`.
    ///
    /// `None` when neither is available. That is worth distinguishing from "no sessions":
    /// a browser that showed an empty project list because `$HOME` was missing would look
    /// like a working app with nothing in it.
    pub fn discover() -> Option<Self> {
        paths::agent_dir().map(Self::at)
    }

    /// The same store at an explicit location — a fixture in tests, a relocated agent dir
    /// in a user's config.
    pub fn at(agent_dir: impl Into<PathBuf>) -> Self {
        Self {
            agent_dir: agent_dir.into(),
        }
    }

    pub fn agent_dir(&self) -> &Path {
        &self.agent_dir
    }

    pub fn sessions_root(&self) -> PathBuf {
        paths::sessions_root(&self.agent_dir)
    }

    /// Every session in the store, newest first.
    ///
    /// The list is flat and carries each session's own cwd; grouping it into projects is
    /// presentation and belongs to whoever renders it.
    pub fn list(&self) -> Vec<SessionSummary> {
        listing::list_all(&self.sessions_root())
    }

    /// Every session in one project, newest first.
    ///
    /// Matches on the session's recorded cwd rather than on a bucket name, because a bucket
    /// name cannot be turned back into a path (see [`paths`]).
    pub fn list_in(&self, project: &Path) -> Vec<SessionSummary> {
        let wanted = project.to_string_lossy();
        let mut sessions: Vec<SessionSummary> = self
            .list()
            .into_iter()
            .filter(|session| session.cwd == wanted)
            .collect();
        sessions.shrink_to_fit();
        sessions
    }

    /// Find one session by id, across every bucket.
    ///
    /// Resuming needs this: the id is what the browser has, and the file's location (and
    /// therefore the working directory to respawn in) is what the engine needs.
    pub fn find(&self, id: &str) -> Option<SessionSummary> {
        self.list().into_iter().find(|session| session.id == id)
    }

    /// Every searchable record in one session, in file order (`docs/12` §7.4).
    ///
    /// Takes a path rather than an id because the index walks the store once and already has
    /// the paths; an id lookup per session would re-scan everything for each one.
    pub fn records(&self, session: &Path) -> Vec<Record> {
        messages::read_records(session)
    }

    pub fn pinned_ids(&self) -> HashSet<String> {
        pins::pinned_ids(&self.agent_dir)
    }

    pub fn hidden_projects(&self) -> HashSet<String> {
        projects::hidden_projects(&self.agent_dir)
    }
}
