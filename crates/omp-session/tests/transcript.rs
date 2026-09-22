//! Transcript conformance: the live path, the restore path, and the requirement
//! that they agree.
//!
//! If they disagreed, resuming a thread would show a different conversation than
//! watching it live — the failure this file exists to prevent.

use omp_session::{Message, Row, ToolOutcome, Transcript};
use omp_transport::events::SessionEvent;
use serde_json::{json, Value};

/// Reduce a scripted event frame the way the client does.
fn apply(transcript: &mut Transcript, event: Value) {
    transcript.apply(&SessionEvent::decode(&event));
}

fn assistant_message(text: &str, stamp: u64, stop_reason: &str) -> Value {
    json!({
        "role": "assistant",
        "content": [{ "type": "text", "text": text }],
        "stopReason": stop_reason,
        "timestamp": stamp,
    })
}

#[test]
fn a_user_turn_produces_one_row_not_two() {
    // `message_start` and `message_end` describe the same message; treating them
    // as two rows would duplicate every prompt in the transcript.
    let mut transcript = Transcript::new();

    apply(
        &mut transcript,
        json!({
            "type": "message_start",
            "message": { "role": "user", "content": "hi", "timestamp": 100 },
        }),
    );
    apply(
        &mut transcript,
        json!({
            "type": "message_end",
            "message": { "role": "user", "content": "hi", "timestamp": 100 },
        }),
    );

    assert_eq!(transcript.len(), 1, "got {:?}", transcript.rows());
    match &transcript.rows()[0] {
        Row::Message(message) => assert_eq!(message.text(), "hi"),
        other => panic!("expected a message row, got {other:?}"),
    }
}

#[test]
fn streaming_text_accumulates_then_yields_to_the_engines_own_message() {
    let mut transcript = Transcript::new();

    apply(
        &mut transcript,
        json!({
            "type": "message_start",
            "message": { "role": "assistant", "content": [], "timestamp": 200 },
        }),
    );

    for delta in ["po", "ng"] {
        apply(
            &mut transcript,
            json!({
                "type": "message_update",
                "message": { "role": "assistant", "content": [], "timestamp": 200 },
                "assistantMessageEvent": { "type": "text_delta", "contentIndex": 0, "delta": delta },
            }),
        );
    }

    // Mid-stream: the deltas are all we have, and the row is marked live.
    match &transcript.rows()[0] {
        Row::Assistant { message, streaming } => {
            assert_eq!(message.text(), "pong");
            assert!(streaming, "the row must be live until message_end");
        }
        other => panic!("expected an assistant row, got {other:?}"),
    }

    // The engine's finished message is authoritative — including here, where it
    // disagrees with what the deltas spelled.
    apply(
        &mut transcript,
        json!({
            "type": "message_end",
            "message": assistant_message("pong.", 200, "stop"),
        }),
    );

    match &transcript.rows()[0] {
        Row::Assistant { message, streaming } => {
            assert_eq!(message.text(), "pong.");
            assert!(!streaming);
        }
        other => panic!("expected an assistant row, got {other:?}"),
    }
}

#[test]
fn interleaved_blocks_keep_their_engine_order() {
    // A turn can think, speak, then call a tool; the block indexes are what order
    // the render, not arrival order.
    let mut transcript = Transcript::new();

    apply(
        &mut transcript,
        json!({
            "type": "message_start",
            "message": { "role": "assistant", "content": [], "timestamp": 300 },
        }),
    );

    for (index, delta) in [(1u32, "text"), (0u32, "think")] {
        let event = if index == 0 {
            json!({ "type": "thinking_delta", "contentIndex": index, "delta": delta })
        } else {
            json!({ "type": "text_delta", "contentIndex": index, "delta": delta })
        };
        apply(
            &mut transcript,
            json!({
                "type": "message_update",
                "message": { "role": "assistant", "content": [], "timestamp": 300 },
                "assistantMessageEvent": event,
            }),
        );
    }

    let Row::Assistant { message, .. } = &transcript.rows()[0] else {
        panic!("expected an assistant row");
    };
    assert_eq!(message.blocks.len(), 2);
    assert!(
        matches!(&message.blocks[0], omp_session::ContentBlock::Thinking { thinking } if thinking == "think"),
        "contentIndex 0 must stay first: {:?}",
        message.blocks
    );
    assert_eq!(message.text(), "text", "text() reads only text blocks");
}

