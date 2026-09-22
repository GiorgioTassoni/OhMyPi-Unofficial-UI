//! Event-decoding conformance.
//!
//! The sample table below is one realistic frame per event kind, transcribed
//! from the upstream union (`session/agent-session-events.ts` and
//! `pi-agent-core`'s `AgentEvent` at v18.2.6). It is the executable form of the
//! contract: if a kind's `snake_case` variant name, its tag, or a payload field
//! drifts, a test here fails rather than a user seeing a blank card.
//!
//! Two properties are asserted throughout:
//!
//! * **totality** — every kind this build claims to know decodes to its own
//!   variant, so the sample table and `KNOWN_EVENT_KINDS` cannot drift apart;
//! * **tolerance** — an unknown kind, and a known kind with a payload this build
//!   cannot read, both decode to a recoverable value carrying the original
//!   frame. The engine adds event types between releases; the app must outlive a
//!   sidecar bump.

use omp_transport::events::{KNOWN_DELTA_KINDS, KNOWN_EVENT_KINDS};
use omp_transport::{MessageDelta, SessionEvent};
use serde_json::{json, Value};

/// One sample frame per session event kind.
fn event_samples() -> Vec<(&'static str, Value)> {
    // Payload shapes are deliberately minimal-but-valid: fields the decoder
    // models must be present and correctly typed, since a mismatch is exactly
    // what these tests exist to catch.
    vec![
        ("agent_start", json!({ "type": "agent_start" })),
        (
            "agent_end",
            json!({
                "type": "agent_end",
                "messages": [{ "role": "assistant", "content": [] }],
                "isTerminal": true,
            }),
        ),
        ("turn_start", json!({ "type": "turn_start" })),
        (
            "turn_end",
            json!({ "type": "turn_end", "message": { "role": "assistant" }, "toolResults": [] }),
        ),
        (
            "message_start",
            json!({ "type": "message_start", "message": { "role": "user", "content": "hi" } }),
        ),
        (
            "message_update",
            json!({
                "type": "message_update",
                "message": { "role": "assistant", "content": [] },
                "assistantMessageEvent": {
                    "type": "text_delta",
                    "contentIndex": 0,
                    "delta": "wor",
                    "partial": { "role": "assistant" },
                },
            }),
        ),
        (
            "message_end",
            json!({ "type": "message_end", "message": { "role": "assistant", "content": [] } }),
        ),
        (
            "tool_execution_start",
            json!({
                "type": "tool_execution_start",
                "toolCallId": "call_1",
                "toolName": "bash",
                "args": { "command": "ls" },
                "intent": "list files",
            }),
        ),
        (
            "tool_execution_update",
            json!({
                "type": "tool_execution_update",
                "toolCallId": "call_1",
                "toolName": "bash",
                "args": { "command": "ls" },
                "partialResult": { "stdout": "a\n" },
            }),
        ),
        (
            "tool_stream_update",
            json!({
                "type": "tool_stream_update",
                "toolCallId": "call_1",
                "toolName": "edit",
                "update": { "line": 12 },
            }),
        ),
        (
            "tool_execution_end",
            json!({
                "type": "tool_execution_end",
                "toolCallId": "call_1",
                "toolName": "bash",
                "result": { "code": 0 },
                "isError": false,
            }),
        ),
        (
            "auto_compaction_start",
            json!({ "type": "auto_compaction_start", "reason": "threshold", "action": "context-full" }),
        ),
        (
            "auto_compaction_end",
            json!({
                "type": "auto_compaction_end",
                "action": "context-full",
                "result": { "tokensBefore": 190000, "tokensAfter": 40000 },
                "aborted": false,
                "willRetry": true,
            }),
        ),
        (
            "auto_retry_start",
            json!({
                "type": "auto_retry_start",
                "attempt": 1,
                "maxAttempts": 3,
                "delayMs": 4000,
                "errorMessage": "overloaded",
            }),
        ),
        (
            "auto_retry_end",
            json!({ "type": "auto_retry_end", "success": true, "attempt": 1 }),
        ),
        (
            "retry_fallback_applied",
            json!({ "type": "retry_fallback_applied", "from": "opus", "to": "sonnet", "role": "main" }),
        ),
        (
            "retry_fallback_succeeded",
            json!({ "type": "retry_fallback_succeeded", "model": "sonnet", "role": "main" }),
        ),
        ("model_changed", json!({ "type": "model_changed" })),
        (
            "thinking_level_changed",
            json!({
                "type": "thinking_level_changed",
                "thinkingLevel": "high",
                "configured": "auto",
                "resolved": "high",
            }),
        ),
        (
            "config_warnings_changed",
            json!({ "type": "config_warnings_changed" }),
        ),
        (
            "advisor_cost_changed",
            json!({ "type": "advisor_cost_changed" }),
        ),
        ("advisor_yielded", json!({ "type": "advisor_yielded" })),
        (
            "ttsr_triggered",
            json!({ "type": "ttsr_triggered", "rules": [{ "name": "no-force-push" }] }),
        ),
        (
            "todo_reminder",
            json!({
                "type": "todo_reminder",
                "todos": [{ "content": "ship it", "status": "pending" }],
                "attempt": 1,
                "maxAttempts": 3,
            }),
        ),
        ("todo_auto_clear", json!({ "type": "todo_auto_clear" })),
        (
            "irc_message",
            json!({ "type": "irc_message", "message": { "from": "Scout", "text": "done" } }),
        ),
        (
            "notice",
            json!({ "type": "notice", "level": "warning", "message": "context is filling", "source": "compaction" }),
        ),
        (
            "goal_updated",
            json!({ "type": "goal_updated", "goal": null, "state": { "active": false } }),
        ),
    ]
}

