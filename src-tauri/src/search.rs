//! The app's own cross-thread search index (`docs/12` §7.4, decision D7).
//!
//! At v18.2.6 the engine has no cross-thread search at all: its `history.db` indexes *prompts*
//! and a title cache, and a session nobody has opened has no sidecar to ask. So the index is
//! the app's, and this module is the whole of it — a SQLite FTS5 table in the app's own config
//! directory, fed by `omp_store`'s reader ([`Store::records`]), which is measured on this
//! machine at 13,504 records from 34 sessions in 228 ms.
//!
//! # The schema
//!
//! ```sql
//! sessions (id TEXT PRIMARY KEY, path TEXT NOT NULL, mtime INTEGER NOT NULL,
//!           size INTEGER NOT NULL, indexed_at INTEGER NOT NULL)
//! records  USING fts5(thread UNINDEXED, kind UNINDEXED, ordinal UNINDEXED, text,
//!                     tokenize = 'unicode61 remove_diacritics 2')
//! ```
//!
//! `sessions` answers one question — "is this file the one I already indexed?" — and the
//! answer is `(mtime, size)`, the identity the catalogue's own cache uses
//! (`crates/omp-store/src/listing.rs`). `records` holds the records themselves: `thread` is
//! the session id a hit opens, and `kind` and `ordinal` are `UNINDEXED` because they are
//! carried for the UI rather than searched for — an indexed session id would rank like a word
//! a person typed.
//!
//! # Where the work runs
//!
//! A full pass reads every session file; an incremental pass stats each one and usually writes
//! nothing. Neither belongs on the thread that serves commands, so every entry point here is
//! either a single FTS5 query (milliseconds) or [`scan_in_background`], which hands the pass to
//! a blocking worker and returns. A scan already in flight is never doubled up.
//!
//! # Failure
//!
//! The index is a cache of the store, so losing it costs one rebuild and nothing else — which
//! is why nothing here panics. A query a user typed that is not a query language, an index that
//! is not a database, and a config directory that cannot be written all end as an empty result,
//! a status of `0`, or a rebuild, rather than as a window that disappears.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use omp_store::{Record, SessionSummary, Store};
use rusqlite::{params, Connection, OptionalExtension};
use tauri::{AppHandle, Emitter};

use crate::dto::{IndexStatus, SearchHit, INDEX_EVENT};

/// The index file, beside the app's other files (`crate::bridge::config_dir`).
///
/// Public because the live test (`tests/search.rs`) reports what the pass it just ran cost: the
/// file's size and the record count behind the session count the status carries.
pub const INDEX_FILE: &str = "search-index.db";

/// How many hits a query returns when the caller does not say.
pub const DEFAULT_LIMIT: u32 = 50;

/// The most hits any caller may ask for.
///
/// The reader caps one record at 8 KiB, so this is the ceiling on one answer: 100 × 8 KiB is
/// just under the megabyte `docs/12` §7.4 rules out sending over the IPC boundary for a broad
/// query. A caller that asks for more is clamped rather than refused, because the limit is a
/// `Option<u32>` the overlay may leave out.
pub const MAX_LIMIT: u32 = 100;

/// How often a running scan may announce itself.
///
/// `docs/12` §7.4 asks for progress, not for a per-file readout: a store of thousands of
/// sessions would otherwise put thousands of events through the webview's bridge.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// How long a statement waits for the index's other connection to get out of the way.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// The schema, as one batch: cheap enough to run on every connection, and the only place the
/// shape of the index is written down.
///
/// `IF NOT EXISTS` on the FTS5 table is not a re-creation — measured: a second run of this
/// batch leaves the rows in place. `remove_diacritics 2` is the fixed spelling of the tokenizer
/// (version 1 mishandles some scripts), and it is what makes `naive` find `naïve`.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    id         TEXT PRIMARY KEY,
    path       TEXT NOT NULL,
    mtime      INTEGER NOT NULL,
    size       INTEGER NOT NULL,
    indexed_at INTEGER NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS records USING fts5(
    thread UNINDEXED,
    kind UNINDEXED,
    ordinal UNINDEXED,
    text,
    tokenize = 'unicode61 remove_diacritics 2'
);
";

/// Where a scan's progress goes.
///
/// A trait rather than an `AppHandle` for the same reason `crate::session::ActivitySink` is
/// one: a test drives the same code without a window, and the SQL here has no opinion about the
/// event bus. The sink is passed to the call that announces, not stored, so an `Index` stays
/// what it is — the file and the scan's state.
pub trait ProgressSink: Send + Sync + 'static {
    /// One whole status, whenever a scan starts, advances, or finishes.
    fn progress(&self, status: &IndexStatus);
}