#[test]
fn a_tool_call_becomes_one_card_across_its_whole_lifecycle() {
    let mut transcript = Transcript::new();

    apply(
        &mut transcript,
        json!({
            "type": "tool_execution_start",
            "toolCallId": "c1",
            "toolName": "bash",
            "args": { "command": "ls" },
            "intent": "list files",
        }),
    );
    apply(
        &mut transcript,
        json!({
            "type": "tool_stream_update",
            "toolCallId": "c1",
            "toolName": "bash",
            "update": { "chunk": "a\n" },
        }),
    );
    apply(
        &mut transcript,
        json!({
            "type": "tool_execution_update",
            "toolCallId": "c1",
            "toolName": "bash",
            "args": {},
            "partialResult": { "stdout": "a\n" },
        }),
    );
    apply(
        &mut transcript,
        json!({
            "type": "tool_execution_end",
            "toolCallId": "c1",
            "toolName": "bash",
            "result": { "content": [{ "type": "text", "text": "a\n" }], "details": { "code": 0 } },
            "isError": false,
        }),
    );

    assert_eq!(transcript.len(), 1, "one card, not four rows");
    let Row::Tool(card) = &transcript.rows()[0] else {
        panic!("expected a tool card");
    };
    assert_eq!(card.tool_name, "bash");
    assert_eq!(card.args["command"], "ls");
    assert_eq!(card.intent.as_deref(), Some("list files"));
    assert_eq!(card.streamed.len(), 1, "streamed output is kept in order");
    assert!(card.partial.is_some(), "the latest snapshot is kept");
    assert!(card.is_finished());

    match &card.outcome {
        ToolOutcome::Done(result) => {
            assert!(!result.is_error);
            assert_eq!(result.content[0]["text"], "a\n");
            assert_eq!(result.details.as_ref().expect("details")["code"], 0);
        }
        other => panic!("expected a finished card, got {other:?}"),
    }
}

#[test]
fn a_tool_result_message_alone_still_produces_a_card() {
    // The restore path sees results without ever seeing the tool events.
    let mut transcript = Transcript::new();
    apply(
        &mut transcript,
        json!({
            "type": "message_end",
            "message": {
                "role": "toolResult",
                "toolCallId": "c9",
                "toolName": "read",
                "content": [{ "type": "text", "text": "file body" }],
                "isError": true,
                "timestamp": 400,
            },
        }),
    );

    let Row::Tool(card) = &transcript.rows()[0] else {
        panic!("expected a tool card");
    };
    assert_eq!(card.tool_call_id, "c9");
    match &card.outcome {
        ToolOutcome::Done(result) => {
            assert!(result.is_error);
            assert_eq!(result.content[0]["text"], "file body");
        }
        other => panic!("expected a finished card, got {other:?}"),
    }
}

#[test]
fn restore_pairs_a_tool_call_with_the_result_that_follows_it() {
    let messages = vec![
        Message::decode(&json!({
            "role": "user", "content": "list the files", "timestamp": 1,
        })),
        Message::decode(&json!({
            "role": "assistant",
            "content": [
                { "type": "text", "text": "Listing." },
                { "type": "toolCall", "id": "c1", "name": "bash", "arguments": { "command": "ls" } },
            ],
            "stopReason": "toolUse",
            "timestamp": 2,
        })),
        Message::decode(&json!({
            "role": "toolResult", "toolCallId": "c1", "toolName": "bash",
            "content": [{ "type": "text", "text": "a.txt" }], "isError": false, "timestamp": 3,
        })),
    ];

    let mut transcript = Transcript::new();
    transcript.restore(&messages);

    assert_eq!(transcript.len(), 3, "got {:?}", transcript.rows());
    let Row::Tool(card) = &transcript.rows()[2] else {
        panic!("the result must attach to a card, not stand alone");
    };
    assert_eq!(card.tool_call_id, "c1");
    assert_eq!(card.args["command"], "ls", "arguments come from the call");
    match &card.outcome {
        ToolOutcome::Done(result) => assert_eq!(result.content[0]["text"], "a.txt"),
        other => panic!("expected a finished card, got {other:?}"),
    }
}

