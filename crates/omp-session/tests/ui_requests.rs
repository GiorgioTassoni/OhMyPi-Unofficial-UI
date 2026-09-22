//! The pending-dialog store: what may be answered, once, and for how long.

use omp_session::ui_requests::{AnswerError, UiRequests};
use omp_transport::protocol::ui::{Incoming, UiRequest, UiRequestKind, UiResponse};
use serde_json::json;

fn select(id: &str, title: &str) -> Incoming {
    Incoming::Request(UiRequest {
        id: id.to_string(),
        kind: UiRequestKind::Select,
        title: title.to_string(),
        message: String::new(),
        options: vec!["Approve".into(), "Deny".into()],
        prefill: None,
        placeholder: None,
        timeout_ms: None,
    })
}

fn confirm(id: &str) -> Incoming {
    Incoming::Request(UiRequest {
        id: id.to_string(),
        kind: UiRequestKind::Confirm,
        title: "Continue?".into(),
        message: "This will restart the server.".into(),
        options: Vec::new(),
        prefill: None,
        placeholder: None,
        timeout_ms: None,
    })
}

/// The invariant the whole safety boundary rests on: one dialog, one answer. A
/// second write for the same id would be matched against a later dialog.
#[test]
fn a_dialog_is_answerable_exactly_once() {
    let mut pending = UiRequests::default();
    assert!(pending.apply(select("ui_1", "Allow tool: bash")));

    let frame = pending
        .answer("ui_1", UiResponse::Value("Approve".into()))
        .expect("the first answer is accepted");
    assert_eq!(
        frame,
        json!({ "type": "extension_ui_response", "id": "ui_1", "value": "Approve" })
    );

    assert_eq!(
        pending.answer("ui_1", UiResponse::Value("Approve".into())),
        Err(AnswerError::NotPending("ui_1".to_string()))
    );
    assert!(pending.pending().is_empty());
}

/// The engine resolves a dialog itself when its deadline fires, and says so with
/// `cancel`. Answering afterwards is the failure mode this guards.
#[test]
fn a_withdrawn_dialog_can_no_longer_be_answered() {
    let mut pending = UiRequests::default();
    pending.apply(select("ui_1", "Allow tool: bash"));

    assert!(pending.apply(Incoming::Withdrawn {
        target_id: "ui_1".to_string(),
    }));
    assert!(pending.pending().is_empty());
    assert_eq!(
        pending.answer("ui_1", UiResponse::Value("Deny".into())),
        Err(AnswerError::NotPending("ui_1".to_string()))
    );
}

/// A replayed dialog must replace its earlier copy, not sit beside it: otherwise
/// the user answers the stale one and the engine keeps waiting.
#[test]
fn a_replayed_dialog_replaces_its_earlier_copy() {
    let mut pending = UiRequests::default();
    pending.apply(select("ui_1", "Allow tool: bash"));
    pending.apply(select("ui_1", "Allow tool: read"));

    assert_eq!(pending.pending().len(), 1);
    assert_eq!(pending.pending()[0].title, "Allow tool: read");
}

/// A refused answer must leave the dialog answerable — the user still has to get
/// past it, so dropping the entry would strand the run instead of fixing it.
#[test]
fn a_mismatched_answer_is_refused_and_the_dialog_survives() {
    let mut pending = UiRequests::default();
    pending.apply(confirm("ui_2"));

    assert_eq!(
        pending.answer("ui_2", UiResponse::Value("yes".into())),
        Err(AnswerError::Mismatch {
            kind: UiRequestKind::Confirm,
            response: "value",
        })
    );
    assert_eq!(
        pending.pending().len(),
        1,
        "the dialog is still there to answer"
    );

    let frame = pending
        .answer("ui_2", UiResponse::Confirmed(true))
        .expect("the fitting answer is accepted");
    assert_eq!(frame["confirmed"], json!(true));
}

/// The change signal drives whether the UI is told to re-read. A request that
/// reported "no change" would be a dialog the user never sees.
#[test]
fn applying_reports_whether_the_visible_set_changed() {
    let mut pending = UiRequests::default();

    assert!(pending.apply(select("ui_1", "Allow tool: bash")));
    assert!(
        !pending.apply(Incoming::Withdrawn {
            target_id: "never-seen".to_string(),
        }),
        "withdrawing an unknown dialog changes nothing"
    );
    assert!(pending.clear());
    assert!(!pending.clear(), "clearing an empty set changes nothing");
}