impl ProgressSink for AppHandle {
    fn progress(&self, status: &IndexStatus) {
        let _ = self.emit(INDEX_EVENT, status.clone());
    }
}

/// A sink that drops what it is told, for a caller with no window: the tests, and the live
/// pass in `tests/search.rs` that reads the status back instead.
#[derive(Debug, Default)]
pub struct NullSink;

impl ProgressSink for NullSink {
    fn progress(&self, _status: &IndexStatus) {}
}

/// The index on disk, and the scan in flight.
pub struct Index {
    /// `None` when the platform gave no config directory. Queries then answer with nothing and
    /// a scan refuses, which is a smaller failure than refusing to launch without one.
    dir: Option<PathBuf>,
    state: Mutex<State>,
}

/// What a scan is doing, and when it last said so.
#[derive(Debug, Default)]
struct State {
    running: bool,
    /// Sessions in the store, from the listing the running scan took. Kept across scans so the
    /// overlay's first paint has a denominator rather than a `0`.
    total: u32,
    /// Epoch milliseconds of the last pass that got to the end.
    completed_at: Option<u64>,
    /// Why the last pass did not get to the end, and `None` once one does.
    ///
    /// Carried rather than logged because the overlay's footer is where "search found nothing"
    /// is read, and a corrupt or unreadable index is a different sentence with a different
    /// action behind it. A new pass clears it, so the reason is never older than the attempt.
    error: Option<String>,
    /// When the last event went out, so the interval above is measured rather than guessed.
    announced_at: Option<Instant>,
}

impl Index {
    /// An index kept under `config_dir` — or, when the platform gave no directory to keep one in,
    /// one that answers with nothing rather than pretending to hold sessions.
    pub fn new(config_dir: Option<PathBuf>) -> Self {
        Self {
            dir: config_dir,
            state: Mutex::new(State::default()),
        }
    }

    /// The index as the overlay's footer reads it.
    pub fn status(&self) -> IndexStatus {
        self.snapshot()
    }

    /// The hits for `query`, best first, at most `limit` of them.
    ///
    /// A query that cannot become an FTS5 expression, and a query over an index that cannot be
    /// read, both answer with nothing: the caller is a search box being typed into, where an
    /// error is a message with no action behind it. The failure is still written down, because
    /// "search finds nothing" and "the index is gone" are different problems for whoever has to
    /// diagnose one.
    pub fn search(&self, query: &str, limit: Option<u32>) -> Vec<SearchHit> {
        let Some(expression) = match_expression(query) else {
            return Vec::new();
        };
        let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

        match self.hits(&expression, limit) {
            Ok(hits) => hits,
            Err(error) => {
                eprintln!("[omp-desktop] the search index could not be read: {error}");
                Vec::new()
            }
        }
    }

    /// Mark a scan as started, unless one is already running.
    ///
    /// `false` means this caller did not start one: two passes would write the same rows, and
    /// the second one's listing was taken before the first finished, so it would re-read files
    /// the first has already indexed. A repeated click on "rebuild" is therefore cheap rather
    /// than duplicated.
    pub fn begin(&self, sink: &dyn ProgressSink) -> bool {
        {
            let mut state = self.lock();
            if state.running {
                return false;
            }
            state.running = true;
            state.error = None;
        }

        // Emitted before the first file is read, which is the point: an overlay opened while
        // the app is still indexing says so, rather than showing an index of zero sessions.
        self.announce(sink, true);
        true
    }

    /// Index the whole store, having already been marked as running by [`Index::begin`].
    ///
    /// **Blocking**, and deliberately: `omp_store` reads every session file, and it is the
    /// caller's decision which thread that happens on ([`scan_in_background`]). Returns when the
    /// pass is over and the state has settled — including when it failed, because a scan left
    /// `running` forever is a spinner that never clears.
    pub fn scan(&self, store: Option<&Store>, sink: &dyn ProgressSink) -> Result<(), String> {
        let outcome = self.walk(store, sink);
        self.settle(sink, outcome.as_ref().err().map(String::as_str));
        outcome
    }

    /// Drop a session's rows, because its file is gone.
    ///
    /// `delete_session` calls this once the file is removed. The next scan would drop the rows
    /// anyway — the session is no longer in the listing — but the user is looking at the search
    /// overlay *now*, and `docs/12` §7.4 is explicit that an index which keeps deleted sessions
    /// returns hits for threads that cannot be opened.
    pub fn forget(&self, id: &str) -> Result<(), String> {
        let mut connection = self.open()?;
        forget_ids(&mut connection, &[id])
    }

