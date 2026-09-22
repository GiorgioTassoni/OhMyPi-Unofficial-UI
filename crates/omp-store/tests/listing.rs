//! The catalogue against fixtures that mirror real session files.
//!
//! Every shape here was copied from files this machine's engine actually wrote — the
//! 256-byte title slot, the header, the message entries, a `compaction` with a
//! `shortSummary`, a tail window cut mid-line. The rules being asserted are transcribed
//! from the engine's own `session-listing.ts`, because the sidebar and the engine's resume
//! picker show the same sessions and a disagreement would be visible as a wrong badge.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use omp_store::listing::{self, Lifecycle};
use omp_store::Store;

/// A bucket name in the engine's own shape (a cwd under the temp root).
const BUCKET: &str = "-tmp-omp-store-fixtures";
const CWD: &str = "/tmp/omp-store-fixtures";

/// An empty fixture store under a per-test name, so tests can run in parallel.
fn store(name: &str) -> Store {
    let root = std::env::temp_dir().join(format!("omp-store-{name}"));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("sessions").join(BUCKET)).expect("fixture store");
    Store::at(&root)
}

fn bucket_of(store: &Store) -> PathBuf {
    store.sessions_root().join(BUCKET)
}

/// A session file with the given lines, as the engine would have written them.
fn write_session(store: &Store, id: &str, lines: &[String]) -> PathBuf {
    let path = bucket_of(store).join(format!("2026-09-01T10-00-00-000Z_{id}.jsonl"));
    let mut body = String::new();
    for line in lines {
        body.push_str(line);
        body.push('\n');
    }
    fs::write(&path, body).expect("session file");
    path
}

/// The fixed-width title slot the engine keeps as line 1.
///
/// The title is interpolated as a JSON string, not as text: a fixture that wrote
/// `"title":Fix the parser` produced a line no parser accepts, and the test then passed
/// against the *header's* title without anyone noticing.
fn slot(title: &str, source: &str) -> String {
    let title = serde_json::Value::String(title.to_string());
    format!(
        r#"{{"type":"title","v":1,"title":{title},"source":"{source}","updatedAt":"2026-09-01T10:00:00.000Z","pad":"          "}}"#
    )
}

/// The header the engine writes as line 2, with optional fields appended.
fn header(id: &str, extra: &str) -> String {
    format!(
        r#"{{"type":"session","version":3,"id":"{id}","timestamp":"2026-09-01T10:00:00.000Z","cwd":"{CWD}"{extra}}}"#
    )
}

fn user(text: &str) -> String {
    format!(
        r#"{{"type":"message","id":"m-u","parentId":null,"message":{{"role":"user","content":[{{"type":"text","text":{}}}]}}}}"#,
        serde_json::Value::String(text.to_string())
    )
}

/// An assistant message whose content is the given blocks.
fn assistant(stop_reason: &str, blocks: &str) -> String {
    format!(
        r#"{{"type":"message","id":"m-a","parentId":"m-u","message":{{"role":"assistant","stopReason":"{stop_reason}","content":[{blocks}]}}}}"#
    )
}

fn tool_result() -> String {
    r#"{"type":"message","id":"m-r","parentId":"m-a","message":{"role":"toolResult","toolCallId":"c1","content":[{"type":"text","text":"ok"}]}}"#.to_string()
}

const TEXT_BLOCK: &str = r#"{"type":"text","text":"done"}"#;
const TOOL_CALL_BLOCK: &str = r#"{"type":"toolCall","id":"c1","name":"bash"}"#;

#[test]
fn titles_follow_the_slot_then_the_header_then_a_summary() {
    let store = store("titles");

    // The slot is rewritten in place on every rename, so it outranks the header, which keeps
    // whatever the session was created with.
    write_session(
        &store,
        "00000000-0000-7000-8000-000000000001",
        &[
            slot("Fix the parser", "user"),
            header(
                "00000000-0000-7000-8000-000000000001",
                r#","title":"Brainstorm","titleSource":"auto""#,
            ),
            user("hello"),
        ],
    );
    // No slot: the header's title is all there is.
    write_session(
        &store,
        "00000000-0000-7000-8000-000000000002",
        &[
            header(
                "00000000-0000-7000-8000-000000000002",
                r#","title":"Header only","titleSource":"auto""#,
            ),
            user("hi"),
        ],
    );
    // Neither: a compaction's summary names the session.
    write_session(
        &store,
        "00000000-0000-7000-8000-000000000003",
        &[
            header("00000000-0000-7000-8000-000000000003", ""),
            user("hi"),
            r#"{"type":"compaction","shortSummary":"Compact summary of work"}"#.to_string(),
        ],
    );

    let sessions = store.list();
    let by_id = |id: &str| {
        sessions
            .iter()
            .find(|session| session.id == id)
            .expect("session listed")
    };

    assert_eq!(
        by_id("00000000-0000-7000-8000-000000000001")
            .title
            .as_deref(),
        Some("Fix the parser")
    );
    assert_eq!(
        by_id("00000000-0000-7000-8000-000000000001")
            .title_source
            .as_deref(),
        Some("user")
    );
    assert_eq!(
        by_id("00000000-0000-7000-8000-000000000002")
            .title
            .as_deref(),
        Some("Header only")
    );
    assert_eq!(
        by_id("00000000-0000-7000-8000-000000000003")
            .title
            .as_deref(),
        Some("Compact summary of work")
    );
}