#[test]
fn the_live_and_restore_paths_agree_on_a_text_turn() {
    // The property the whole module exists to hold.
    let mut live = Transcript::new();
    apply(
        &mut live,
        json!({
            "type": "message_start",
            "message": { "role": "user", "content": "pong?", "timestamp": 1 },
        }),
    );
    apply(
        &mut live,
        json!({
            "type": "message_end",
            "message": { "role": "user", "content": "pong?", "timestamp": 1 },
        }),
    );
    apply(
        &mut live,
        json!({
            "type": "message_start",
            "message": { "role": "assistant", "content": [], "timestamp": 2 },
        }),
    );
    apply(
        &mut live,
        json!({
            "type": "message_update",
            "message": { "role": "assistant", "content": [], "timestamp": 2 },
            "assistantMessageEvent": { "type": "text_delta", "contentIndex": 0, "delta": "pong" },
        }),
    );
    apply(
        &mut live,
        json!({ "type": "message_end", "message": assistant_message("pong", 2, "stop") }),
    );

    let restored_messages = vec![
        Message::decode(&json!({ "role": "user", "content": "pong?", "timestamp": 1 })),
        Message::decode(&assistant_message("pong", 2, "stop")),
    ];
    let mut restored = Transcript::new();
    restored.restore(&restored_messages);

    assert_eq!(live.rows().len(), restored.rows().len());
    for (live_row, restored_row) in live.rows().iter().zip(restored.rows()) {
        match (live_row, restored_row) {
            (Row::Message(a), Row::Message(b)) => {
                assert_eq!(a.text(), b.text());
                assert_eq!(a.role(), b.role());
            }
            (Row::Assistant { message: a, .. }, Row::Assistant { message: b, .. }) => {
                assert_eq!(a.text(), b.text());
                assert_eq!(a.blocks, b.blocks);
            }
            (a, b) => panic!("row kinds diverged: {a:?} vs {b:?}"),
        }
    }
}

#[test]
fn maintenance_activity_becomes_a_visible_chip() {
    let mut transcript = Transcript::new();

    apply(
        &mut transcript,
        json!({
            "type": "auto_retry_start",
            "attempt": 2,
            "maxAttempts": 5,
            "delayMs": 4000,
            "errorMessage": "429",
        }),
    );
    apply(
        &mut transcript,
        json!({ "type": "auto_compaction_end", "action": "context-full", "aborted": true, "willRetry": false }),
    );
    apply(
        &mut transcript,
        json!({ "type": "notice", "level": "error", "message": "login refused", "source": "auth" }),
    );

    assert_eq!(transcript.len(), 3);
    match &transcript.rows()[0] {
        Row::Notice {
            level, kind, text, ..
        } => {
            assert_eq!(level, "warning");
            assert_eq!(kind, "auto_retry_start");
            assert!(
                text.contains("2/5"),
                "the attempt count must be visible: {text}"
            );
        }
        other => panic!("expected a notice, got {other:?}"),
    }
    match &transcript.rows()[1] {
        Row::Notice { level, kind, .. } => {
            assert_eq!(level, "warning", "an aborted compaction is not routine");
            assert_eq!(kind, "auto_compaction_end");
        }
        other => panic!("expected a notice, got {other:?}"),
    }
    match &transcript.rows()[2] {
        Row::Notice {
            level,
            source,
            text,
            ..
        } => {
            assert_eq!(level, "error");
            assert_eq!(text, "login refused", "engine text is used verbatim");
            assert_eq!(source.as_deref(), Some("auth"));
        }
        other => panic!("expected a notice, got {other:?}"),
    }
}