/// One sample frame per `assistantMessageEvent` kind.
fn delta_samples() -> Vec<(&'static str, Value)> {
    vec![
        ("start", json!({ "type": "start", "partial": {} })),
        (
            "text_start",
            json!({ "type": "text_start", "contentIndex": 0, "partial": {} }),
        ),
        (
            "text_delta",
            json!({ "type": "text_delta", "contentIndex": 0, "delta": "hi", "partial": {} }),
        ),
        (
            "text_end",
            json!({ "type": "text_end", "contentIndex": 0, "content": "hi", "partial": {} }),
        ),
        (
            "thinking_start",
            json!({ "type": "thinking_start", "contentIndex": 1, "partial": {} }),
        ),
        (
            "thinking_delta",
            json!({ "type": "thinking_delta", "contentIndex": 1, "delta": "hmm", "partial": {} }),
        ),
        (
            "thinking_end",
            json!({ "type": "thinking_end", "contentIndex": 1, "content": "hmm", "partial": {} }),
        ),
        (
            "image_end",
            json!({
                "type": "image_end",
                "contentIndex": 2,
                "content": { "type": "image", "mimeType": "image/png" },
                "partial": {},
            }),
        ),
        (
            "toolcall_start",
            json!({ "type": "toolcall_start", "contentIndex": 3, "partial": {} }),
        ),
        (
            "toolcall_delta",
            json!({ "type": "toolcall_delta", "contentIndex": 3, "delta": "{\"a\"", "partial": {} }),
        ),
        (
            "toolcall_end",
            json!({
                "type": "toolcall_end",
                "contentIndex": 3,
                "toolCall": { "id": "call_1", "name": "bash" },
                "partial": {},
            }),
        ),
        (
            "done",
            json!({
                "type": "done",
                "reason": "toolUse",
                "message": { "role": "assistant" },
            }),
        ),
        (
            "error",
            json!({
                "type": "error",
                "reason": "aborted",
                "error": { "role": "assistant" },
            }),
        ),
    ]
}

#[test]
fn every_known_event_kind_decodes_to_its_own_variant() {
    for (kind, sample) in event_samples() {
        let event = SessionEvent::decode(&sample);
        assert_eq!(
            event.kind(),
            kind,
            "sample for `{kind}` decoded as {event:?}"
        );
        assert!(
            !matches!(
                event,
                SessionEvent::Unknown { .. } | SessionEvent::Malformed { .. }
            ),
            "`{kind}` should be understood at this pinned version, got {event:?}"
        );
    }
}