#[test]
fn a_blank_slot_title_falls_through_to_the_header() {
    let store = store("blank-slot");

    write_session(
        &store,
        "00000000-0000-7000-8000-000000000010",
        &[
            // The engine's `normalizeTitleOverride` treats whitespace as "no title".
            slot("   ", "auto"),
            header(
                "00000000-0000-7000-8000-000000000010",
                r#","title":"Real name","titleSource":"auto""#,
            ),
            user("hi"),
        ],
    );

    let session = &store.list()[0];
    assert_eq!(session.title.as_deref(), Some("Real name"));
}

#[test]
fn a_display_name_falls_back_to_the_first_message() {
    let store = store("display-name");

    write_session(
        &store,
        "00000000-0000-7000-8000-000000000011",
        &[
            header("00000000-0000-7000-8000-000000000011", ""),
            user("  explain the diff  "),
        ],
    );

    let session = &store.list()[0];
    assert_eq!(session.title, None);
    assert_eq!(session.display_name().as_deref(), Some("explain the diff"));
    assert_eq!(session.first_message, "explain the diff");
}

#[test]
fn a_session_with_nothing_in_it_has_no_display_name() {
    let store = store("empty-session");

    write_session(
        &store,
        "00000000-0000-7000-8000-000000000012",
        &[header("00000000-0000-7000-8000-000000000012", "")],
    );

    let session = &store.list()[0];
    assert_eq!(session.first_message, "(no messages)");
    assert_eq!(session.display_name(), None);
    assert_eq!(session.message_count, 0);
    assert_eq!(session.lifecycle, Lifecycle::Unknown);
}