#[test]
fn an_event_the_app_cannot_read_is_shown_rather_than_swallowed() {
    // docs/14 §7.6: no known silent-failure path. A decode gap is a failure the
    // user must be able to see.
    let mut transcript = Transcript::new();

    apply(
        &mut transcript,
        json!({ "type": "reasoning_budget_changed", "budget": 3 }),
    );
    apply(
        &mut transcript,
        json!({ "type": "notice", "detail": "field renamed" }),
    );

    assert_eq!(transcript.len(), 2);
    match &transcript.rows()[0] {
        Row::Notice {
            level, kind, text, ..
        } => {
            assert_eq!(level, "warning");
            assert_eq!(kind, "unknown_event");
            assert!(
                text.contains("reasoning_budget_changed"),
                "name the type: {text}"
            );
        }
        other => panic!("expected a notice, got {other:?}"),
    }
    match &transcript.rows()[1] {
        Row::Notice {
            level, kind, text, ..
        } => {
            assert_eq!(
                level, "error",
                "a payload mismatch is our bug, not a surprise"
            );
            assert_eq!(kind, "malformed_event");
            assert!(text.contains("notice"), "name the event: {text}");
        }
        other => panic!("expected a notice, got {other:?}"),
    }
}

#[test]
fn a_turn_that_starts_mid_stream_is_adopted_rather_than_dropped() {
    // A client that subscribes after `message_start` still has to render the turn.
    let mut transcript = Transcript::new();

    apply(
        &mut transcript,
        json!({
            "type": "message_update",
            "message": { "role": "assistant", "content": [], "timestamp": 500 },
            "assistantMessageEvent": { "type": "text_delta", "contentIndex": 0, "delta": "late" },
        }),
    );

    assert_eq!(transcript.len(), 1);
    match &transcript.rows()[0] {
        Row::Assistant { message, streaming } => {
            assert_eq!(message.text(), "late");
            assert!(streaming);
        }
        other => panic!("expected an assistant row, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Change tracking: what a view has to re-render.
//
// The conversation view sends everything from the first changed row onward, so the
// index has to be the *earliest* change, not the last one written. Getting that
// wrong is invisible until a tool card settles behind rows that came after it —
// which is exactly the case pinned below.
// ---------------------------------------------------------------------------

#[test]
fn a_change_is_reported_once_and_then_forgotten() {
    let mut transcript = Transcript::new();
    assert_eq!(transcript.take_dirty(), None, "nothing has happened yet");

    apply(
        &mut transcript,
        json!({
            "type": "message_start",
            "message": { "role": "user", "content": "hi", "timestamp": 1 },
        }),
    );

    assert_eq!(
        transcript.take_dirty(),
        Some(0),
        "the appended row is the change"
    );
    assert_eq!(
        transcript.take_dirty(),
        None,
        "a view that has been told once does not need telling again"
    );
}

#[test]
fn a_delta_reports_the_row_it_lands_on() {
    let mut transcript = Transcript::new();

    apply(
        &mut transcript,
        json!({
            "type": "message_start",
            "message": { "role": "user", "content": "first", "timestamp": 1 },
        }),
    );
    transcript.take_dirty();

    apply(
        &mut transcript,
        json!({
            "type": "message_start",
            "message": { "role": "assistant", "content": [], "timestamp": 2 },
        }),
    );
    assert_eq!(
        transcript.take_dirty(),
        Some(1),
        "a fresh row, at its own index"
    );

    apply(
        &mut transcript,
        json!({
            "type": "message_update",
            "message": { "role": "assistant", "content": [], "timestamp": 2 },
            "assistantMessageEvent": { "type": "text_delta", "contentIndex": 0, "delta": "a" },
        }),
    );
    assert_eq!(
        transcript.take_dirty(),
        Some(1),
        "streaming rewrites the assistant row, not the whole conversation"
    );
}

#[test]
fn a_card_settling_behind_later_rows_reports_its_own_index() {
    // The case a "send the tail" shortcut silently gets wrong: two calls in one
    // turn, the first settling after the second has already appeared.
    let mut transcript = Transcript::new();

    apply(
        &mut transcript,
        json!({
            "type": "tool_execution_start",
            "toolCallId": "call_1",
            "toolName": "read",
            "args": {},
        }),
    );
    apply(
        &mut transcript,
        json!({
            "type": "tool_execution_start",
            "toolCallId": "call_2",
            "toolName": "bash",
            "args": {},
        }),
    );
    assert_eq!(transcript.len(), 2);
    transcript.take_dirty();

    apply(
        &mut transcript,
        json!({
            "type": "tool_execution_end",
            "toolCallId": "call_1",
            "toolName": "read",
            "result": { "content": [{ "type": "text", "text": "done" }] },
        }),
    );

    assert_eq!(
        transcript.take_dirty(),
        Some(0),
        "the first card changed, so everything from it onward must be re-read"
    );
    match &transcript.rows()[0] {
        Row::Tool(card) => assert!(matches!(card.outcome, ToolOutcome::Done(_))),
        other => panic!("expected a tool card, got {other:?}"),
    }
}

#[test]
fn emptying_the_transcript_reports_index_zero() {
    // A view has to be told to drop its rows, or switching threads would leave the
    // previous conversation on screen with the new one appended below it.
    let mut transcript = Transcript::new();
    apply(
        &mut transcript,
        json!({
            "type": "message_start",
            "message": { "role": "user", "content": "hi", "timestamp": 1 },
        }),
    );
    transcript.take_dirty();

    transcript.clear();
    assert_eq!(transcript.take_dirty(), Some(0));
}

#[test]
fn a_card_keeps_the_engines_details_on_both_paths() {
    // The `edit` card renders its unified diff out of `details`, so a path that
    // dropped details would show a bare path for a live edit and a diff for the
    // same edit after a resume. Both paths are asserted here for that reason.
    let diff = "--- a/f.ts\n+++ b/f.ts\n@@ -1 +1 @@\n-old\n+new\n";

    let mut live = Transcript::new();
    apply(
        &mut live,
        json!({
            "type": "tool_execution_start",
            "toolCallId": "c1", "toolName": "edit", "args": { "path": "f.ts" },
        }),
    );
    apply(
        &mut live,
        json!({
            "type": "tool_execution_end",
            "toolCallId": "c1", "toolName": "edit", "isError": false,
            "result": {
                "content": [{ "type": "text", "text": "Edited f.ts" }],
                "details": { "diff": diff, "path": "/w/f.ts" },
            },
        }),
    );

    let mut restored = Transcript::new();
    restored.restore(&[Message::decode(&json!({
        "role": "toolResult",
        "toolCallId": "c1",
        "toolName": "edit",
        "content": [{ "type": "text", "text": "Edited f.ts" }],
        "details": { "diff": diff, "path": "/w/f.ts" },
        "isError": false,
        "timestamp": 5,
    }))]);

    for (path, transcript) in [("live", &live), ("restore", &restored)] {
        let Row::Tool(card) = &transcript.rows()[0] else {
            panic!("{path}: expected a tool card");
        };
        let ToolOutcome::Done(result) = &card.outcome else {
            panic!("{path}: expected a finished card");
        };
        assert_eq!(
            result
                .details
                .as_ref()
                .and_then(|details| details.get("diff")),
            Some(&json!(diff)),
            "{path}: the card's diff must survive"
        );
    }
}

#[test]
fn a_streamed_image_block_keeps_its_bytes() {
    // Assistant-side images arrive as an `image_end` delta whose `content` is the
    // finished block. It has to decode like any other image block, or a generated
    // image would render empty while the same image in a stored message rendered.
    let mut transcript = Transcript::new();
    apply(
        &mut transcript,
        json!({
            "type": "message_start",
            "message": { "role": "assistant", "content": [], "timestamp": 700 },
        }),
    );
    apply(
        &mut transcript,
        json!({
            "type": "message_update",
            "message": { "role": "assistant", "content": [], "timestamp": 700 },
            "assistantMessageEvent": {
                "type": "image_end",
                "contentIndex": 0,
                "content": { "type": "image", "data": "AAAA", "mimeType": "image/png" },
            },
        }),
    );

    let Row::Assistant { message, .. } = &transcript.rows()[0] else {
        panic!("expected an assistant row");
    };
    assert_eq!(
        message.images().collect::<Vec<_>>(),
        vec![("image/png", "AAAA")]
    );
}
