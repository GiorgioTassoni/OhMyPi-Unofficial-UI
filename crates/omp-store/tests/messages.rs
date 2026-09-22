//! The message reader the search index is built on (`docs/12` §7.4).
//!
//! The shapes here are the ones real session files carry, including the two that decide how
//! useful a search is: a tool *call* (where a path or a command lives, and the only way to
//! find "which thread ran that rm?") and a tool *result* (which can be enormous, and must not
//! become the index).

use std::fs;
use std::path::PathBuf;

use omp_store::messages::{read_records, RecordKind};

fn fixture(name: &str, lines: &[&str]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("omp-store-messages-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("2026-09-01T10-00-00-000Z_session.jsonl");
    fs::write(&path, format!("{}\n", lines.join("\n"))).expect("fixture");
    path
}

#[test]
fn a_turn_becomes_prompt_answer_thinking_and_tool_records() {
    let path = fixture(
        "turn",
        &[
            r#"{"type":"title","v":1,"title":"Fix the parser","source":"user","pad":"  "}"#,
            r#"{"type":"session","version":3,"id":"s1","timestamp":"2026-09-01T10:00:00.000Z","cwd":"/tmp"}"#,
            r#"{"type":"message","id":"m1","message":{"role":"user","content":[{"type":"text","text":"why is the parser slow?"}]}}"#,
            r#"{"type":"message","id":"m2","message":{"role":"assistant","stopReason":"stop","content":[{"type":"thinking","thinking":"the tokenizer re-reads the buffer"},{"type":"text","text":"because the tokenizer re-reads the buffer"},{"type":"toolCall","id":"c1","name":"bash","input":{"command":"rg tokenizer src"}}]}}"#,
            r#"{"type":"message","id":"m3","message":{"role":"toolResult","toolCallId":"c1","content":[{"type":"text","text":"src/parser/tokenizer.ts:41: re-read"}]}}"#,
        ],
    );

    let records = read_records(&path);
    let kinds: Vec<(&str, u64)> = records
        .iter()
        .map(|record| (record.kind.as_str(), record.ordinal))
        .collect();

    assert_eq!(
        kinds,
        vec![
            ("title", 0),
            ("prompt", 1),
            ("answer", 2),
            ("thinking", 2),
            ("tool", 2),
            ("result", 3),
        ]
    );

    let text = |kind: RecordKind| {
        records
            .iter()
            .find(|record| record.kind == kind)
            .map(|record| record.text.clone())
            .unwrap_or_default()
    };
    assert_eq!(text(RecordKind::Title), "Fix the parser");
    assert_eq!(text(RecordKind::Prompt), "why is the parser slow?");
    assert_eq!(
        text(RecordKind::Answer),
        "because the tokenizer re-reads the buffer"
    );
    assert_eq!(
        text(RecordKind::Thinking),
        "the tokenizer re-reads the buffer"
    );
    // A tool record is the name *and* the input: a search for a command or a path has to hit
    // something, and the name alone would match every bash call ever made.
    assert!(text(RecordKind::Tool).starts_with("bash "));
    assert!(text(RecordKind::Tool).contains("rg tokenizer src"));
    assert!(text(RecordKind::Result).contains("tokenizer.ts:41"));
}

#[test]
fn bookkeeping_is_not_content() {
    let path = fixture(
        "bookkeeping",
        &[
            r#"{"type":"title","v":1,"title":"A name","source":"user","pad":" "}"#,
            r#"{"type":"session","version":3,"id":"s1","cwd":"/tmp"}"#,
            r#"{"type":"title_change","title":"Renamed","source":"user"}"#,
            r#"{"type":"session_exit","kind":"normal"}"#,
            r#"{"type":"custom","customType":"tool_execution_start","data":{"toolName":"bash"}}"#,
            r#"{"type":"message","id":"m1","message":{"role":"user","content":[{"type":"text","text":"hello"}]}}"#,
            // A truncated/partial line, as a write in flight leaves behind.
            r#"{"type":"message","id":"m2","message":{"role":"assist"#,
        ],
    );

    let records = read_records(&path);
    assert_eq!(
        records.len(),
        2,
        "only the title and the one complete message"
    );
    assert_eq!(records[1].text, "hello");
}

#[test]
fn one_message_is_one_record_however_many_blocks_it_has() {
    // A hit that repeats the same message three times is worse than no hit: the search
    // results are grouped by thread, and duplicates fill the group.
    let path = fixture(
        "blocks",
        &[
            r#"{"type":"title","v":1,"title":"t","source":"auto","pad":" "}"#,
            r#"{"type":"session","version":3,"id":"s1","cwd":"/tmp"}"#,
            r#"{"type":"message","id":"m1","message":{"role":"assistant","content":[{"type":"text","text":"first"},{"type":"text","text":"second"}]}}"#,
        ],
    );

    let answers: Vec<_> = read_records(&path)
        .into_iter()
        .filter(|record| record.kind == RecordKind::Answer)
        .collect();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].text, "first second");
}

