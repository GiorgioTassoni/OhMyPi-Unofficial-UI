//! Restore conformance: the engine's two documented refusals, and the guard
//! rails around them.
//!
//! These run against a scripted [`PageSource`], not an engine, because the whole
//! point is to control *when* the session goes busy or stale. The live path is
//! covered separately (`tests/live_session.rs`) — including the real coded error.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use omp_session::restore::{restore_history, PageSource, RestoreError, RestorePolicy};
use omp_session::Message;
use omp_transport::ClientError;
use serde_json::{json, Value};

/// A [`PageSource`] that replays a fixed script and records the cursors it saw.
struct Scripted {
    responses: Mutex<VecDeque<Value>>,
    cursors: Mutex<Vec<Option<String>>>,
}

impl Scripted {
    fn new(responses: Vec<Value>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            cursors: Mutex::new(Vec::new()),
        }
    }

    fn cursors(&self) -> Vec<Option<String>> {
        self.cursors.lock().expect("cursors lock").clone()
    }
}

impl PageSource for Scripted {
    async fn page(&self, cursor: Option<&str>, _limit: u32) -> Result<Value, ClientError> {
        self.cursors
            .lock()
            .expect("cursors lock")
            .push(cursor.map(ToString::to_string));

        Ok(self
            .responses
            .lock()
            .expect("responses lock")
            .pop_front()
            .expect("the script must supply a response for every request"))
    }
}

/// A page carrying one user message per label, with an optional cursor on.
///
/// Labels become both the text and the timestamp, so order across pages is
/// observable rather than inferred.
fn page(labels: &[u64], next: Option<&str>) -> Value {
    let messages: Vec<Value> = labels
        .iter()
        .map(|label| json!({ "role": "user", "content": format!("m{label}"), "timestamp": label }))
        .collect();

    let mut data = json!({ "messages": messages, "totalMessages": labels.len() });
    if let Some(next) = next {
        data["nextCursor"] = json!(next);
    }
    json!({ "type": "response", "command": "get_messages_page", "success": true, "data": data })
}

fn busy() -> Value {
    json!({
        "type": "response",
        "command": "get_messages_page",
        "success": false,
        "error": "Cannot page messages while the session is changing",
        "code": "session_busy",
    })
}

fn stale() -> Value {
    json!({
        "type": "response",
        "command": "get_messages_page",
        "success": false,
        "error": "RPC message cursor is stale",
        "code": "stale_cursor",
    })
}

/// A policy with no real waiting, so refusal tests are instant.
fn policy() -> RestorePolicy {
    RestorePolicy {
        busy_backoff: Duration::from_millis(0),
        max_busy_waits: 3,
        max_restarts: 2,
        ..Default::default()
    }
}

#[tokio::test]
async fn a_single_page_restores_the_whole_history() {
    let source = Scripted::new(vec![page(&[0, 1], None)]);
    let outcome = restore_history(&source, &policy()).await.expect("restores");

    assert_eq!(outcome.messages.len(), 2);
    assert_eq!(outcome.pages, 1);
    assert_eq!(outcome.busy_waits, 0);
    assert_eq!(outcome.restarts, 0);
    assert_eq!(
        source.cursors(),
        vec![None],
        "the first request has no cursor"
    );
}

#[tokio::test]
async fn paging_follows_the_cursor_until_a_page_omits_it() {
    let source = Scripted::new(vec![
        page(&[0], Some("c1")),
        page(&[1], Some("c2")),
        page(&[2], None),
    ]);
    let outcome = restore_history(&source, &policy()).await.expect("restores");

    assert_eq!(outcome.pages, 3);
    assert_eq!(
        outcome
            .messages
            .iter()
            .map(Message::text)
            .collect::<Vec<_>>(),
        vec!["m0", "m1", "m2"],
        "pages must concatenate in engine order"
    );
    assert_eq!(
        source.cursors(),
        vec![None, Some("c1".into()), Some("c2".into())]
    );
}

