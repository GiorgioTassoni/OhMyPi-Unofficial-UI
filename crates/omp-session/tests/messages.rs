//! Message-model conformance.
//!
//! The engine's `AgentMessage` is far wider than anything the UI renders —
//! provider replay payloads, signature blobs, a dozen internal block kinds. These
//! tests pin the line this crate draws: model what a renderer branches on,
//! preserve the rest verbatim, and never fail on an unfamiliar shape.

use omp_session::{ContentBlock, Message, MessageKind};
use serde_json::json;

#[test]
fn a_user_turn_may_carry_a_bare_string_or_blocks() {
    // Both shapes occur in real history; renderers must see one of them.
    let bare = Message::decode(&json!({
        "role": "user",
        "content": "fix the parser",
        "timestamp": 1,
    }));
    assert_eq!(
        bare.kind,
        MessageKind::User {
            synthetic: false,
            steering: false
        }
    );
    assert_eq!(bare.text(), "fix the parser");

    let blocks = Message::decode(&json!({
        "role": "user",
        "content": [{ "type": "text", "text": "look at " }, { "type": "text", "text": "this" }],
        "timestamp": 2,
    }));
    assert_eq!(blocks.text(), "look at this");
}

#[test]
fn an_assistant_turn_exposes_the_fields_that_decide_how_it_renders() {
    let message = Message::decode(&json!({
        "role": "assistant",
        "content": [
            { "type": "thinking", "thinking": "the user wants X" },
            { "type": "text", "text": "Done." },
        ],
        "provider": "anthropic",
        "model": "claude-sonnet-4-5",
        "usage": { "input": 10, "output": 2 },
        "stopReason": "stop",
        "timestamp": 3,
    }));

    match &message.kind {
        MessageKind::Assistant {
            stop_reason,
            error_message,
            model,
            provider,
            usage,
        } => {
            assert_eq!(stop_reason.as_deref(), Some("stop"));
            assert_eq!(error_message, &None);
            assert_eq!(model.as_deref(), Some("claude-sonnet-4-5"));
            assert_eq!(provider.as_deref(), Some("anthropic"));
            assert!(usage.is_some(), "token accounting must stay reachable");
        }
        other => panic!("expected an assistant message, got {other:?}"),
    }

    assert_eq!(message.text(), "Done.", "thinking is not what was said");
    assert_eq!(message.blocks.len(), 2);
}

#[test]
fn a_failed_assistant_turn_keeps_the_error_the_ui_must_show() {
    // Rendering a failed turn as an empty answer would hide a real failure.
    let message = Message::decode(&json!({
        "role": "assistant",
        "content": [],
        "stopReason": "error",
        "errorMessage": "429 rate limited",
        "timestamp": 4,
    }));

    match &message.kind {
        MessageKind::Assistant {
            stop_reason,
            error_message,
            ..
        } => {
            assert_eq!(stop_reason.as_deref(), Some("error"));
            assert_eq!(error_message.as_deref(), Some("429 rate limited"));
        }
        other => panic!("expected an assistant message, got {other:?}"),
    }
}

#[test]
fn tool_calls_are_extracted_for_card_construction() {
    let message = Message::decode(&json!({
        "role": "assistant",
        "content": [{
            "type": "toolCall",
            "id": "call_7",
            "name": "bash",
            "arguments": { "command": "ls" },
            "intent": "list files",
        }],
        "timestamp": 5,
    }));

    let calls: Vec<_> = message.tool_calls().collect();
    assert_eq!(calls.len(), 1);
    let (id, name, arguments) = calls[0];
    assert_eq!(id, "call_7");
    assert_eq!(name, "bash");
    assert_eq!(arguments["command"], "ls");
}

#[test]
fn a_tool_result_carries_its_correlation_and_failure_flag() {
    let message = Message::decode(&json!({
        "role": "toolResult",
        "toolCallId": "call_7",
        "toolName": "bash",
        "content": [{ "type": "text", "text": "hello" }],
        "isError": false,
        "timestamp": 6,
    }));

    match &message.kind {
        MessageKind::ToolResult {
            tool_call_id,
            tool_name,
            is_error,
        } => {
            assert_eq!(tool_call_id, "call_7");
            assert_eq!(tool_name, "bash");
            assert!(!is_error);
        }
        other => panic!("expected a tool result, got {other:?}"),
    }
    assert_eq!(message.text(), "hello");
}

#[test]
fn provider_internal_blocks_are_preserved_rather_than_dropped() {
    // Half the union is provider plumbing. It must survive a round trip through
    // the model even though nothing renders it yet.
    let message = Message::decode(&json!({
        "role": "assistant",
        "content": [
            { "type": "text", "text": "hi" },
            { "type": "redactedThinking", "data": "opaque" },
            { "type": "anthropicCompaction", "summary": "earlier work" },
        ],
        "timestamp": 7,
    }));

    assert_eq!(message.blocks.len(), 3);
    match &message.blocks[1] {
        ContentBlock::Other { kind, raw } => {
            assert_eq!(kind, "redactedThinking");
            assert_eq!(raw["data"], "opaque");
        }
        other => panic!("expected a preserved block, got {other:?}"),
    }
    assert_eq!(
        message.text(),
        "hi",
        "preserved blocks must not leak into the text"
    );
}

#[test]
fn an_unmodelled_role_is_kept_so_session_extensions_survive() {
    // Session extensions add their own roles (hidden rewind reports, and others).
    let message = Message::decode(&json!({
        "role": "rewindReport",
        "content": "files restored",
        "timestamp": 8,
    }));

    match &message.kind {
        MessageKind::Other { role } => assert_eq!(role, "rewindReport"),
        other => panic!("expected an unmodelled role, got {other:?}"),
    }
    assert_eq!(message.role(), "rewindReport");
    assert_eq!(message.text(), "files restored");
}

#[test]
fn a_half_recognised_message_still_decodes() {
    // History is engine-authored, so a missing or oddly typed field must degrade
    // the row rather than lose the turn.
    let message = Message::decode(&json!({ "role": "assistant" }));

    assert!(message.blocks.is_empty());
    assert_eq!(message.timestamp, None);
    assert_eq!(message.role(), "assistant");
}

#[test]
fn an_image_attachment_is_readable_and_an_unusable_one_is_preserved() {
    // `docs/12` §3.1 shows a user message's attachments as thumbnails, so the
    // bytes have to reach a renderer. A block shaped like an image but carrying
    // no payload must not become a thumbnail pointing at nothing.
    let message = Message::decode(&json!({
        "role": "user",
        "content": [
            { "type": "text", "text": "look at this" },
            { "type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png" },
            { "type": "image", "mimeType": "image/png" },
        ],
        "timestamp": 1,
    }));

    assert_eq!(message.text(), "look at this", "text() ignores attachments");
    assert_eq!(
        message.images().collect::<Vec<_>>(),
        vec![("image/png", "iVBORw0KGgo=")]
    );
    assert!(
        matches!(
            &message.blocks[2],
            omp_session::ContentBlock::Other { kind, raw }
                if kind == "image" && raw.get("mimeType").is_some()
        ),
        "an image block with no data must stay reachable: {:?}",
        message.blocks
    );
}