    /// One pass over the store.
    fn walk(&self, store: Option<&Store>, sink: &dyn ProgressSink) -> Result<(), String> {
        let store = store.ok_or_else(|| {
            "the engine's session store is not available, so there is nothing to index".to_string()
        })?;

        let mut connection = self.open()?;

        // One listing for the whole pass: it is also the denominator the overlay shows, and a
        // listing per session would read two more windows of every file already read.
        let sessions = store.list();
        {
            let mut state = self.lock();
            state.total = sessions.len() as u32;
        }

        for session in &sessions {
            if unchanged(&connection, session)? {
                continue;
            }
            write_session(&mut connection, session, &store.records(&session.path))?;
            self.announce(sink, false);
        }

        drop_missing(&mut connection, &sessions)
    }

    /// The status as of now: the scan's state, and the table's own count.
    fn snapshot(&self) -> IndexStatus {
        let (running, total, completed_at, error) = {
            let state = self.lock();
            (
                state.running,
                state.total,
                state.completed_at,
                state.error.clone(),
            )
        };

        IndexStatus {
            indexed: self.count(),
            total,
            running,
            completed_at,
            error,
        }
    }

    /// How many sessions the index holds.
    ///
    /// The database's own answer rather than a counter kept here: the count moves without this
    /// process doing anything (a scan writes one row per session, `forget` removes one), and a
    /// number kept in two places is one that eventually disagrees with the table it describes.
    /// `0` is the answer for an index that is not there yet as well as for one that cannot be
    /// read, which is what the status means either way: nothing is indexed.
    fn count(&self) -> u32 {
        let Some(path) = self.path() else {
            return 0;
        };

        // Before the first scan there is no file, and a read-only question should not create
        // one.
        if !path.is_file() {
            return 0;
        }

        let count = connect(&path).and_then(|connection| {
            connection.query_row("SELECT COUNT(*) FROM sessions", [], |row| {
                row.get::<_, i64>(0)
            })
        });

        match count {
            Ok(count) => count.clamp(0, u32::MAX as i64) as u32,
            Err(error) => {
                eprintln!("[omp-desktop] the search index could not be counted: {error}");
                0
            }
        }
    }

    /// The rows an FTS5 expression matches, best first.
    fn hits(&self, expression: &str, limit: u32) -> Result<Vec<SearchHit>, String> {
        let connection = self.open()?;
        let mut statement = connection
            .prepare(
                "SELECT thread, kind, ordinal, text FROM records
                 WHERE records MATCH ?1 ORDER BY bm25(records) LIMIT ?2",
            )
            .map_err(failed)?;

        let rows = statement
            .query_map(params![expression, limit], |row| {
                Ok(SearchHit {
                    thread: row.get(0)?,
                    kind: row.get(1)?,
                    // An INTEGER on the way in and on the way out — measured, because an FTS5
                    // column is otherwise a text column and a `?` on a number is a detail worth
                    // having checked rather than assumed.
                    ordinal: row.get::<_, i64>(2)?.max(0) as u64,
                    text: row.get(3)?,
                })
            })
            .map_err(failed)?;

        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(failed)
    }

    /// Emit `index-progress`, unless the last one was a moment ago.
    ///
    /// The payload is the whole status, so a window that missed an interval is behind in time
    /// rather than in content. `force` bypasses the interval for the two events a spinner hangs
    /// off: the start and the end.
    fn announce(&self, sink: &dyn ProgressSink, force: bool) {
        {
            let mut state = self.lock();
            let now = Instant::now();
            let due = force
                || state
                    .announced_at
                    .is_none_or(|last| now.duration_since(last) >= PROGRESS_INTERVAL);
            if !due {
                return;
            }
            state.announced_at = Some(now);
        }

        // Outside the lock: the sink crosses into the webview, and a slow one must not hold up
        // a `status()` that is only reading four fields.
        sink.progress(&self.snapshot());
    }

    /// Settle the scan's state, and tell the overlay it is over.
    ///
    /// Always announces, including for a failed pass: the event is what clears a spinner, and
    /// the reason it carries is why the spinner cleared without the index moving. A failed pass
    /// leaves `completed_at` where it was, because the last *complete* index is the honest
    /// answer to "when was this last right?".
    fn settle(&self, sink: &dyn ProgressSink, error: Option<&str>) {
        {
            let mut state = self.lock();
            state.running = false;
            state.error = error.map(str::to_string);
            if error.is_none() {
                state.completed_at = Some(now_ms());
            }
        }

        self.announce(sink, true);
    }

