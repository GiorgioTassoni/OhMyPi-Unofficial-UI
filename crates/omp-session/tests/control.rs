//! Control-state conformance.
//!
//! The tricky part is not decoding `get_state` — it is knowing which events make
//! a decoded snapshot stale before the next poll. Getting that wrong shows a user
//! the wrong model, which is exactly the class of bug this file pins down.

use omp_session::SessionControl;
use omp_transport::events::SessionEvent;
use serde_json::json;

/// A state payload shaped like a real `get_state` response, trimmed to the
/// fields this crate reads.
fn state() -> serde_json::Value {
    json!({
        "sessionId": "sess_1",
        "sessionName": "parser work",
        "sessionFile": "/home/x/.omp/agent/sessions/p/abc.jsonl",
        "model": { "id": "claude-sonnet-4-5", "name": "Sonnet 4.5", "provider": "anthropic", "identity": { "family": "claude" } },
        "thinkingLevel": "high",
        "isStreaming": false,
        "isCompacting": false,
        "steeringMode": "one-at-a-time",
        "followUpMode": "all",
        "interruptMode": "immediate",
        "autoCompactionEnabled": true,
        "fastModeEnabled": false,
        "fastModeActive": false,
        "tokensPerSecond": null,
        "messageCount": 4,
        "queuedMessageCount": 0,
        "contextUsage": { "tokens": 12000, "contextWindow": 200000, "percent": 6 },
        "todoPhases": [
            { "name": "Implementation", "tasks": [
                { "content": "Write the reducer", "status": "in_progress" },
                { "content": "Wire the panel", "status": "blocked", "blocker": "needs the reducer" }
            ] }
        ],
        "dumpTools": [{ "name": "bash" }],
        "systemPrompt": ["you are a coding agent"]
    })
}

#[test]
fn a_state_snapshot_decodes_the_fields_the_chrome_renders() {
    let control = SessionControl::decode(&state()).expect("a session id makes it usable");

    assert_eq!(control.session_id, "sess_1");
    assert_eq!(control.session_name.as_deref(), Some("parser work"));
    assert!(control.session_file.is_some());
    assert_eq!(control.thinking_level.as_deref(), Some("high"));
    assert!(control.auto_compaction_enabled);
    assert_eq!(control.steering_mode, "one-at-a-time");
    assert_eq!(control.follow_up_mode, "all");
    assert_eq!(control.interrupt_mode, "immediate");
    assert_eq!(control.message_count, 4);
    assert_eq!(control.tokens_per_second, None, "null is not zero");

    let model = control.model.expect("model identity");
    assert_eq!(model.provider.as_deref(), Some("anthropic"));
    assert_eq!(model.id.as_deref(), Some("claude-sonnet-4-5"));
    assert_eq!(model.name.as_deref(), Some("Sonnet 4.5"));

    let usage = control.context_usage.expect("context usage");
    assert_eq!(usage.tokens, 12_000);
    assert_eq!(usage.context_window, 200_000);
    assert!((usage.percent - 6.0).abs() < f64::EPSILON);

    assert_eq!(control.todo_phases.len(), 1);
    let phase = &control.todo_phases[0];
    assert_eq!(phase.name, "Implementation");
    assert_eq!(phase.tasks.len(), 2);
    assert_eq!(phase.tasks[1].status, "blocked");
    assert_eq!(phase.tasks[1].blocker.as_deref(), Some("needs the reducer"));
}

#[test]
fn a_state_payload_without_a_session_id_is_rejected() {
    // Without identity the app cannot tell which thread it is looking at, so
    // rendering it would put a plausible but wrong session in the chrome.
    assert!(SessionControl::decode(&json!({ "isStreaming": false })).is_none());
}

#[test]
fn the_optional_halves_of_a_state_payload_degrade_cleanly() {
    let control = SessionControl::decode(&json!({ "sessionId": "s" })).expect("decode");

    assert_eq!(control.session_name, None);
    assert_eq!(control.model, None);
    assert_eq!(control.context_usage, None);
    assert!(control.todo_phases.is_empty());
    assert_eq!(control.message_count, 0);
    assert!(!control.is_streaming);
    assert!(
        control.thinking_level.is_none(),
        "no level selected is legal"
    );
}

