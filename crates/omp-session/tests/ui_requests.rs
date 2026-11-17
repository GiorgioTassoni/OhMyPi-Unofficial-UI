//! The pending-dialog store: what may be answered, once, and for how long.

use omp_session::ui_requests::{AnswerError, UiRequests};
use omp_transport::protocol::ui::{
    AskAnswer, AskOption, AskQuestion, Incoming, UiRequest, UiRequestKind, UiResponse,
};
use serde_json::json;

fn select(id: &str, title: &str) -> Incoming {
    Incoming::Request(UiRequest {
        id: id.to_string(),
        kind: UiRequestKind::Select,
        title: title.to_string(),
        message: String::new(),
        options: vec!["Approve".into(), "Deny".into()],
        option_details: Vec::new(),
        questions: Vec::new(),
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
        option_details: Vec::new(),
        questions: Vec::new(),
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

fn ask(id: &str) -> Incoming {
    Incoming::Request(UiRequest {
        id: id.to_string(),
        kind: UiRequestKind::Ask,
        title: String::new(),
        message: String::new(),
        options: Vec::new(),
        option_details: Vec::new(),
        questions: vec![
            AskQuestion {
                id: "q1".to_string(),
                question: "Framework?".to_string(),
                header: None,
                options: vec![
                    AskOption {
                        label: "Vue".to_string(),
                        description: None,
                        preview: None,
                    },
                    AskOption {
                        label: "React".to_string(),
                        description: None,
                        preview: None,
                    },
                ],
                multi: false,
                recommended: None,
            },
            AskQuestion {
                id: "q2".to_string(),
                question: "Tools?".to_string(),
                header: Some("Setup".to_string()),
                options: vec![
                    AskOption {
                        label: "Vite".to_string(),
                        description: None,
                        preview: None,
                    },
                    AskOption {
                        label: "Vitest".to_string(),
                        description: None,
                        preview: None,
                    },
                ],
                multi: true,
                recommended: Some(0),
            },
        ],
        prefill: None,
        placeholder: None,
        timeout_ms: Some(600_000),
    })
}

fn answered(id: &str, selected: &[&str], custom: Option<&str>) -> AskAnswer {
    AskAnswer {
        id: id.to_string(),
        selected_options: selected.iter().map(ToString::to_string).collect(),
        custom_input: custom.map(ToString::to_string),
    }
}

#[test]
fn an_ask_is_answered_once_with_every_question_in_order() {
    let mut pending = UiRequests::default();
    pending.apply(ask("ask_1"));

    let frame = pending
        .answer(
            "ask_1",
            UiResponse::Answers(vec![
                answered("q1", &["Vue"], Some("  ")),
                answered("q2", &["Vite", "Vitest"], None),
            ]),
        )
        .expect("a complete answer is accepted");

    assert_eq!(
        frame,
        json!({
            "type": "extension_ui_response",
            "id": "ask_1",
            "answers": [
                { "id": "q1", "selectedOptions": ["Vue"] },
                { "id": "q2", "selectedOptions": ["Vite", "Vitest"] },
            ],
        })
    );
    assert!(pending.pending().is_empty());
    assert_eq!(
        pending.answer(
            "ask_1",
            UiResponse::Answers(vec![answered("q1", &["Vue"], None), answered("q2", &[], None)])
        ),
        Err(AnswerError::NotPending("ask_1".to_string()))
    );
}

#[test]
fn a_malformed_ask_answer_is_refused_and_the_form_survives() {
    let mut pending = UiRequests::default();
    pending.apply(ask("ask_1"));

    let cases = [
        ("one answer for two questions", vec![answered("q1", &["Vue"], None)]),
        (
            "questions out of order",
            vec![answered("q2", &["Vite"], None), answered("q1", &["Vue"], None)],
        ),
        (
            "a label the question never offered",
            vec![answered("q1", &["Svelte"], None), answered("q2", &[], None)],
        ),
        (
            "the same label twice",
            vec![
                answered("q1", &[], None),
                answered("q2", &["Vite", "Vite"], None),
            ],
        ),
        (
            "two choices on a single-select question",
            vec![answered("q1", &["Vue", "React"], None), answered("q2", &[], None)],
        ),
        (
            "a choice and free text on a single-select question",
            vec![
                answered("q1", &["Vue"], Some("Svelte")),
                answered("q2", &[], None),
            ],
        ),
        (
            "a single-select question with no answer at all",
            vec![answered("q1", &[], None), answered("q2", &[], None)],
        ),
    ];

    for (case, answers) in cases {
        assert_eq!(
            pending.answer("ask_1", UiResponse::Answers(answers)),
            Err(AnswerError::Mismatch {
                kind: UiRequestKind::Ask,
                response: "answers",
            }),
            "{case}"
        );
        assert_eq!(pending.pending().len(), 1, "{case}: the form is still there");
    }

    let frame = pending
        .answer(
            "ask_1",
            UiResponse::Answers(vec![
                answered("q1", &[], Some(" Svelte ")),
                answered("q2", &[], None),
            ]),
        )
        .expect("free text alone answers a single-select question");
    assert_eq!(
        frame["answers"],
        json!([
            { "id": "q1", "selectedOptions": [], "customInput": "Svelte" },
            { "id": "q2", "selectedOptions": [] },
        ])
    );
}

#[test]
fn a_withdrawn_ask_can_no_longer_be_answered() {
    let mut pending = UiRequests::default();
    pending.apply(ask("ask_1"));

    assert!(pending.apply(Incoming::Withdrawn {
        target_id: "ask_1".to_string(),
    }));
    assert_eq!(
        pending.answer(
            "ask_1",
            UiResponse::Answers(vec![answered("q1", &["Vue"], None), answered("q2", &[], None)])
        ),
        Err(AnswerError::NotPending("ask_1".to_string()))
    );
}

/// The engine permits an empty question id, so matching by equality is the whole rule;
/// refusing it would leave the form unanswered.
#[test]
fn an_ask_with_an_empty_question_id_is_answerable() {
    let mut pending = UiRequests::default();
    pending.apply(Incoming::Request(UiRequest {
        id: "ask_1".to_string(),
        kind: UiRequestKind::Ask,
        title: String::new(),
        message: String::new(),
        options: Vec::new(),
        option_details: Vec::new(),
        questions: vec![AskQuestion {
            id: String::new(),
            question: "Framework?".to_string(),
            header: None,
            options: vec![AskOption {
                label: "Vue".to_string(),
                description: None,
                preview: None,
            }],
            multi: false,
            recommended: None,
        }],
        prefill: None,
        placeholder: None,
        timeout_ms: None,
    }));

    let frame = pending
        .answer("ask_1", UiResponse::Answers(vec![answered("", &["Vue"], None)]))
        .expect("an empty id is answered by equality, not refused");

    assert_eq!(
        frame["answers"],
        json!([{ "id": "", "selectedOptions": ["Vue"] }])
    );
}