    /// The index file, when the platform gave a directory to keep it in.
    fn path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|dir| dir.join(INDEX_FILE))
    }

    /// Open the index, ready to be read or written.
    fn open(&self) -> Result<Connection, String> {
        let path = self.path().ok_or_else(|| {
            "the app has no config directory, so it has nowhere to keep a search index".to_string()
        })?;

        if let Some(dir) = self.dir.as_ref() {
            std::fs::create_dir_all(dir)
                .map_err(|error| format!("{} could not be created: {error}", dir.display()))?;
        }

        match connect(&path) {
            Ok(connection) => Ok(connection),
            // A file that is not a database is a cache to be thrown away, and there is nothing
            // to salvage: the store is the source of truth and a full pass over it takes a
            // fraction of a second. Only corruption is handled this way — a permission error or
            // a full disk would not be fixed by deleting the index, and it is reported instead.
            Err(error) if is_corruption(&error) => {
                eprintln!(
                    "[omp-desktop] the search index is not a database, so it is being rebuilt: {error}"
                );
                // The database *and* the two files a WAL leaves beside it: a crash can leave a
                // stale log behind, and applying one to a fresh database is a second failure
                // rather than a rebuild.
                for stale in index_files(&path) {
                    match std::fs::remove_file(&stale) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            return Err(format!(
                                "{} could not be replaced: {error}",
                                stale.display()
                            ))
                        }
                    }
                }
                connect(&path).map_err(|error| unopenable(&path, &error))
            }
            Err(error) => Err(unopenable(&path, &error)),
        }
    }

    /// The scan's state, even if a previous holder panicked.
    ///
    /// Nothing in this module panics while holding it, but a poisoned lock would leave a host
    /// that cannot answer `index_status` at all — and the status is exactly what a person reads
    /// to find out that something is wrong.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Start a scan on a worker of its own, and return without waiting for it.
///
/// The two callers are the app's startup and the overlay's "rebuild", and neither may wait:
/// a pass reads every session file, so a command that awaited one would be a window that does
/// not answer while it runs — the thing `docs/12` §7.4 rules out. A scan already in flight makes
/// this a no-op rather than a second pass ([`Index::begin`]).
///
/// A pass that fails announces that it is over and writes the reason to stderr: the caller is a
/// window that asked for a refresh, and the event's `running: false` is the answer it can act on.
pub fn scan_in_background(
    index: &Arc<Index>,
    store: Option<Store>,
    sink: Arc<dyn ProgressSink>,
) -> Result<(), String> {
    if !index.begin(&*sink) {
        return Ok(());
    }

    let index = Arc::clone(index);
    // Dropped on purpose: dropping the handle leaves the pass running on the blocking pool
    // instead of cancelling it, which is the whole shape of this function — nobody waits, and the
    // `index-progress` events are how the window hears about it.
    drop(tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = index.scan(store.as_ref(), &*sink) {
            eprintln!("[omp-desktop] the search index was not rebuilt: {error}");
        }
    }));

    Ok(())
}

/// Open the index at `path`, with the schema in place.
///
/// One connection per operation rather than one held for the life of the process: SQLite's own
/// locking is what keeps a scan and a query apart, and a connection this module never hands out
/// is one nothing can leave mid-transaction.
fn connect(path: &Path) -> Result<Connection, rusqlite::Error> {
    let connection = Connection::open(path)?;

    // WAL, because the index does two things at once: a scan writes while the overlay's
    // keystrokes read, and in the default rollback journal the two take turns on the whole file.
    connection.pragma_update(None, "journal_mode", "WAL")?;
    // The documented pairing with WAL: a commit is not fsynced, and the worst a power cut can
    // cost is the pass that was running — which the next one repeats.
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.execute_batch(SCHEMA)?;

    Ok(connection)
}

/// Whether this is the index telling us it is not a database.
///
/// The one SQLite failure a rebuild fixes. Everything else — a directory that cannot be created,
/// a disk that is full — is reported, because removing the index would not help.
fn is_corruption(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(failure, _)
            if matches!(
                failure.code,
                rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase
            )
    )
}

/// The index's own files: the database, and the two a WAL keeps beside it.
fn index_files(path: &Path) -> [PathBuf; 3] {
    let sibling = |suffix: &str| {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        PathBuf::from(name)
    };

    [path.to_path_buf(), sibling("-wal"), sibling("-shm")]
}

/// An index that could not be opened, named.
///
/// The path is the whole diagnosis for the failures that happen in practice — a config
/// directory that is not writable, a file that is not a database — and the caller only ever sees
/// the sentence.
fn unopenable(path: &Path, error: &rusqlite::Error) -> String {
    format!(
        "the search index at {} could not be opened: {error}",
        path.display()
    )
}

/// The sentence a failed statement gets.
fn failed(error: rusqlite::Error) -> String {
    format!("the search index failed: {error}")
}

