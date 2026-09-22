//! Where the engine keeps its files, as observed on disk at v18.2.6.
//!
//! ```text
//!   <agent dir>/                      PI_CODING_AGENT_DIR, else ~/.omp/agent
//!     sessions/
//!       -Projects-MyApp/              one bucket per working directory
//!         2026-09-19T21-05-55-808Z_01a0bb7d-….jsonl
//!         2026-09-19T21-05-55-808Z_01a0bb7d-….jsonl/   ← that session's artifacts
//!     session-pins.json               a JSON array of session ids
//!     projects.json                   the project registry
//!     history.db                      prompt history + a title cache (not a message index)
//! ```
//!
//! **A bucket name is not a path, and this crate never pretends it is.** The engine
//! *encodes* a cwd into a bucket name (`-` + the path relative to `$HOME`, or `-tmp` +
//! the path relative to `os.tmpdir()`, or `--<absolute>--`), and the encoding is lossy:
//! `session-paths.ts`'s `encodeRelativeSessionDirName` maps every `/`, `\` and `:` to
//! `-`, so `-tmp-omp-8d-app` could be `/tmp/omp-8d-app` and `~/tmp/omp/8d/app` equally.
//! There is no decoder upstream, because none is needed there — and it is not needed
//! here either, because **every session header records its own `cwd`** (measured:
//! `{"type":"session","version":3,"id":"01a0bb7d-…","cwd":"/home/GioViale/Projects/…"}`).
//! The catalogue therefore groups by the recorded cwd and treats the bucket name as
//! nothing more than the directory to read.

use std::env;
use std::path::{Path, PathBuf};

/// The engine's own environment override for its agent directory.
///
/// Read at v18.2.6: `PI_CODING_AGENT_DIR` relocates everything under the agent dir
/// (`docs/06` §1.2). A host that ignored it would list one store and drive another.
const AGENT_DIR_ENV: &str = "PI_CODING_AGENT_DIR";

/// The agent directory as this process should read it.
///
/// `None` when neither the override nor a home directory can be resolved, which is the
/// one case where "no sessions" and "the wrong directory" would look identical — so the
/// caller is told rather than handed an empty list.
pub fn agent_dir() -> Option<PathBuf> {
    if let Some(override_dir) = env::var_os(AGENT_DIR_ENV) {
        let path = PathBuf::from(override_dir);
        if !path.as_os_str().is_empty() {
            return Some(path);
        }
    }

    // `$HOME` rather than a platform API: the engine resolves it the same way
    // (`getAgentDir` → `os.homedir()`), and a mismatch here would read a different store
    // than the sidecar writes.
    let home = env::var_os("HOME").map(PathBuf::from)?;
    Some(home.join(".omp").join("agent"))
}

/// The directory holding one bucket per working directory.
pub fn sessions_root(agent_dir: &Path) -> PathBuf {
    agent_dir.join("sessions")
}

/// The persisted pin set (`session-pins.json`): a JSON array of session ids.
pub fn pins_file(agent_dir: &Path) -> PathBuf {
    agent_dir.join("session-pins.json")
}

/// The engine's project registry (`projects.json`).
pub fn projects_file(agent_dir: &Path) -> PathBuf {
    agent_dir.join("projects.json")
}

/// The session id inside a `<file-safe-timestamp>_<id>.jsonl` name.
///
/// The id after the last `_` is the engine's own rule (`sessionIdFromSessionPath`); the
/// timestamp half is file-safe (colons become `-`), so an id is never mistaken for one.
pub fn session_id_from_path(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".jsonl")?;
    let (_, id) = stem.rsplit_once('_')?;
    (!id.is_empty()).then(|| id.to_string())
}

/// Every session bucket in the store, as `<dir, files>` pairs, newest bucket first by
/// nothing in particular — the caller sorts the sessions, not the directories.
///
/// Unreadable buckets are skipped rather than failing the listing: one bad directory
/// must not hide every other session from the browser.
pub fn buckets(sessions_root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(sessions_root) else {
        return Vec::new();
    };

    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    dirs.sort();
    dirs
}

/// The `*.jsonl` files in one bucket, sorted by path for a stable listing order.
///
/// The artifacts directory a session owns shares the file's stem and has no extension, so
/// filtering on the extension is what keeps a session's own artifacts from being read as
/// a session.
pub fn session_files(bucket: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(bucket) else {
        return Vec::new();
    };

    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
                && path.is_file()
        })
        .collect();
    files.sort();
    files
}