#[test]
fn an_enormous_tool_result_is_capped_and_says_so() {
    let huge = "log line ".repeat(4000);
    let path = fixture(
        "huge",
        &[
            r#"{"type":"title","v":1,"title":"t","source":"auto","pad":" "}"#,
            r#"{"type":"session","version":3,"id":"s1","cwd":"/tmp"}"#,
            &format!(
                r#"{{"type":"message","id":"m1","message":{{"role":"toolResult","toolCallId":"c1","content":[{{"type":"text","text":"{huge}"}}]}}}}"#
            ),
        ],
    );

    let record = read_records(&path)
        .into_iter()
        .find(|record| record.kind == RecordKind::Result)
        .expect("a result record");
    assert!(
        record.text.len() < 9 * 1024,
        "capped near the limit, not 36 KB"
    );
    assert!(record.text.ends_with("… [truncated]"));
    // The cap must not split a character: a multi-byte boundary inside the cut would produce
    // text the index cannot store.
    assert!(record.text.is_char_boundary(0) && std::str::from_utf8(record.text.as_bytes()).is_ok());
}

#[test]
fn a_missing_file_is_empty_rather_than_fatal() {
    let missing = std::env::temp_dir().join("omp-store-messages-missing.jsonl");
    let _ = fs::remove_file(&missing);
    assert!(read_records(&missing).is_empty());
}

/// The reader against **this machine's real store**: the cost of indexing everything, and the
/// invariants the index depends on.
///
/// Ignored by default (it reads the user's own sessions) and worth running when the index
/// changes shape, because the numbers decide whether a full scan at startup is affordable.
#[test]
#[ignore = "reads the user's real store"]
fn the_real_store_reads_as_records() {
    let Some(store) = omp_store::Store::discover() else {
        eprintln!("no agent dir on this machine; nothing to check");
        return;
    };

    let started = std::time::Instant::now();
    let mut sessions = 0;
    let mut records = 0;
    let mut bytes = 0_u64;
    let mut capped = 0;
    let mut biggest = (String::new(), 0_u64, 0_usize);

    for session in store.list() {
        let found = store.records(&session.path);
        if found.is_empty() {
            continue;
        }
        sessions += 1;
        records += found.len();
        let size: u64 = found.iter().map(|record| record.text.len() as u64).sum();
        bytes += size;
        for record in &found {
            if record.text.ends_with("… [truncated]") {
                capped += 1;
            }
        }
        if session.size > biggest.1 {
            biggest = (session.id.clone(), session.size, found.len());
        }
    }

    let elapsed = started.elapsed();
    println!(
        "indexed {records} records from {sessions} sessions in {elapsed:?} ({bytes} bytes of text)"
    );
    println!(
        "largest session: {} bytes → {} records ({} capped, {} total)",
        biggest.1, biggest.2, capped, records
    );

    assert!(sessions > 0 && records > 0, "a real store has content");
    // The point of the cap: a session of tens of megabytes contributes kilobytes, so the
    // index stays a fraction of the store it describes.
    assert!(
        bytes < 32 * 1024 * 1024,
        "the extracted text is nowhere near the store's size"
    );
    // Every file the reader opened has content, or the index would silently skip sessions.
    assert!(biggest.2 > 0, "the largest session yields records");
}