#[test]
fn the_sample_table_covers_exactly_the_known_kinds() {
    let mut sampled: Vec<&str> = event_samples().iter().map(|(kind, _)| *kind).collect();
    sampled.sort_unstable();

    let mut known: Vec<&str> = KNOWN_EVENT_KINDS.to_vec();
    known.sort_unstable();

    assert_eq!(
        sampled, known,
        "the sample table and KNOWN_EVENT_KINDS must list the same event types"
    );
}

#[test]
fn every_known_delta_kind_decodes_to_its_own_variant() {
    for (kind, sample) in delta_samples() {
        let delta = MessageDelta::decode(&sample);
        assert_eq!(
            delta.kind(),
            kind,
            "sample for `{kind}` decoded as {delta:?}"
        );
        assert!(
            !matches!(
                delta,
                MessageDelta::Unknown { .. } | MessageDelta::Malformed { .. }
            ),
            "`{kind}` should be understood at this pinned version, got {delta:?}"
        );
    }
}

#[test]
fn the_delta_sample_table_covers_exactly_the_known_kinds() {
    let mut sampled: Vec<&str> = delta_samples().iter().map(|(kind, _)| *kind).collect();
    sampled.sort_unstable();

    let mut known: Vec<&str> = KNOWN_DELTA_KINDS.to_vec();
    known.sort_unstable();

    assert_eq!(sampled, known);
}

#[test]
fn an_unknown_event_is_preserved_rather_than_dropped() {
    // The shape a future release might add: a new kind we have never heard of.
    let frame = json!({ "type": "reasoning_budget_changed", "budget": 9000 });
    let event = SessionEvent::decode(&frame);

    match event {
        SessionEvent::Unknown { kind, raw } => {
            assert_eq!(kind, "reasoning_budget_changed");
            assert_eq!(raw, frame, "the original frame must survive verbatim");
        }
        other => panic!("expected Unknown, got {other:?}"),
    }
}

#[test]
fn a_known_event_with_an_unreadable_payload_is_reported_not_swallowed() {
    // `notice` requires `level` and `message`; this one has neither, which is
    // what upstream drift on a known kind would look like.
    let frame = json!({ "type": "notice", "detail": "renamed field" });
    let event = SessionEvent::decode(&frame);

    match event {
        SessionEvent::Malformed { kind, reason, raw } => {
            assert_eq!(kind, "notice");
            assert!(
                !reason.is_empty(),
                "the reason must name what did not match"
            );
            assert_eq!(raw, frame);
        }
        other => panic!("expected Malformed, got {other:?}"),
    }
}

#[test]
fn an_event_without_a_type_is_malformed_rather_than_a_panic() {
    let event = SessionEvent::decode(&json!({ "messages": [] }));
    match event {
        SessionEvent::Malformed { kind, .. } => assert!(kind.is_empty()),
        other => panic!("expected Malformed, got {other:?}"),
    }
}

#[test]
fn an_unknown_delta_does_not_poison_the_message_update() {
    // A future delta kind must not turn a whole message update into a failure:
    // the outer event is still usable, and the delta says what it could not read.
    let frame = json!({
        "type": "message_update",
        "message": { "role": "assistant" },
        "assistantMessageEvent": { "type": "citation_delta", "contentIndex": 0 },
    });

    let SessionEvent::MessageUpdate(update) = SessionEvent::decode(&frame) else {
        panic!("expected a decoded message_update");
    };
    assert!(update.message.is_object(), "the message survives the delta");

    match update.assistant_message_event {
        MessageDelta::Unknown { kind, .. } => assert_eq!(kind, "citation_delta"),
        other => panic!("expected an Unknown delta, got {other:?}"),
    }
}