#[tokio::test]
async fn a_busy_session_is_waited_out_on_the_same_cursor() {
    // The engine checks `session_busy` before paging, so the cursor is still
    // valid: retrying it — rather than restarting — is what keeps the first
    // page's work.
    let source = Scripted::new(vec![busy(), page(&[0], Some("c1")), page(&[1], None)]);
    let outcome = restore_history(&source, &policy()).await.expect("restores");

    assert_eq!(outcome.messages.len(), 2);
    assert_eq!(outcome.pages, 2, "the waited-out page still counts once");
    assert_eq!(outcome.busy_waits, 1);
    assert_eq!(outcome.restarts, 0, "busy is not staleness");
    assert_eq!(
        source.cursors(),
        vec![None, None, Some("c1".into())],
        "the retried request must repeat its cursor"
    );
}

#[tokio::test]
async fn a_persistently_busy_session_gives_up_rather_than_looping() {
    let source = Scripted::new(vec![busy(), busy(), busy(), busy()]);
    let error = restore_history(&source, &policy())
        .await
        .expect_err("must stop");

    match error {
        RestoreError::StillBusy { busy_waits } => assert_eq!(busy_waits, 4),
        other => panic!("expected StillBusy, got {other:?}"),
    }
}

#[tokio::test]
async fn a_stale_cursor_restarts_from_the_top_and_does_not_double_count() {
    // A turn landed between page one and page two, so every cursor is invalid:
    // the partial set must be discarded, not appended to.
    let source = Scripted::new(vec![
        page(&[0, 1], Some("c1")),
        stale(),
        page(&[0, 1, 2], Some("c1")),
        page(&[], None),
    ]);
    let outcome = restore_history(&source, &policy()).await.expect("restores");

    assert_eq!(outcome.restarts, 1);
    assert_eq!(
        outcome
            .messages
            .iter()
            .map(Message::text)
            .collect::<Vec<_>>(),
        vec!["m0", "m1", "m2"],
        "the restart must replace the partial set, not extend it"
    );
    assert_eq!(
        source.cursors(),
        vec![None, Some("c1".into()), None, Some("c1".into())],
        "the restart must drop the cursor"
    );
}

#[tokio::test]
async fn a_history_that_never_settles_reports_staleness() {
    let source = Scripted::new(vec![stale(), stale(), stale()]);
    let error = restore_history(&source, &policy())
        .await
        .expect_err("must stop");

    match error {
        RestoreError::StillStale { restarts } => assert_eq!(restarts, 3),
        other => panic!("expected StillStale, got {other:?}"),
    }
}

#[tokio::test]
async fn an_uncoded_refusal_surfaces_the_engine_text() {
    let source = Scripted::new(vec![json!({
        "type": "response",
        "command": "get_messages_page",
        "success": false,
        "error": "RPC message page limit must be between 1 and 256",
    })]);

    let error = restore_history(&source, &policy())
        .await
        .expect_err("must fail");

    match error {
        RestoreError::Rejected { code, error } => {
            assert_eq!(code, None, "no code means it is not a retryable refusal");
            assert!(error.contains("limit"), "engine text must reach the caller");
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
}

#[tokio::test]
async fn a_successful_page_without_messages_is_treated_as_corrupt() {
    let source = Scripted::new(vec![json!({
        "type": "response",
        "command": "get_messages_page",
        "success": true,
        "data": { "totalMessages": 7 },
    })]);

    let error = restore_history(&source, &policy())
        .await
        .expect_err("must fail");

    match error {
        RestoreError::MalformedPage { reported } => assert_eq!(reported, 7),
        other => panic!("expected MalformedPage, got {other:?}"),
    }
}

#[tokio::test]
async fn a_cursor_that_does_not_advance_cannot_spin_forever() {
    // Defensive: paging appends, so a cursor bug would otherwise grow the
    // transcript without bound instead of failing.
    let source = Scripted::new(vec![page(&[0], Some("c1")), page(&[1], Some("c1"))]);
    let error = restore_history(&source, &policy())
        .await
        .expect_err("must fail");

    match error {
        RestoreError::RepeatedCursor { cursor } => assert_eq!(cursor, "c1"),
        other => panic!("expected RepeatedCursor, got {other:?}"),
    }
}