/// Whether this file is the one the index already has.
///
/// `(mtime, size)` and nothing else: the identity the catalogue's own cache uses, so a session
/// the sidebar considers unchanged is one the index does not re-read. A file that was appended
/// to, or rewritten in place, moves both — which is the case this exists to catch.
fn unchanged(connection: &Connection, session: &SessionSummary) -> Result<bool, String> {
    let recorded = connection
        .query_row(
            "SELECT mtime, size FROM sessions WHERE id = ?1",
            params![session.id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(failed)?;

    Ok(recorded == Some((session.modified_ms as i64, session.size as i64)))
}

/// Replace one session's records with what its file now holds, in one transaction.
///
/// Per session rather than per pass: a pass is thousands of records, and a transaction spanning
/// all of them would hold the write lock for its whole length — every query would queue behind
/// it — and lose the entire pass to one unreadable file. Committed per session, a session is
/// never half-indexed, and a reader sees the count move from 12 to 13 rather than from 0 to 34.
fn write_session(
    connection: &mut Connection,
    session: &SessionSummary,
    records: &[Record],
) -> Result<(), String> {
    let transaction = connection.transaction().map_err(failed)?;

    // Delete-then-insert rather than an update: a file that was rewritten can hold fewer
    // records than it did, and rows left behind would be hits for text the thread no longer
    // contains.
    transaction
        .execute("DELETE FROM records WHERE thread = ?1", params![session.id])
        .map_err(failed)?;

    {
        let mut insert = transaction
            .prepare("INSERT INTO records (thread, kind, ordinal, text) VALUES (?1, ?2, ?3, ?4)")
            .map_err(failed)?;

        for record in records {
            insert
                .execute(params![
                    session.id,
                    record.kind.as_str(),
                    record.ordinal as i64,
                    record.text
                ])
                .map_err(failed)?;
        }
    }

    transaction
        .execute(
            "INSERT INTO sessions (id, path, mtime, size, indexed_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET path = excluded.path,
                                           mtime = excluded.mtime,
                                           size = excluded.size,
                                           indexed_at = excluded.indexed_at",
            params![
                session.id,
                session.path.display().to_string(),
                session.modified_ms as i64,
                session.size as i64,
                now_ms() as i64
            ],
        )
        .map_err(failed)?;

    transaction.commit().map_err(failed)
}

/// Drop the sessions the store no longer has.
///
/// The listing is the whole truth about what exists, so anything the index holds that it does
/// not name is gone — deleted through `delete_session`, by the engine, or by the user's shell —
/// and keeping its rows would return hits for a thread that cannot be opened.
fn drop_missing(connection: &mut Connection, sessions: &[SessionSummary]) -> Result<(), String> {
    let present: HashSet<&str> = sessions.iter().map(|session| session.id.as_str()).collect();

    let indexed: Vec<String> = {
        let mut statement = connection
            .prepare("SELECT id FROM sessions")
            .map_err(failed)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(failed)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(failed)?
    };

    let missing: Vec<&str> = indexed
        .iter()
        .filter(|id| !present.contains(id.as_str()))
        .map(String::as_str)
        .collect();

    forget_ids(connection, &missing)
}

/// Remove these sessions from the index, in one transaction.
///
/// One helper for the two callers — the user's delete, and a pass that found a session gone —
/// because they are the same write and a half-applied one is the bug both exist to prevent.
fn forget_ids(connection: &mut Connection, ids: &[&str]) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }

    let transaction = connection.transaction().map_err(failed)?;
    for id in ids {
        transaction
            .execute("DELETE FROM records WHERE thread = ?1", params![id])
            .map_err(failed)?;
        transaction
            .execute("DELETE FROM sessions WHERE id = ?1", params![id])
            .map_err(failed)?;
    }
    transaction.commit().map_err(failed)
}

/// The FTS5 `MATCH` expression for what a user typed, or `None` when there is nothing in it.
///
/// FTS5's query language has operators — `AND`, `OR`, `NOT`, `NEAR`, `*`, `^`, `:` — and a
/// person typing `"` or `naive AND rust` is not writing a query in it. Measured, the raw strings
/// are worse than useless: `AND` is a syntax error, `c++` is a syntax error, `foo:bar` is
/// *"no such column: foo"*, `a - b` is *"no such column: b"*, and a lone `"` is *"unterminated
/// string"*. So every whitespace-separated term is quoted, which makes it a phrase, and the
/// phrases are joined with `AND`.
///
/// That leaves a term that is only punctuation (`--`, `"`) as a phrase with no tokens, which
/// FTS5 matches against nothing rather than rejecting — also measured, and the reason there is
/// no filter over the terms here. What it cannot have is an *empty* expression (`MATCH ''` is a
/// syntax error), which is the one case this answers `None` for: a query of nothing but
/// whitespace finds nothing.
///
/// The last term also gets a prefix `*`: the overlay searches on every keystroke (120 ms
/// debounce, `frontend/src/components/SearchOverlay.vue`), so the word still being typed has to
/// match the words that start with it, or the results would only appear once a word was
/// finished.
fn match_expression(query: &str) -> Option<String> {
    let mut terms: Vec<String> = query
        .split_whitespace()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect();

    let last = terms.last_mut()?;
    last.push('*');

    Some(terms.join(" AND "))
}

