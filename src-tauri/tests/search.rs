//! The app's own search index against **this machine's real store** (decision D7, `docs/12`
//! §7.4).
//!
//! Ignored by default: it reads every one of the user's own session files, which costs what a
//! full pass costs. It is also the only test that can tell "the index is correct on fixtures"
//! (that is `src/search.rs`'s own tests) apart from "the index covers the sessions that are
//! actually here", so it is written to be run deliberately:
//!
//! ```sh
//! cargo test -p omp-desktop --test search -- --ignored --nocapture
//! ```
//!
//! It writes nothing to the store. The store is read through `omp_store`, which only opens files,
//! and the index it builds lives under a temporary directory — the same separation the app keeps
//! between the engine's files and its own.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use omp_desktop::dto::IndexStatus;
use omp_desktop::search::{scan_in_background, Index, NullSink, INDEX_FILE, MAX_LIMIT};
use omp_store::Store;

#[test]
#[ignore = "reads this machine's real store; run with `--ignored --nocapture`"]
fn the_real_store_is_indexed_and_searchable() {
    let store = Store::discover().expect("this machine has an agent directory");

    // A temporary index, thrown away by the next run: the pass may not write to the store it is
    // reading, and a fixture that outlives a failed run is what makes the failure readable.
    let dir = std::env::temp_dir().join("omp-desktop-search-live");
    let _ = std::fs::remove_dir_all(&dir);
    let index = Arc::new(Index::new(Some(dir.clone())));

    let sessions = store.list();
    assert!(
        !sessions.is_empty(),
        "{} holds no sessions to index",
        store.agent_dir().display()
    );
    println!(
        "store: {} sessions under {}",
        sessions.len(),
        store.agent_dir().display()
    );

    // The pass the app runs at startup: started on a worker and watched through the same status
    // the overlay's footer reads.
    let cold = Instant::now();
    scan_in_background(&index, Some(store.clone()), Arc::new(NullSink)).expect("a pass starts");
    assert!(
        index.status().running,
        "a pass is announced as running before it finishes"
    );
    let status = settle(&index);
    let cold = cold.elapsed();

    assert!(!status.running, "the pass settles");
    assert!(
        status.completed_at.is_some(),
        "a pass that reached the end is stamped"
    );
    assert!(status.indexed >= 10, "a real store is not three sessions");
    assert_eq!(
        status.indexed as usize,
        sessions.len(),
        "every session in the store is indexed"
    );

    let records = count_records(&dir);
    let bytes = std::fs::metadata(dir.join(INDEX_FILE))
        .expect("the index file")
        .len();
    println!(
        "cold pass: {cold:?} — {} sessions, {records} records, index {bytes} bytes",
        status.indexed
    );

    // The same pass again: every file is unchanged, so no session is re-read.
    let warm = Instant::now();
    scan_in_background(&index, Some(store.clone()), Arc::new(NullSink)).expect("a pass starts");
    let warm_status = settle(&index);
    let warm = warm.elapsed();

    assert_eq!(
        warm_status.indexed, status.indexed,
        "an unchanged store keeps its sessions"
    );
    println!("incremental pass: {warm:?}");

    // A word that is really in one of the user's sessions, taken from the store rather than
    // written down: the reader here is the one the index uses, so a word it yields is in the
    // index by construction, and a hardcoded one would rot as sessions come and go.
    let (thread, word) = a_word_in(&store);
    let looked = Instant::now();
    let hits = index.search(&word, Some(MAX_LIMIT));
    println!(
        "search {word:?}: {} hits in {:?}",
        hits.len(),
        looked.elapsed()
    );

    let hit = hits
        .iter()
        .find(|hit| hit.thread == thread && hit.text.contains(&word))
        .unwrap_or_else(|| panic!("no hit in {thread} for {word:?}: {hits:?}"));
    assert!(
        matches!(
            hit.kind.as_str(),
            "title" | "prompt" | "answer" | "thinking" | "tool" | "result"
        ),
        "a hit is labelled with the reader's own kind: {hit:?}"
    );

    // And a query that is not a query language, over the real index: `AND` is an FTS5 operator
    // and a search for the word.
    assert!(index.search("AND", Some(10)).len() <= 10);
}

/// Wait for the pass to finish, and answer with the status it settled on.
///
/// Polling rather than awaiting a handle, because polling is what the overlay does: a pass that
/// never finished then fails as a timeout with the status in hand, rather than hanging the test.
fn settle(index: &Index) -> IndexStatus {
    let deadline = Instant::now() + Duration::from_secs(120);

    while index.status().running {
        assert!(
            Instant::now() < deadline,
            "the pass did not finish in two minutes"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    index.status()
}

/// How many records the index holds, read from the index itself.
///
/// The status counts *sessions*, because that is what the footer shows; the number behind them is
/// what says whether a pass wrote what it should have.
fn count_records(dir: &Path) -> i64 {
    let connection = rusqlite::Connection::open(dir.join(INDEX_FILE)).expect("the index opens");
    connection
        .query_row("SELECT COUNT(*) FROM records", [], |row| row.get(0))
        .expect("a record count")
}

/// A word from a real session, and the session it is in.
///
/// The longest word in the whole store, which is the likeliest to be distinctive enough that its
/// own record is not pushed past the hit limit by every other session that mentions it.
fn a_word_in(store: &Store) -> (String, String) {
    let mut best: Option<(String, String)> = None;

    for session in store.list() {
        for record in store.records(&session.path) {
            for word in words(&record.text) {
                if best
                    .as_ref()
                    .is_none_or(|(_, longest)| word.len() > longest.len())
                {
                    best = Some((session.id.clone(), word));
                }
            }
        }
    }

    best.expect("this machine's store has a word in it")
}

/// The words a search can be made of: ASCII, alphabetic, and long enough to be a word rather than
/// something the tokenizer would split.
///
/// ASCII letters are the one alphabet `unicode61` and this split agree on exactly, so a word taken
/// here is a token in the index rather than an approximation of one.
fn words(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_ascii_alphabetic())
        .filter(|word| word.len() >= 8)
        .map(str::to_string)
        .collect()
}