#[test]
fn a_thinking_level_change_is_applied_from_the_event_itself() {
    // The event carries the new level, so waiting for a poll would show a stale
    // effort chip for as long as the poll interval.
    let mut control = SessionControl::decode(&state()).expect("decode");

    let stale = control.apply(&SessionEvent::decode(&json!({
        "type": "thinking_level_changed",
        "thinkingLevel": "low",
        "configured": "auto",
    })));

    assert!(!stale, "the level was carried, so nothing needs re-reading");
    assert_eq!(control.thinking_level.as_deref(), Some("low"));
}

#[test]
fn streaming_and_compaction_flags_track_their_events() {
    let mut control = SessionControl::decode(&state()).expect("decode");

    control.apply(&SessionEvent::decode(&json!({ "type": "agent_start" })));
    assert!(control.is_streaming);

    // A non-terminal end means more work is scheduled, so the UI must keep
    // showing a live turn.
    control.apply(&SessionEvent::decode(
        &json!({ "type": "agent_end", "messages": [], "isTerminal": false }),
    ));
    assert!(control.is_streaming, "isTerminal false is not a stop state");

    control.apply(&SessionEvent::decode(
        &json!({ "type": "agent_end", "messages": [], "isTerminal": true }),
    ));
    assert!(!control.is_streaming);

    control.apply(&SessionEvent::decode(&json!({
        "type": "auto_compaction_start",
        "reason": "threshold",
        "action": "context-full",
    })));
    assert!(control.is_compacting);

    control.apply(&SessionEvent::decode(&json!({
        "type": "auto_compaction_end",
        "action": "context-full",
        "aborted": false,
        "willRetry": true,
    })));
    assert!(!control.is_compacting);
}

#[test]
fn payloadless_notifications_ask_for_a_refresh_instead_of_guessing() {
    // `model_changed` says *something* changed but not what. Inventing a model
    // name would be worse than re-reading state.
    let mut control = SessionControl::decode(&state()).expect("decode");
    let before = control.model.clone().expect("model");

    assert!(
        control.apply(&SessionEvent::decode(&json!({ "type": "model_changed" }))),
        "the caller must be told to re-read get_state"
    );
    assert_eq!(control.model, Some(before), "nothing may be invented");

    assert!(control.apply(&SessionEvent::decode(
        &json!({ "type": "config_warnings_changed" })
    )));
    assert!(!control.apply(&SessionEvent::decode(&json!({ "type": "turn_start" }))));
}

#[test]
fn a_finished_todo_call_is_what_makes_the_plan_stale() {
    // The plan has no event of its own — measured, the engine's stream carries only
    // `todo_reminder` and `todo_auto_clear` — so the tool call's end is the only
    // signal there is, and a build that missed it would show a plan the engine has
    // already replaced.
    let mut control = SessionControl::decode(&state()).expect("decode");

    assert!(
        control.apply(&SessionEvent::decode(&json!({
            "type": "tool_execution_end",
            "toolCallId": "call_1",
            "toolName": "todo",
            "result": {
                "content": [{ "type": "text", "text": "2 tasks" }],
                "details": { "phases": [] },
            },
            "isError": false,
        }))),
        "a `todo` tool result is the plan moving, so the caller must re-read get_state"
    );

    // Every other tool leaves the plan where it was; re-reading for each of them would
    // be a poll loop by another name.
    for tool in ["bash", "edit", "read", "todolist"] {
        let event = SessionEvent::decode(&json!({
            "type": "tool_execution_end",
            "toolCallId": "call_2",
            "toolName": tool,
            "result": { "content": [] },
            "isError": false,
        }));

        assert!(
            !control.apply(&event),
            "`{tool}` does not touch the plan, so it must not ask for a re-read"
        );
    }
}