/// Unix milliseconds, in the frontend's own unit (`IndexStatus.completed_at`).
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A temporary agent directory with an index beside it, named for the test that made it.
    ///
    /// `std::env::temp_dir` rather than a temporary-directory crate, as `crates/omp-store`'s own
    /// tests do: one fewer dependency, and a fixture that outlives a failed run is what makes the
    /// failure readable.
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("omp-desktop-search-{name}"));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("a temp directory");
            Self { root }
        }

        fn store(&self) -> Store {
            Store::at(self.root.join("agent"))
        }

        /// An index under the fixture rather than under the real app config directory: the same
        /// separation the app keeps between the engine's store and its own files.
        fn index(&self) -> Index {
            Index::new(Some(self.root.join("config")))
        }

        fn index_file(&self) -> PathBuf {
            self.root.join("config").join(INDEX_FILE)
        }

        /// Write one session the way the engine writes one: a title slot, a header, then
        /// messages. Returns its path, for the tests that change or delete it.
        fn session(&self, id: &str, title: &str, messages: &[(&str, &str)]) -> PathBuf {
            let bucket = self.store().sessions_root().join("-tmp-project");
            std::fs::create_dir_all(&bucket).expect("a bucket");
            let path = bucket.join(format!("2026-09-20T00-00-00-000Z_{id}.jsonl"));

            let mut body = format!(
                "{}\n{}\n",
                serde_json::json!({ "type": "title", "title": title, "source": "user" }),
                serde_json::json!({
                    "type": "session", "version": 3, "id": id, "cwd": "/tmp/project"
                })
            );
            for (role, text) in messages {
                body.push_str(&format!(
                    "{}\n",
                    serde_json::json!({
                        "type": "message",
                        "message": {
                            "role": role,
                            "content": [{ "type": "text", "text": text }]
                        }
                    })
                ));
            }

            std::fs::write(&path, body).expect("a session file");
            path
        }

        /// One message appended to a session, which is the shape a live session's file takes.
        fn append(&self, path: &Path, role: &str, text: &str) {
            let mut body = std::fs::read_to_string(path).expect("the fixture file");
            body.push_str(&format!(
                "{}\n",
                serde_json::json!({
                    "type": "message",
                    "message": { "role": role, "content": [{ "type": "text", "text": text }] }
                })
            ));
            std::fs::write(path, body).expect("the changed file");
        }
    }

    /// One pass over the fixture's store, the way the app runs one.
    fn scan(fixture: &Fixture, index: &Index) -> IndexStatus {
        index
            .scan(Some(&fixture.store()), &NullSink)
            .expect("a pass over the fixture");
        index.status()
    }

    /// Each indexed session, and the moment the index last wrote it.
    fn stamped(index: &Index) -> HashMap<String, i64> {
        let connection = connect(&index.path().expect("an index path")).expect("the index opens");
        let mut statement = connection
            .prepare("SELECT id, indexed_at FROM sessions")
            .expect("the sessions table");
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .expect("the rows");

        rows.collect::<rusqlite::Result<HashMap<_, _>>>()
            .expect("the stamps")
    }

    /// A sink that keeps what it was told, for what a status read afterwards cannot show.
    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<IndexStatus>>);

    impl RecordingSink {
        fn announced(&self) -> Vec<IndexStatus> {
            self.0.lock().expect("the sink").clone()
        }
    }

    impl ProgressSink for RecordingSink {
        fn progress(&self, status: &IndexStatus) {
            self.0.lock().expect("the sink").push(status.clone());
        }
    }

    #[test]
    fn a_hit_carries_the_thread_kind_and_ordinal_it_matched() {
        let fixture = Fixture::new("hit");
        fixture.session(
            "01alpha",
            "Alpha",
            &[
                ("user", "why does the tokenizer re-read the buffer?"),
                ("assistant", "because the naive scan is cheap"),
            ],
        );
        fixture.session("01beta", "Beta", &[("user", "the weather is fine")]);

        let index = fixture.index();
        let status = scan(&fixture, &index);
        assert_eq!(
            (status.indexed, status.total, status.running),
            (2, 2, false)
        );
        assert!(status.completed_at.is_some());

        let hits = index.search("tokenizer", None);
        assert_eq!(hits.len(), 1, "one record mentions it: {hits:?}");
        assert_eq!(hits[0].thread, "01alpha");
        assert_eq!(hits[0].kind, "prompt");
        assert_eq!(hits[0].ordinal, 1);
        assert!(hits[0].text.contains("re-read the buffer"), "{:?}", hits[0]);

        // The kinds and ordinals are the reader's, so a hit on reasoning or a tool result is
        // labelled as one rather than flattened into the message it came from.
        let hits = index.search("naive", None);
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].kind.as_str(), hits[0].ordinal), ("answer", 2));

        // The title is a record too, at ordinal 0.
        let hits = index.search("Beta", None);
        assert_eq!((hits[0].kind.as_str(), hits[0].ordinal), ("title", 0));

        // And a half-typed word finds the words that start with it: the overlay searches on
        // every keystroke, so this is what makes the results appear while the word is written.
        assert_eq!(index.search("tokeniz", None).len(), 1, "prefix");
        assert_eq!(index.search("tok", None).len(), 1, "shorter prefix");
    }

    #[test]
    fn a_second_pass_over_unchanged_files_writes_nothing() {
        let fixture = Fixture::new("unchanged");
        fixture.session(
            "01alpha",
            "Alpha",
            &[("user", "the tokenizer re-reads the buffer")],
        );
        fixture.session("01beta", "Beta", &[("user", "nothing to do with it")]);

        let index = fixture.index();
        scan(&fixture, &index);
        let before = stamped(&index);
        assert_eq!(before.len(), 2);

        // A pass that re-indexed anything would stamp a later `indexed_at`, so two equal maps
        // are the whole assertion. The sleep is what makes "later" measurable rather than a
        // coincidence of the clock's resolution.
        std::thread::sleep(Duration::from_millis(5));
        scan(&fixture, &index);

        assert_eq!(
            stamped(&index),
            before,
            "an unchanged store must not be re-indexed"
        );
    }

    #[test]
    fn a_changed_file_is_the_one_that_is_re_indexed() {
        let fixture = Fixture::new("changed");
        let path = fixture.session("01alpha", "Alpha", &[("user", "the original wording")]);
        fixture.session("01beta", "Beta", &[("user", "the other session")]);

        let index = fixture.index();
        scan(&fixture, &index);
        let before = stamped(&index);

        // An append is what a live session's file does: the same file, one more message, a size
        // and an mtime that both moved.
        fixture.append(&path, "user", "a replacement wording entirely");

        std::thread::sleep(Duration::from_millis(5));
        let status = scan(&fixture, &index);

        assert_eq!(status.indexed, 2);
        let after = stamped(&index);
        assert!(
            after["01alpha"] > before["01alpha"],
            "the changed session was re-indexed"
        );
        assert_eq!(
            after["01beta"], before["01beta"],
            "the untouched session was not"
        );

        // What the index answers with moved with the file, and the record that was there before
        // is still there: an append adds.
        let hits = index.search("replacement", None);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].thread, "01alpha");
        assert_eq!(index.search("original", None).len(), 1);
    }

    #[test]
    fn a_session_that_is_gone_loses_its_rows() {
        let fixture = Fixture::new("deleted");
        let path = fixture.session("01alpha", "Alpha", &[("user", "a distinctive pheasant")]);
        fixture.session("01beta", "Beta", &[("user", "the other session")]);

        let index = fixture.index();
        scan(&fixture, &index);
        assert_eq!(index.search("pheasant", None).len(), 1);

        std::fs::remove_file(&path).expect("the session file is gone");
        let status = scan(&fixture, &index);

        assert_eq!((status.indexed, status.total), (1, 1));
        assert!(
            index.search("pheasant", None).is_empty(),
            "a session the store no longer has must not be searchable"
        );

        // And the write `delete_session` makes, without waiting for a pass.
        index.forget("01beta").expect("a session to forget");
        assert_eq!(index.status().indexed, 0);
        assert!(index.search("other", None).is_empty());
    }

    #[test]
    fn a_query_a_person_would_type_is_a_search_and_not_an_error() {
        let fixture = Fixture::new("operators");
        fixture.session(
            "01alpha",
            "Alpha",
            &[(
                "user",
                "the naive rust bindings, a c++ wrapper, and a stray \" quote",
            )],
        );

        let index = fixture.index();
        scan(&fixture, &index);

        // Every one of these is a failure for FTS5 as written — measured: `AND` and `c++` are
        // syntax errors, `a - b` and `foo:bar` are "no such column", a lone quote is an
        // unterminated string, `*` is an unknown special query. As a search they all have to
        // reach SQLite and come back.
        for query in [
            "AND",
            "c++",
            "a - b",
            "foo:bar",
            "\"",
            "()",
            "NEAR(",
            "*",
            "^naive",
            "naive AND rust",
            "--",
            "naïve",
        ] {
            let expression = match_expression(query).expect("a query with something in it");
            let hits = index.hits(&expression, 10).unwrap_or_else(|error| {
                panic!("{query:?} has to be a search, not an error: {error}")
            });
            assert!(
                hits.len() <= 1,
                "{query:?} matched more than the one record there is: {hits:?}"
            );
        }

        // The operator is searched for as the text it is: the record's words include "and".
        let hits = index.search("AND", Some(10));
        assert!(hits.iter().any(|hit| hit.thread == "01alpha"), "{hits:?}");

        // A diacritic is not a barrier either way.
        assert_eq!(index.search("naive", None).len(), 1, "naive finds naïve");

        // A query with nothing in it finds nothing, and never reaches SQLite: `MATCH ''` is a
        // syntax error rather than an empty result.
        assert!(match_expression("   ").is_none());
        assert!(index.search("   ", None).is_empty());
    }

    #[test]
    fn an_index_with_nowhere_to_live_answers_with_nothing() {
        let index = Index::new(None);

        assert!(index.search("anything", None).is_empty());
        let status = index.status();
        assert_eq!(
            (
                status.indexed,
                status.total,
                status.running,
                status.completed_at
            ),
            (0, 0, false, None)
        );

        assert!(
            index.forget("01alpha").is_err(),
            "there is nothing to forget"
        );
        assert!(
            index
                .scan(Some(&Store::at(std::env::temp_dir())), &NullSink)
                .is_err(),
            "a scan has nowhere to write"
        );
    }

    /// A pass that could not finish says why, and the next pass that does clears it.
    ///
    /// The reason is the whole difference between "the store has nothing in it" and "the index
    /// could not be read", which are the same three words in the overlay's footer otherwise.
    #[test]
    fn a_pass_that_failed_carries_its_reason_and_a_later_one_clears_it() {
        let fixture = Fixture::new("reason");
        fixture.session("01alpha", "Alpha", &[("user", "a distinctive pheasant")]);

        let index = fixture.index();
        assert_eq!(
            scan(&fixture, &index).error,
            None,
            "a pass that reached the end has nothing to report"
        );

        // An index with no config directory has nowhere to write, so the pass cannot finish —
        // the same shape as a disk that has gone away under it.
        let nowhere = Index::new(None);
        assert!(
            nowhere.scan(Some(&fixture.store()), &NullSink).is_err(),
            "a scan with nowhere to write must fail"
        );

        let failed = nowhere.status();
        assert!(
            failed.error.is_some(),
            "a failed pass carries its reason, not just a cleared spinner"
        );
        assert!(
            !failed.running,
            "and does not leave the pass claiming to run"
        );
        assert_eq!(
            failed.completed_at, None,
            "a pass that never finished cannot say when the index was last right"
        );

        // A later pass that finishes forgets the older failure rather than reporting both.
        let after = scan(&fixture, &index);
        assert_eq!(after.error, None);
        assert!(after.completed_at.is_some());
    }

    #[test]
    fn an_index_that_is_not_a_database_is_rebuilt() {
        let fixture = Fixture::new("corrupt");
        fixture.session("01alpha", "Alpha", &[("user", "a distinctive pheasant")]);

        let index = fixture.index();
        scan(&fixture, &index);

        std::fs::write(fixture.index_file(), b"not a database at all").expect("the corruption");

        // The store is the truth, so the answer is a rebuild rather than an error: the file is
        // thrown away, the query answers with nothing, and the next pass reads the store again.
        assert!(
            index.search("pheasant", None).is_empty(),
            "a corrupt index cannot answer"
        );
        assert_eq!(index.status().indexed, 0);

        let status = scan(&fixture, &index);
        assert_eq!(status.indexed, 1);
        assert_eq!(index.search("pheasant", None).len(), 1);
    }

    #[test]
    fn a_pass_announces_itself_when_it_starts_and_when_it_ends() {
        let fixture = Fixture::new("progress");
        fixture.session("01alpha", "Alpha", &[("user", "a distinctive pheasant")]);

        let index = fixture.index();
        let sink = RecordingSink::default();

        assert!(index.begin(&sink), "the first caller starts a pass");
        assert!(!index.begin(&sink), "a pass in flight is not doubled up");
        assert!(index.status().running);

        index.scan(Some(&fixture.store()), &sink).expect("the pass");

        // A start with `running`, an end without it, and nothing in between faster than the
        // interval: a store of thousands of sessions must not put thousands of events through
        // the webview.
        let announced = sink.announced();
        assert!(announced.len() >= 2, "a start and an end: {announced:?}");
        assert!(announced.len() <= 4, "rate limited: {announced:?}");
        assert!(announced.first().expect("a start").running);
        let last = announced.last().expect("an end");
        assert!(!last.running);
        assert_eq!(last.indexed, 1);
        assert!(last.completed_at.is_some());
        assert!(!index.status().running);
    }
}