#[test]
fn the_completion_rule_treats_an_absent_isterminal_as_terminal() {
    let terminal_absent = SessionEvent::decode(&json!({ "type": "agent_end", "messages": [] }));
    let terminal_true =
        SessionEvent::decode(&json!({ "type": "agent_end", "messages": [], "isTerminal": true }));
    let terminal_false =
        SessionEvent::decode(&json!({ "type": "agent_end", "messages": [], "isTerminal": false }));

    assert!(
        terminal_absent.ends_run(),
        "absent is terminal (older runtimes)"
    );
    assert!(terminal_true.ends_run());
    assert!(
        !terminal_false.ends_run(),
        "isTerminal: false means more work is scheduled"
    );

    // Only agent_end can end a run, terminal or not.
    assert!(!SessionEvent::decode(&json!({ "type": "turn_start" })).ends_run());
    assert!(!SessionEvent::decode(
        &json!({ "type": "turn_end", "message": {}, "toolResults": [] })
    )
    .ends_run());
}

#[test]
fn a_terminal_agent_end_reports_its_is_terminal_flag_as_given() {
    let SessionEvent::AgentEnd(end) =
        SessionEvent::decode(&json!({ "type": "agent_end", "messages": [], "isTerminal": false }))
    else {
        panic!("expected agent_end");
    };
    assert_eq!(end.is_terminal, Some(false));
    assert!(!end.is_terminal());
}

#[test]
fn tool_correlation_fields_are_typed_for_every_phase() {
    // The tool card keys off these three, so all three phases must expose the
    // same call id even though their payloads differ.
    let phases = [
        json!({ "type": "tool_execution_start", "toolCallId": "c1", "toolName": "bash", "args": {} }),
        json!({ "type": "tool_execution_update", "toolCallId": "c1", "toolName": "bash", "args": {}, "partialResult": {} }),
        json!({ "type": "tool_stream_update", "toolCallId": "c1", "toolName": "bash", "update": {} }),
        json!({ "type": "tool_execution_end", "toolCallId": "c1", "toolName": "bash", "result": {} }),
    ];

    for frame in phases {
        let event = SessionEvent::decode(&frame);
        let (tool_call_id, tool_name) = match &event {
            SessionEvent::ToolExecutionStart(p) => (&p.tool_call_id, &p.tool_name),
            SessionEvent::ToolExecutionUpdate(p) => (&p.tool_call_id, &p.tool_name),
            SessionEvent::ToolStreamUpdate(p) => (&p.tool_call_id, &p.tool_name),
            SessionEvent::ToolExecutionEnd(p) => (&p.tool_call_id, &p.tool_name),
            other => panic!("expected a tool event, got {other:?}"),
        };
        assert_eq!(tool_call_id, "c1");
        assert_eq!(tool_name, "bash");
    }
}

#[test]
fn a_tool_result_without_iserror_reads_as_success() {
    // `isError` is optional on the wire; every engine-side consumer reads an
    // absent value as falsy, so we collapse it the same way.
    let SessionEvent::ToolExecutionEnd(end) = SessionEvent::decode(
        &json!({ "type": "tool_execution_end", "toolCallId": "c1", "toolName": "bash", "result": {} }),
    ) else {
        panic!("expected tool_execution_end");
    };
    assert!(!end.is_error);

    let SessionEvent::ToolExecutionEnd(failed) = SessionEvent::decode(
        &json!({ "type": "tool_execution_end", "toolCallId": "c1", "toolName": "bash", "result": {}, "isError": true }),
    ) else {
        panic!("expected tool_execution_end");
    };
    assert!(failed.is_error);
}