#[test]
fn the_parent_resolves_from_an_id_or_a_path() {
    let store = store("parents");
    let parent = "00000000-0000-7000-8000-000000000020";

    write_session(&store, parent, &[header(parent, ""), user("hi")]);
    // `fork()` records the parent *id* (session-manager.ts:1912).
    write_session(
        &store,
        "00000000-0000-7000-8000-000000000021",
        &[
            header(
                "00000000-0000-7000-8000-000000000021",
                &format!(r#","parentSession":"{parent}""#),
            ),
            user("hi"),
        ],
    );
    // One branch records a *path* instead (:3199), which is why the parent is resolved
    // rather than trusted verbatim.
    write_session(
        &store,
        "00000000-0000-7000-8000-000000000022",
        &[
            header(
                "00000000-0000-7000-8000-000000000022",
                &format!(
                    r#","parentSession":"{CWD}/../sessions/{BUCKET}/2026-09-01T10-00-00-000Z_{parent}.jsonl""#
                ),
            ),
            user("hi"),
        ],
    );

    let sessions = store.list();
    let child = |id: &str| {
        sessions
            .iter()
            .find(|session| session.id == id)
            .expect("session listed")
    };

    assert_eq!(
        child("00000000-0000-7000-8000-000000000021")
            .parent_id()
            .as_deref(),
        Some(parent)
    );
    assert_eq!(
        child("00000000-0000-7000-8000-000000000022")
            .parent_id()
            .as_deref(),
        Some(parent)
    );
    assert_eq!(child(parent).parent_id(), None);
}

#[test]
fn the_lifecycle_comes_from_the_last_persisted_message() {
    let store = store("lifecycle");

    let cases: Vec<(&str, Vec<String>, Lifecycle)> = vec![
        (
            "00000000-0000-7000-8000-000000000030",
            vec![user("hi"), assistant("stop", TEXT_BLOCK)],
            Lifecycle::Complete,
        ),
        (
            "00000000-0000-7000-8000-000000000031",
            vec![user("hi"), assistant("stop", TOOL_CALL_BLOCK)],
            Lifecycle::Interrupted,
        ),
        (
            "00000000-0000-7000-8000-000000000032",
            vec![user("hi"), assistant("length", TEXT_BLOCK)],
            Lifecycle::Interrupted,
        ),
        (
            "00000000-0000-7000-8000-000000000033",
            vec![user("hi"), assistant("aborted", TEXT_BLOCK)],
            Lifecycle::Aborted,
        ),
        (
            "00000000-0000-7000-8000-000000000034",
            vec![user("hi"), assistant("error", TEXT_BLOCK)],
            Lifecycle::Error,
        ),
        (
            "00000000-0000-7000-8000-000000000035",
            vec![user("hi")],
            Lifecycle::Pending,
        ),
        (
            "00000000-0000-7000-8000-000000000036",
            vec![
                user("hi"),
                assistant("stop", TOOL_CALL_BLOCK),
                tool_result(),
            ],
            Lifecycle::Interrupted,
        ),
    ];

    for (id, lines, _) in &cases {
        let mut body = vec![header(id, "")];
        body.extend(lines.iter().cloned());
        write_session(&store, id, &body);
    }

    let sessions = store.list();
    for (id, _, expected) in &cases {
        let session = sessions
            .iter()
            .find(|session| &session.id == id)
            .expect("session listed");
        assert_eq!(session.lifecycle, *expected, "wrong lifecycle for {id}");
    }
}

#[test]
fn bookkeeping_after_the_last_message_does_not_change_the_lifecycle() {
    let store = store("bookkeeping");

    let id = "00000000-0000-7000-8000-000000000040";
    write_session(
        &store,
        id,
        &[
            header(id, ""),
            user("hi"),
            assistant("stop", TEXT_BLOCK),
            // Renames and exit records are appended after the turn, and none of them is a
            // message: a browser that read only the last *line* would call this unknown.
            r#"{"type":"title_change","title":"Renamed","source":"user"}"#.to_string(),
            r#"{"type":"session_exit","kind":"normal"}"#.to_string(),
        ],
    );

    assert_eq!(store.list()[0].lifecycle, Lifecycle::Complete);
}

#[test]
fn files_that_are_not_sessions_are_skipped_rather_than_fatal() {
    let store = store("skipped");
    let bucket = bucket_of(&store);

    // A truncated write.
    fs::write(
        bucket.join("2026-09-01T10-00-00-000Z_broken.jsonl"),
        "{\"type\":\"sess",
    )
    .expect("broken");
    // Valid JSON, but no header to describe a session.
    fs::write(
        bucket.join("2026-09-01T10-00-00-000Z_headerless.jsonl"),
        "{\"type\":\"message\",\"message\":{\"role\":\"user\"}}\n",
    )
    .expect("headerless");
    // The session's own artifacts directory shares its stem — and a directory named
    // `*.jsonl` must not be read as a file.
    fs::create_dir_all(bucket.join("2026-09-01T10-00-00-000Z_artifacts.jsonl")).expect("artifacts");
    // A backup the engine's EPERM-rewrite path leaves behind is not a session until the
    // engine promotes it.
    fs::write(
        bucket.join("2026-09-01T10-00-00-000Z_backup.jsonl.4242.bak"),
        "{}",
    )
    .expect("backup");

    let good = "00000000-0000-7000-8000-000000000050";
    write_session(
        &store,
        good,
        &[header(good, ""), user("the only real session")],
    );

    let sessions = store.list();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, good);
}

#[test]
fn a_first_message_larger_than_the_window_is_still_recovered() {
    let store = store("wide-first-message");

    // A pasted log as an opening prompt: the 4 KiB prefix ends inside this message, so the
    // line never parses. The engine salvages the text out of the raw bytes; so does this.
    let huge = "log line with a payload ".repeat(400);
    let id = "00000000-0000-7000-8000-000000000060";
    write_session(
        &store,
        id,
        &[header(id, ""), user(&huge), assistant("stop", TEXT_BLOCK)],
    );

    let session = &store.list()[0];
    assert!(session.first_message.starts_with("log line with a payload"));
    assert_eq!(
        session.message_count, 1,
        "only the first message fits the window"
    );
}

#[test]
fn the_tail_window_is_read_from_the_end() {
    let store = store("tail-window");

    // A long turn pushes the interesting part past any prefix, and the tail window starts
    // in the middle of one line: the fragment must be skipped, not parsed.
    let huge = assistant(
        "stop",
        &format!(r#"{{"type":"text","text":"{}"}}"#, "x".repeat(40_000)),
    );
    let id = "00000000-0000-7000-8000-000000000070";
    write_session(
        &store,
        id,
        &[header(id, ""), user("hi"), huge, user("still here")],
    );

    let session = &store.list()[0];
    assert!(session.size > listing::TAIL_BYTES);
    assert_eq!(session.lifecycle, Lifecycle::Pending);
}

#[test]
fn the_list_is_newest_first_and_ties_break_on_creation() {
    let store = store("order");

    let older = "00000000-0000-7000-8000-000000000080";
    let newer = "00000000-0000-7000-8000-000000000081";
    let first = write_session(&store, older, &[header(older, ""), user("older")]);
    let second = write_session(&store, newer, &[header(newer, ""), user("newer")]);

    let now = SystemTime::now();
    fs::File::options()
        .write(true)
        .open(&first)
        .expect("open")
        .set_modified(now - Duration::from_secs(600))
        .expect("mtime");
    fs::File::options()
        .write(true)
        .open(&second)
        .expect("open")
        .set_modified(now)
        .expect("mtime");

    let sessions = store.list();
    assert_eq!(sessions[0].id, newer);
    assert_eq!(sessions[1].id, older);
}

#[test]
fn find_locates_a_session_across_buckets() {
    let store = store("find");
    let id = "00000000-0000-7000-8000-000000000090";
    write_session(&store, id, &[header(id, ""), user("hi")]);

    // A second bucket, so `find` cannot get away with reading only the first.
    let other = store.sessions_root().join("-Projects-Elsewhere");
    fs::create_dir_all(&other).expect("bucket");
    let stray = "00000000-0000-7000-8000-000000000091";
    fs::write(
        other.join(format!("2026-09-02T10-00-00-000Z_{stray}.jsonl")),
        format!(
            "{}\n{}\n",
            header(stray, r#","cwd":"/home/GioViale/Projects/Elsewhere""#),
            user("there")
        ),
    )
    .expect("stray");

    assert_eq!(store.find(id).expect("found").cwd, CWD);
    assert_eq!(
        store.find(stray).expect("found").cwd,
        "/home/GioViale/Projects/Elsewhere"
    );
    assert!(store.find("nope").is_none());
    assert_eq!(
        store
            .list_in(Path::new("/home/GioViale/Projects/Elsewhere"))
            .len(),
        1
    );
    assert_eq!(store.list_in(Path::new(CWD)).len(), 1);
}

/// The catalogue against **this machine's real store**, and in the shape the parity check
/// needs (`tests/` cannot compare two languages by itself; `frontend`'s scripts read the
/// JSON this prints).
///
/// Ignored by default: it reads the user's own sessions, which a unit test must never do.
/// Run it with:
/// `cargo test -p omp-store --test listing -- --ignored --nocapture the_real_store`
#[test]
#[ignore = "reads the user's real store"]
fn the_real_store_reads_as_sessions() {
    let Some(store) = Store::discover() else {
        eprintln!("no agent dir on this machine; nothing to check");
        return;
    };

    let sessions = store.list();
    println!("store: {}", store.agent_dir().display());
    println!("sessions: {}", sessions.len());

    assert!(
        !sessions.is_empty(),
        "a store with an agent dir should have sessions"
    );
    for session in &sessions {
        assert!(!session.id.is_empty(), "every session has an id");
        assert!(
            session.path.is_file(),
            "{} is a file",
            session.path.display()
        );
        assert!(
            matches!(
                session.lifecycle,
                Lifecycle::Complete
                    | Lifecycle::Interrupted
                    | Lifecycle::Aborted
                    | Lifecycle::Error
                    | Lifecycle::Pending
                    | Lifecycle::Unknown
            ),
            "lifecycle parses"
        );
    }

    // The largest session on this machine is ~25 MB: proof that the two-window read is what
    // is happening, not a whole-file parse that would have been unusable in a browser.
    //
    // Its *message* count is small and that is the point, not a defect: a 25 MB session can
    // be three tool results, and the prefix window only ever sees the first few entries.
    // Counting what the window holds is what the engine does too.
    let largest = sessions
        .iter()
        .max_by_key(|session| session.size)
        .expect("a session");
    let busiest = sessions
        .iter()
        .max_by_key(|session| session.message_count)
        .expect("a session");
    println!(
        "largest: {} bytes, {} messages, lifecycle {:?}, name {:?}",
        largest.size,
        largest.message_count,
        largest.lifecycle,
        largest.display_name()
    );
    println!(
        "busiest: {} messages, {} bytes, name {:?}",
        busiest.message_count,
        busiest.size,
        busiest.display_name()
    );
    assert!(
        largest.size > 1_000_000,
        "a real session is megabytes, and still cheap to list"
    );
    assert!(busiest.message_count > 0, "the window counts messages");

    let dump = sessions
        .iter()
        .map(|session| {
            serde_json::json!({
                "id": session.id,
                "status": session.lifecycle.as_str(),
                "messageCount": session.message_count,
                "title": session.title,
                "firstMessage": session.first_message,
                "cwd": session.cwd,
                "parentId": session.parent_id(),
                "size": session.size,
            })
        })
        .collect::<Vec<_>>();
    let path = std::env::temp_dir().join("omp-store-dump.json");
    fs::write(&path, serde_json::to_string_pretty(&dump).expect("encode")).expect("write dump");
    println!("dump: {}", path.display());
}