#[test]
fn compaction_and_retry_progress_is_readable_for_the_status_line() {
    let SessionEvent::AutoCompactionStart(start) = SessionEvent::decode(
        &json!({ "type": "auto_compaction_start", "reason": "overflow", "action": "remote" }),
    ) else {
        panic!("expected auto_compaction_start");
    };
    assert_eq!(start.reason, "overflow");
    assert_eq!(start.action, "remote");

    let SessionEvent::AutoCompactionEnd(end) = SessionEvent::decode(
        &json!({ "type": "auto_compaction_end", "action": "remote", "aborted": true, "willRetry": false }),
    ) else {
        panic!("expected auto_compaction_end");
    };
    assert!(end.aborted);
    assert!(!end.will_retry);
    assert_eq!(end.result, None, "an aborted compaction reports no result");

    let SessionEvent::AutoRetryStart(retry) = SessionEvent::decode(
        &json!({ "type": "auto_retry_start", "attempt": 2, "maxAttempts": 5, "delayMs": 8000, "errorMessage": "429" }),
    ) else {
        panic!("expected auto_retry_start");
    };
    assert_eq!(
        (retry.attempt, retry.max_attempts, retry.delay_ms),
        (2, 5, 8000)
    );
    assert_eq!(retry.error_message, "429");
}

#[test]
fn a_notice_keeps_its_level_and_text_for_the_status_line() {
    let SessionEvent::Notice(notice) = SessionEvent::decode(
        &json!({ "type": "notice", "level": "error", "message": "login refused for `secret: true`" }),
    ) else {
        panic!("expected notice");
    };
    assert_eq!(notice.level, "error");
    assert_eq!(notice.message, "login refused for `secret: true`");
    assert_eq!(notice.source, None);
}

#[test]
fn a_thinking_level_change_distinguishes_effective_from_configured() {
    let SessionEvent::ThinkingLevelChanged(change) = SessionEvent::decode(
        &json!({ "type": "thinking_level_changed", "thinkingLevel": "medium", "configured": "auto", "resolved": "medium" }),
    ) else {
        panic!("expected thinking_level_changed");
    };
    assert_eq!(change.thinking_level.as_deref(), Some("medium"));
    assert_eq!(change.configured.as_deref(), Some("auto"));
    assert_eq!(change.resolved.as_deref(), Some("medium"));

    // An undefined level is legal: the session can have no level selected.
    let SessionEvent::ThinkingLevelChanged(none) =
        SessionEvent::decode(&json!({ "type": "thinking_level_changed" }))
    else {
        panic!("expected thinking_level_changed");
    };
    assert_eq!(none.thinking_level, None);
}

#[test]
fn a_delta_carries_the_text_to_append_and_the_block_it_belongs_to() {
    let samples = [
        (
            json!({ "type": "text_delta", "contentIndex": 0, "delta": "abc" }),
            "text_delta",
            Some(0),
            Some("abc"),
        ),
        (
            json!({ "type": "thinking_delta", "contentIndex": 1, "delta": "hm" }),
            "thinking_delta",
            Some(1),
            Some("hm"),
        ),
        (
            json!({ "type": "toolcall_delta", "contentIndex": 2, "delta": "{\"a\"" }),
            "toolcall_delta",
            Some(2),
            Some("{\"a\""),
        ),
    ];

    for (frame, kind, index, text) in samples {
        let delta = MessageDelta::decode(&frame);
        assert_eq!(delta.kind(), kind);
        match delta {
            MessageDelta::TextDelta {
                content_index,
                delta,
            }
            | MessageDelta::ThinkingDelta {
                content_index,
                delta,
            }
            | MessageDelta::ToolCallDelta {
                content_index,
                delta,
            } => {
                assert_eq!(Some(content_index), index);
                assert_eq!(Some(delta.as_str()), text);
            }
            other => panic!("expected a delta carrying text, got {other:?}"),
        }
    }
}

#[test]
fn a_malformed_delta_is_classified_against_the_delta_list() {
    // `toolcall_delta` is known but arrives without `contentIndex`, so this must
    // be reported as a payload mismatch rather than an unfamiliar type.
    let delta = MessageDelta::decode(&json!({ "type": "toolcall_delta", "delta": "{}" }));
    match delta {
        MessageDelta::Malformed { kind, reason, .. } => {
            assert_eq!(kind, "toolcall_delta");
            assert!(!reason.is_empty());
        }
        other => panic!("expected Malformed, got {other:?}"),
    }
}
